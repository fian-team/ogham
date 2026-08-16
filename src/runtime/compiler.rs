//! Bytecode compiler: walks the AST and emits [`OpCode`] instructions into
//! a [`Chunk`] that the VM can execute.

use std::sync::Arc;

use crate::parser::{
    Block, Call, Expression, ForLoopExpression, Function, Literal, MatchExpression, Operator,
    Statement, SyntaxError,
};
use crate::runtime::error::VMError;
use crate::runtime::opcode::{Chunk, FunctionProto, ImportMeta, OpCode, UpvalueDescriptor};
use crate::runtime::schema::ModuleSchema;
use crate::runtime::value::Value;

/// Names of identifiers that are always available in strict mode
/// without being declared in `host_state`, parameters, or `state`.
/// Kept as a single source of truth so the LSP's completion (M3)
/// can read the same list.
pub(crate) const BUILTINS: &[&str] = &["event", "use_context", "rgb", "rgba", "true", "false"];

/// Host-state key carrying the active route path: an array of screen ids,
/// outermost first. The host owns it; a document reads it only through
/// `outlet`, never by name (`ogham INTENT §10` — ogham never navigates
/// itself).
pub const ROUTE_PATH_KEY: &str = "__route_path";

/// The module-level local a screen's view compiles to.
///
/// Named by *index* rather than by id, because a screen id is a route id
/// and route ids are not ogham identifiers — `map-edit` would scan as
/// three tokens. The index is the screen's position in the schema's
/// (sorted) map, so it is stable across the two places that compute it.
fn screen_fn_name(index: usize) -> String {
    format!("__ogh_screen_{}", index)
}

/// The host-state key a screen's own `state` field reads.
///
/// `"<id>::<field>"`. Two screens may both declare `rows`; this is why
/// neither can see the other's.
pub fn scoped_key(id: &str, field: &str) -> String {
    format!("{}::{}", id, field)
}

/// The placeholder `outlet`, compiled before the module body so that a
/// `main` written above the dispatcher still resolves the name.
const OUTLET_FORWARD_DECL: &str = "let outlet = fn () { Flex { style: {} } };";

/// Source for the real dispatcher, built from the module's screen ids.
///
/// Generated rather than hand-emitted because the alternative is a lot of
/// bytecode for a for-loop and a match, and because generating it means
/// the feature is expressed in the language it extends — if this source
/// does not compile, the language cannot express routing and that is worth
/// finding out loudly.
///
/// The stack is rendered outermost-first, so a deeper route draws over a
/// shallower one. Which ids are *in* the path is the host's decision
/// (occlusion is the router's, not the document's).
fn outlet_source(screen_ids: &[String]) -> String {
    let arms: String = screen_ids
        .iter()
        .enumerate()
        .map(|(i, id)| format!("    {:?} => {}(),\n", id, screen_fn_name(i)))
        .collect();
    format!(
        "let __ogh_dispatch = fn (__ogh_id: string) {{
  match __ogh_id {{
{arms}    _ => Flex {{ style: {{}} }},
  }}
}};
outlet = fn () {{
  Flex {{
    style: {{ width: \"grow\", height: \"grow\" }},
    block_interactions: false,
    children: for (__ogh_i in 0..{path}.length()) {{
      __ogh_dispatch({path}[__ogh_i])
    }},
  }}
}};",
        arms = arms,
        path = ROUTE_PATH_KEY,
    )
}

/// Scan and parse a synthesized snippet into top-level statements.
///
/// A failure here is a compiler bug, not a user error, so it surfaces as
/// an `InvalidOperation` naming the snippet rather than as a syntax error
/// pointing at a line the author never wrote.
fn parse_synthetic(src: &str) -> Result<Vec<Statement>, VMError> {
    let tokens = crate::scanner::Scanner::new(src.to_string()).scan();
    let module = crate::parser::Parser::new(tokens).parse().map_err(|e| {
        VMError::InvalidOperation(format!(
            "internal: synthesized routing source failed to parse ({e:?}); source was:\n{src}"
        ))
    })?;
    Ok(module.body.statement_list)
}

/// Render a `Vec<TypeRef>` for display in event-signature
/// diagnostics. e.g. `[Int, String]` → `"int, string"`.
fn type_args_for_display(args: &[crate::parser::typed_bindings::TypeRef]) -> String {
    args.iter()
        .map(format_type_ref)
        .collect::<Vec<_>>()
        .join(", ")
}

fn format_type_ref(ty: &crate::parser::typed_bindings::TypeRef) -> String {
    ty.to_canonical_string()
}

// ---------------------------------------------------------------------------
// Local – a compile-time record for a local variable on the stack.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct Local {
    name: String,
    depth: u32,
    is_captured: bool,
    /// Whether this is a state variable (declared with `state`).
    is_state: bool,
    /// Actual stack position relative to the frame base.
    slot: u8,
}

// ---------------------------------------------------------------------------
// Compiler
// ---------------------------------------------------------------------------

pub struct Compiler {
    pub function: FunctionProto,
    locals: Vec<Local>,
    upvalue_descs: Vec<UpvalueDescriptor>,
    scope_depth: u32,
    /// The enclosing compiler (used for upvalue resolution across nested fns).
    enclosing: Option<Box<Compiler>>,
    /// Current source line (for error reporting in emitted bytecode).
    current_line: usize,
    /// Tracks actual number of values on the stack relative to frame base.
    stack_depth: usize,
    /// Module schema, attached only when compiling a top-level
    /// module. Child compilers (nested fns) inherit a clone via
    /// the `Arc` so they share strict-mode state without recursive
    /// ownership headaches. `None` means loose mode — the compiler
    /// emits the same bytecode it always has.
    schema: Option<Arc<ModuleSchema>>,
    /// Top-level names this module's imports provide, pre-scanned by
    /// the runtime (which alone can resolve import sources) and passed
    /// into [`Self::compile_module_with_imports`]. Strict-mode
    /// identifier resolution accepts these — the runtime import copies
    /// them into the environment, so a strict module referencing an
    /// imported helper is exactly the promise the strict-mode
    /// diagnostic makes ("… state, imports, records, and built-ins").
    /// Empty in loose mode and for callers with nothing to pre-scan.
    import_values: Arc<std::collections::BTreeSet<String>>,
    /// The id of the `screen` whose view is currently being compiled, if
    /// any. Inherited by child compilers, because a view's helpers are
    /// nested `fn`s and a screen's slice must be readable from inside
    /// them.
    ///
    /// This is the whole of scoped host state at compile time: a name
    /// that is one of this screen's `state` fields is emitted as the
    /// namespaced key `"<id>::<field>"`, so two screens may both declare
    /// `rows` and neither can see the other's. A name that is not
    /// resolves as it always did — to the module's `host_state {}`.
    current_screen: Option<String>,
}

impl Compiler {
    // -- Construction -------------------------------------------------------

    pub fn new(name: String, arity: u8) -> Self {
        Self {
            function: FunctionProto::new(name, arity),
            locals: Vec::new(),
            upvalue_descs: Vec::new(),
            scope_depth: 0,
            enclosing: None,
            current_line: 0,
            stack_depth: 0,
            schema: None,
            import_values: Arc::new(std::collections::BTreeSet::new()),
            current_screen: None,
        }
    }

    /// Create a child compiler for a nested function and move `self` into
    /// its `enclosing` slot. Returns the child. The child inherits the
    /// parent's schema (cheaply via `Arc::clone`) so strict-mode
    /// resolution applies inside nested closures too.
    fn child(self, name: String, arity: u8) -> Self {
        let schema = self.schema.clone();
        let import_values = self.import_values.clone();
        let current_screen = self.current_screen.clone();
        Self {
            function: FunctionProto::new(name, arity),
            locals: Vec::new(),
            upvalue_descs: Vec::new(),
            scope_depth: 0,
            enclosing: Some(Box::new(self)),
            current_line: 0,
            stack_depth: 0,
            schema,
            import_values,
            current_screen,
        }
    }

    /// True iff the module being compiled has declared *any*
    /// schema block (host_state or events). Used for event-call
    /// validation.
    fn is_strict(&self) -> bool {
        self.schema.as_ref().map(|s| s.is_strict()).unwrap_or(false)
    }

    /// True iff the module being compiled has declared
    /// `host_state {}`. Used for strict identifier resolution
    /// (which requires a known list of valid host_state fields).
    fn has_host_state_schema(&self) -> bool {
        self.schema
            .as_ref()
            .map(|s| s.has_host_state())
            .unwrap_or(false)
    }

    /// In strict mode, decide whether `name` resolves to something
    /// the body is allowed to reference: a local, an upvalue,
    /// a host_state field, a declared/imported record, or a built-in.
    /// Locals/upvalues are pre-checked by the caller (we already
    /// tried `resolve_local` and `resolve_upvalue` before reaching
    /// this), so this only checks the schema-level slots.
    fn is_known_in_schema(&self, name: &str) -> bool {
        if BUILTINS.contains(&name) {
            return true;
        }
        if self.import_values.contains(name) {
            return true;
        }
        let Some(schema) = self.schema.as_ref() else {
            return false;
        };
        if name == ROUTE_PATH_KEY {
            return true;
        }
        if self.screen_field(name).is_some() {
            return true;
        }
        if let Some(hs) = &schema.host_state {
            if hs.fields.contains_key(name) {
                return true;
            }
        }
        if schema.lookup_record(name).is_some() {
            return true;
        }
        false
    }

    /// If `name` is a `state` field of the screen currently being
    /// compiled, the namespaced host-state key it reads.
    ///
    /// Returning `None` for a name outside any screen — or for a name a
    /// screen did not declare — is what makes the scoping work in both
    /// directions: the field falls through to the module's
    /// `host_state {}`, and a screen's private field is simply not a
    /// name anywhere else, so it fails strict resolution rather than
    /// silently reading a neighbour's value.
    fn screen_field(&self, name: &str) -> Option<String> {
        let id = self.current_screen.as_ref()?;
        let schema = self.schema.as_ref()?;
        let screen = schema.screens.get(id.as_str())?;
        screen
            .state
            .fields
            .contains_key(name)
            .then(|| scoped_key(id, name))
    }

    /// Build a strict-mode "unknown identifier" diagnostic, with a
    /// levenshtein-1 suggestion when one is available.
    fn strict_unknown_identifier(&self, name: &str, line: usize, column: usize) -> SyntaxError {
        let mut err = SyntaxError::new(line, column, format!("unknown identifier `{}`", name))
            .with_length(name.len())
            .with_note(
                "this module declares `host_state {}`; identifiers resolve only to \
                 declared fields, locals, parameters, state, imports, records, \
                 and built-ins",
            );
        if let Some(suggestion) = self.suggest_identifier(name) {
            err = err.with_help(format!("did you mean `{}`?", suggestion));
        }
        err
    }

    /// Strict-mode validation for an `event("name", arg, ...)` call:
    ///
    /// 1. The first arg must be a string *literal* — computed event
    ///    names defeat the whole point of declared schemas.
    /// 2. The literal must name a declared event.
    /// 3. The number of extra args must match the declared signature
    ///    (excluding the name itself).
    ///
    /// Argument *types* are not checked here. The plan's stretch goal
    /// (best-effort: bare-identifier args whose type is statically
    /// known) is deferred to a later sub-merge — production code's
    /// most common bug is name typos and arg-count mismatches, which
    /// this catches.
    fn check_event_call(
        &self,
        call: &Call,
        callee_ident: &crate::parser::Identifier,
    ) -> Result<(), VMError> {
        let schema = self
            .schema
            .as_ref()
            .expect("check_event_call requires strict mode");

        // (1) name must be a string literal
        let Some(first_arg) = call.arguments.first() else {
            return Err(VMError::StrictMode(
                SyntaxError::new(
                    callee_ident.span.start_line,
                    callee_ident.span.start_column,
                    "`event(...)` requires at least an event name",
                )
                .with_length(5)
                .with_note("declare events with `events { name(...) };` first"),
            ));
        };
        let (event_name, name_span) = match first_arg {
            Expression::Literal(Literal::String(s, span)) => (s.clone(), *span),
            other => {
                return Err(VMError::StrictMode(
                    SyntaxError::new(
                        callee_ident.span.start_line,
                        callee_ident.span.start_column,
                        "computed event names are not allowed in strict mode",
                    )
                    .with_length(5)
                    .with_note(
                        "the first argument to `event()` must be a string literal \
                         so the schema can validate the call statically",
                    )
                    .with_help(format!(
                        "replace the dynamic value with a literal: \
                         `event(\"<name>\", ...)`. Saw: {:?}",
                        std::mem::discriminant(other)
                    )),
                ));
            }
        };

        // (2) name must be declared
        let Some(sig) = schema.events.get(&event_name) else {
            let candidates: Vec<&str> = schema.events.keys().map(|s| s.as_str()).collect();
            let mut err = SyntaxError::new(
                name_span.start_line,
                name_span.start_column,
                format!("unknown event `{}`", event_name),
            )
            .with_length(event_name.len() + 2) // include quotes
            .with_note("only events declared in `events { ... }` may be emitted");
            if let Some(suggestion) =
                crate::runtime::schema::levenshtein_1_pub(&event_name, &candidates)
            {
                err = err.with_help(format!("did you mean `{}`?", suggestion));
            } else if !candidates.is_empty() {
                err = err.with_help(format!("declared events: {}", candidates.join(", ")));
            }
            return Err(VMError::StrictMode(err));
        };

        // (3) arg count (excluding the name itself) must match
        let actual_args = call.arguments.len().saturating_sub(1);
        let expected_args = sig.args.len();
        if actual_args != expected_args {
            return Err(VMError::StrictMode(
                SyntaxError::new(
                    callee_ident.span.start_line,
                    callee_ident.span.start_column,
                    format!(
                        "wrong number of arguments to `event(\"{}\", ...)`",
                        event_name
                    ),
                )
                .with_length(5)
                .with_note(format!(
                    "declared signature: {}({})",
                    event_name,
                    type_args_for_display(&sig.args)
                ))
                .with_help(format!(
                    "expected {} argument{}, got {}",
                    expected_args,
                    if expected_args == 1 { "" } else { "s" },
                    actual_args
                )),
            ));
        }
        Ok(())
    }

    /// Suggest an identifier within Levenshtein-1 of `name` from
    /// the union of known-in-scope names. Walks built-ins,
    /// host_state fields, and declared/imported record names.
    fn suggest_identifier(&self, name: &str) -> Option<String> {
        let mut candidates: Vec<&str> = BUILTINS.to_vec();
        if let Some(schema) = self.schema.as_ref() {
            if let Some(hs) = &schema.host_state {
                candidates.extend(hs.fields.keys().map(|s| s.as_str()));
            }
            candidates.extend(schema.records.keys().map(|s| s.as_str()));
            candidates.extend(schema.imports.keys().map(|s| s.as_str()));
        }
        candidates.extend(self.import_values.iter().map(|s| s.as_str()));
        // Locals are also valid candidates but only at the level
        // we're compiling at; including them is best-effort.
        for local in &self.locals {
            candidates.push(local.name.as_str());
        }
        crate::runtime::schema::levenshtein_1_pub(name, &candidates).map(String::from)
    }

    /// Finish compiling a child and return the enclosing (parent) compiler
    /// together with the finished `FunctionProto`.
    fn finish_child(mut self) -> (Box<Compiler>, FunctionProto) {
        // Implicit return Void at the end of every function.
        self.emit(OpCode::Void);
        self.emit(OpCode::Return);
        self.function.upvalue_count = self.upvalue_descs.len() as u8;
        self.function.upvalues = self.upvalue_descs;
        let parent = self
            .enclosing
            .expect("finish_child called on top-level compiler");
        (parent, self.function)
    }

    // -- Helpers: emit bytecode ---------------------------------------------

    fn chunk(&mut self) -> &mut Chunk {
        &mut self.function.chunk
    }

    fn emit(&mut self, op: OpCode) -> usize {
        let effect = Self::stack_effect(&op);
        let line = self.current_line;
        let idx = self.chunk().emit(op, line);
        self.stack_depth = (self.stack_depth as i32 + effect) as usize;
        idx
    }

    /// Returns the net stack effect of an opcode (positive = pushes, negative = pops).
    fn stack_effect(op: &OpCode) -> i32 {
        match op {
            // Push one value
            OpCode::Constant(_)
            | OpCode::True
            | OpCode::False
            | OpCode::Void
            | OpCode::GetLocal(_)
            | OpCode::GetUpvalue(_)
            | OpCode::GetState(_)
            | OpCode::GetHostState(_)
            | OpCode::Dup
            | OpCode::BeginForExpr
            | OpCode::Closure(_) => 1,

            // Pop one value
            OpCode::Pop
            | OpCode::CloseUpvalue
            | OpCode::SetLocal(_)
            | OpCode::SetUpvalue(_)
            | OpCode::AppendForExpr
            | OpCode::SpreadForExpr
            | OpCode::JumpIfFalse(_) => -1,

            // Pop the trailing result + N locals, push the result back.
            OpCode::EndExprScope(n) => -(*n as i32),

            // Pop 1, push 1 (net 0)
            OpCode::Negate
            | OpCode::Not
            | OpCode::ArrayLength
            | OpCode::GetProperty(_)
            | OpCode::Log
            | OpCode::DeclareState(_) => 0,

            // Peek only (net 0)
            OpCode::SetState(_) | OpCode::MarkBranched | OpCode::Import(_) => 0,

            // No stack effect
            OpCode::Jump(_) | OpCode::Loop(_) | OpCode::Return => 0,

            // Pop 2, push 1 (net -1)
            OpCode::Add
            | OpCode::Sub
            | OpCode::Mul
            | OpCode::Div
            | OpCode::Mod
            | OpCode::Pow
            | OpCode::Eq
            | OpCode::Ne
            | OpCode::Gt
            | OpCode::Ge
            | OpCode::Lt
            | OpCode::Le
            | OpCode::And
            | OpCode::Or
            | OpCode::ArrayJoin
            | OpCode::GetIndex => -1,

            // Pop 2n key-value pairs, push 1
            OpCode::Map(n) => -(2 * *n as i32 - 1),
            OpCode::Widget { property_count, .. } => -(2 * *property_count as i32 - 1),

            // Pop n, push 1
            OpCode::Array(n) => -(*n as i32 - 1),

            // Pop callee + n args, push 1 result
            OpCode::Call(n) => -(*n as i32),

            // Pop n args (including name), push Void
            OpCode::EmitEvent(n) => -(*n as i32 - 1),

            // Pop (name, value), no push
            OpCode::PushContext => -2,
            // No stack effect — pops from the runtime side-channel
            OpCode::PopContext => 0,
            // Push the looked-up context value
            OpCode::GetContext(_) => 1,
        }
    }

    fn emit_constant(&mut self, value: Value) -> u16 {
        let idx = self.chunk().add_constant(value);
        self.emit(OpCode::Constant(idx));
        idx
    }

    fn emit_jump(&mut self, op: OpCode) -> usize {
        self.emit(op) // placeholder offset; patched later
    }

    fn patch_jump(&mut self, idx: usize) {
        self.function.chunk.patch_jump(idx);
    }

    fn emit_loop(&mut self, loop_start: usize) {
        // +1 because we want to jump *back* to loop_start, and the offset
        // is applied after ip has advanced past the Loop instruction.
        let offset = self.function.chunk.code.len() - loop_start + 1;
        self.emit(OpCode::Loop(offset as u16));
    }

    // -- Helpers: scope management ------------------------------------------

    fn begin_scope(&mut self) {
        self.scope_depth += 1;
    }

    fn end_scope(&mut self) {
        self.scope_depth -= 1;
        // Pop / close locals that belong to the scope we just left.
        while let Some(local) = self.locals.last() {
            if local.depth <= self.scope_depth {
                break;
            }
            // We don't pop state locals – they persist in the Runtime's
            // component_state map. But we still remove them from the compiler's
            // locals list so the slots are freed.
            if self.locals.last().unwrap().is_captured {
                // Close the upvalue before popping the local off the stack,
                // so that any closure still holding a reference to it gets
                // the value snapshot rather than a dangling stack index.
                self.emit(OpCode::CloseUpvalue);
            } else {
                self.emit(OpCode::Pop);
            }
            self.locals.pop();
        }
    }

    /// End a scope whose body left a trailing expression value at the top of
    /// the stack. Plain `end_scope` would emit `Pop` / `CloseUpvalue` against
    /// the top, clobbering the result and leaving captured-local upvalues
    /// open at slots that are about to be invalidated. Instead we emit a
    /// single `EndExprScope(n)` that closes upvalues for the N locals below
    /// the result and drops them in one shot.
    fn end_scope_preserving_result(&mut self) {
        self.scope_depth -= 1;
        let mut n: u8 = 0;
        while let Some(local) = self.locals.last() {
            if local.depth <= self.scope_depth {
                break;
            }
            self.locals.pop();
            n = n.checked_add(1).expect("scope local count overflowed u8");
        }
        if n > 0 {
            self.emit(OpCode::EndExprScope(n));
        }
    }

    /// Record a local for a value that was already pushed onto the stack by
    /// compiled code (e.g. `let` initializers, for-loop bounds). The slot is
    /// `stack_depth - 1` because `emit` already incremented `stack_depth`.
    fn add_local(&mut self, name: String, is_state: bool) -> u8 {
        let slot = (self.stack_depth - 1) as u8;
        self.locals.push(Local {
            name,
            depth: self.scope_depth,
            is_captured: false,
            is_state,
            slot,
        });
        slot
    }

    /// Record a local for a value that is pre-existing on the stack (e.g.
    /// the callee placeholder and function parameters). Unlike `add_local`,
    /// this also increments `stack_depth` because no `emit` call placed the
    /// value.
    fn add_param_local(&mut self, name: String) -> u8 {
        let slot = self.stack_depth as u8;
        self.stack_depth += 1;
        self.locals.push(Local {
            name,
            depth: self.scope_depth,
            is_captured: false,
            is_state: false,
            slot,
        });
        slot
    }

    /// Resolve an identifier as a local in the *current* function.
    /// Returns the **stack slot** (not the array index) if found.
    fn resolve_local(&self, name: &str) -> Option<u8> {
        for local in self.locals.iter().rev() {
            if local.name == name {
                return Some(local.slot);
            }
        }
        None
    }

    /// Check whether a local variable (by name) is a state variable.
    fn is_local_state(&self, name: &str) -> bool {
        self.locals
            .iter()
            .rev()
            .find(|l| l.name == name)
            .map_or(false, |l| l.is_state)
    }

    /// Mark a local variable (by name) as captured by a closure.
    fn mark_local_captured(&mut self, name: &str) {
        for local in self.locals.iter_mut().rev() {
            if local.name == name {
                local.is_captured = true;
                return;
            }
        }
    }

    /// Resolve an identifier as an upvalue (captured from an enclosing
    /// function). Returns the upvalue index if found.
    fn resolve_upvalue(&mut self, name: &str) -> Option<u8> {
        // Take enclosing out temporarily so we can recurse.
        let mut enclosing = match self.enclosing.take() {
            Some(e) => e,
            None => return None,
        };

        // Is it a local in the immediately enclosing function?
        if let Some(local_slot) = enclosing.resolve_local(name) {
            enclosing.mark_local_captured(name);
            let idx = self.add_upvalue(UpvalueDescriptor::Local(local_slot));
            self.enclosing = Some(enclosing);
            return Some(idx);
        }

        // Is it an upvalue in the enclosing function (transitive capture)?
        if let Some(upvalue_idx) = enclosing.resolve_upvalue(name) {
            let idx = self.add_upvalue(UpvalueDescriptor::Upvalue(upvalue_idx));
            self.enclosing = Some(enclosing);
            return Some(idx);
        }

        self.enclosing = Some(enclosing);
        None
    }

    fn add_upvalue(&mut self, desc: UpvalueDescriptor) -> u8 {
        // Check if we already have this exact upvalue.
        for (i, existing) in self.upvalue_descs.iter().enumerate() {
            let matches = match (existing, &desc) {
                (UpvalueDescriptor::Local(a), UpvalueDescriptor::Local(b)) => a == b,
                (UpvalueDescriptor::Upvalue(a), UpvalueDescriptor::Upvalue(b)) => a == b,
                _ => false,
            };
            if matches {
                return i as u8;
            }
        }
        let idx = self.upvalue_descs.len() as u8;
        self.upvalue_descs.push(desc);
        idx
    }

    // -----------------------------------------------------------------------
    // Public entry point
    // -----------------------------------------------------------------------

    /// Compile a top-level module (the `Function` returned by the parser for
    /// the whole file). If the module declares `host_state {}`, the
    /// compiler runs strict-mode resolution: identifier references,
    /// field access, and `event(...)` calls are validated against the
    /// declared schema. Strict-mode violations surface as
    /// `VMError::StrictMode(SyntaxError)` carrying rich diagnostics.
    pub fn compile_module(module: &Function) -> Result<FunctionProto, VMError> {
        Self::compile_module_with_imports(module, std::collections::BTreeSet::new())
    }

    /// [`Self::compile_module`], with the top-level names this module's
    /// imports provide (pre-scanned by the runtime, which alone can
    /// resolve import sources). Strict-mode identifier resolution
    /// accepts them, keeping `host_state {}` modules able to compose
    /// shared `.ogh` fragments.
    pub fn compile_module_with_imports(
        module: &Function,
        import_values: std::collections::BTreeSet<String>,
    ) -> Result<FunctionProto, VMError> {
        // Build the module schema first. Loose-mode modules return
        // a schema with `host_state == None`; strict-mode modules
        // return a fully-resolved schema. Either way, the compiler
        // attaches it so identifier resolution can consult it.
        let schema = ModuleSchema::from_module(module).map_err(VMError::StrictMode)?;
        let screen_ids: Vec<String> = schema.screens.keys().cloned().collect();
        let mut compiler = Compiler::new("<module>".to_string(), 0);
        compiler.schema = Some(Arc::new(schema));
        compiler.import_values = Arc::new(import_values);

        // `outlet` is forward-declared, because `main` is written last and
        // the dispatcher it calls can only be built once every screen's
        // closure exists. A module-level slot stays an *open* upvalue for
        // the whole module frame, so `main` captures the slot and reads
        // whatever is in it when it finally runs — which is the real
        // dispatcher, assigned below.
        if !screen_ids.is_empty() {
            for stmt in &parse_synthetic(OUTLET_FORWARD_DECL)? {
                compiler.compile_statement(stmt, false)?;
            }
        }

        compiler.compile_block(&module.body)?;

        if !screen_ids.is_empty() {
            let src = outlet_source(&screen_ids);
            for stmt in &parse_synthetic(&src)? {
                compiler.compile_statement(stmt, false)?;
            }
        }

        // After executing the module body, look up `main` and call it.
        // We emit this as: GetLocal/GetUpvalue for "main", Call(0), Return.
        // But since `main` is always a module-level local, we resolve it
        // directly.
        if let Some(slot) = compiler.resolve_local("main") {
            compiler.emit(OpCode::GetLocal(slot));
            compiler.emit(OpCode::Call(0));
        } else {
            compiler.emit(OpCode::Void);
        }
        compiler.emit(OpCode::Return);

        compiler.function.upvalue_count = compiler.upvalue_descs.len() as u8;
        compiler.function.upvalues = compiler.upvalue_descs;
        Ok(compiler.function)
    }

    /// Compile an imported module. Unlike [`compile_module`](Self::compile_module),
    /// this does not look up or call a `main` function. Instead it returns the
    /// top-level local name-to-slot mapping so the caller can extract exported
    /// bindings from the VM stack after execution. Imported modules also get
    /// schema resolution (so an imported file's strict-mode errors surface).
    pub fn compile_import(
        module: &Function,
    ) -> Result<(FunctionProto, Vec<(String, u8)>), VMError> {
        let schema = ModuleSchema::from_module(module).map_err(VMError::StrictMode)?;
        let mut compiler = Compiler::new("<import>".to_string(), 0);
        compiler.schema = Some(Arc::new(schema));
        compiler.compile_block(&module.body)?;

        let local_names: Vec<(String, u8)> = compiler
            .locals
            .iter()
            .filter(|l| l.depth == 0)
            .map(|l| (l.name.clone(), l.slot))
            .collect();

        compiler.emit(OpCode::Void);
        compiler.emit(OpCode::Return);

        compiler.function.upvalue_count = compiler.upvalue_descs.len() as u8;
        compiler.function.upvalues = compiler.upvalue_descs;
        Ok((compiler.function, local_names))
    }

    // -----------------------------------------------------------------------
    // Block / Statement compilation
    // -----------------------------------------------------------------------

    fn compile_block(&mut self, block: &Block) -> Result<(), VMError> {
        let stmts = &block.statement_list;
        let last_idx = if stmts.is_empty() {
            None
        } else {
            Some(stmts.len() - 1)
        };

        for (i, stmt) in stmts.iter().enumerate() {
            let is_last = Some(i) == last_idx;
            self.compile_statement(stmt, is_last)?;
        }
        Ok(())
    }

    /// Compile a block that should produce an expression value on the stack.
    ///
    /// The parser wraps the last expression in a block (when not followed by
    /// a semicolon) in `Statement::Return`. The AST interpreter catches
    /// `VMError::Return` at the match/for-loop level and converts it to a
    /// normal value. In the bytecode VM, however, `OpCode::Return` exits the
    /// entire function frame. This helper strips the `Return` from the last
    /// statement, compiling only the inner expression so the value is left on
    /// the stack without exiting the function.
    fn compile_expression_block(&mut self, block: &Block) -> Result<(), VMError> {
        let stmts = &block.statement_list;
        if stmts.is_empty() {
            self.emit(OpCode::Void);
            return Ok(());
        }
        let last_idx = stmts.len() - 1;
        for (i, stmt) in stmts.iter().enumerate() {
            let is_last = i == last_idx;
            if is_last {
                match stmt {
                    Statement::Return(ret) => {
                        // Produce the value without exiting the function.
                        if let Some(expr) = ret.get_value() {
                            self.compile_expression(&expr)?;
                        } else {
                            self.emit(OpCode::Void);
                        }
                    }
                    _ => self.compile_statement(stmt, true)?,
                }
            } else {
                self.compile_statement(stmt, false)?;
            }
        }
        Ok(())
    }

    fn compile_statement(&mut self, statement: &Statement, is_last: bool) -> Result<(), VMError> {
        self.current_line = statement.span().start_line;
        match statement {
            Statement::Expression(expr_stmt) => {
                self.compile_expression(&expr_stmt.get_value())?;
                if is_last {
                    // Last expression in a block – leave value on the stack
                    // (implicit return). The caller decides whether to pop or
                    // return it.
                } else {
                    self.emit(OpCode::Pop);
                }
            }
            Statement::Declare(decl) => {
                let name = decl.get_identifier_value();
                // When the initializer is a function literal, thread the
                // binding name into the function so its body can refer to
                // itself (self-recursion). The binding name is bound to the
                // callee's slot 0; see `compile_function`.
                match decl.get_value() {
                    Expression::Literal(Literal::Function(func)) => {
                        self.compile_function(&func, &name)?;
                    }
                    value => {
                        self.compile_expression(&value)?;
                    }
                }
                self.add_local(name, false);
            }
            Statement::DeclareState(state_decl) => {
                let name = state_decl.get_identifier_value();
                // Push the name as a constant so the VM can work with it.
                let name_const = self.chunk().add_constant(Value::String(name.clone()));
                // Compile the initializer expression (default value).
                self.compile_expression(&state_decl.get_value())?;
                // Emit DeclareState – the VM will either use the initializer
                // or substitute the persisted value.
                self.emit(OpCode::DeclareState(name_const));
                // The result value is left on the stack as a local.
                self.add_local(name, true);
            }
            Statement::Assign(assign) => {
                let name = assign.get_identifier_value();
                self.compile_expression(&assign.get_value())?;

                // Try local first.
                if let Some(slot) = self.resolve_local(&name) {
                    // If this is a state variable we also need to update
                    // the runtime state map.
                    if self.is_local_state(&name) {
                        let name_const = self.chunk().add_constant(Value::String(name.clone()));
                        self.emit(OpCode::SetState(name_const));
                    }
                    self.emit(OpCode::SetLocal(slot));
                } else if let Some(uv) = self.resolve_upvalue(&name) {
                    // Check if the upvalue is a state variable.
                    // We need a name constant either way for state.
                    let name_const = self.chunk().add_constant(Value::String(name.clone()));
                    // We always emit SetState in case it's state; the VM will
                    // check at runtime.
                    self.emit(OpCode::SetState(name_const));
                    self.emit(OpCode::SetUpvalue(uv));
                } else {
                    return Err(VMError::UndefinedVariable(name));
                }
            }
            Statement::Return(ret) => {
                self.emit(OpCode::MarkBranched);
                if let Some(expr) = &ret.get_value() {
                    self.compile_expression(expr)?;
                } else {
                    self.emit(OpCode::Void);
                }
                self.emit(OpCode::Return);
            }
            Statement::Conditional(cond) => {
                self.emit(OpCode::MarkBranched);
                self.compile_conditional(cond)?;
            }
            Statement::Log(log_stmt) => {
                self.compile_expression(&log_stmt.get_value())?;
                self.emit(OpCode::Log);
            }
            Statement::Import(import_stmt) => {
                self.compile_import_stmt(import_stmt)?;
            }
            Statement::ForLoop(for_loop) => {
                self.emit(OpCode::MarkBranched);
                self.compile_for_loop_statement(for_loop)?;
            }
            // Typed-bindings declarations (Phase 1) are pure schema
            // metadata: they're consumed by the resolver and the LSP,
            // not the VM. The compiler emits no bytecode for them and
            // they leave nothing on the stack. If `is_last` is true,
            // an empty block-result of `Void` is implicitly produced
            // by the surrounding block-compile path; declarations don't
            // change that.
            Statement::RecordDeclaration(_)
            | Statement::HostStateDeclaration(_)
            | Statement::EventsDeclaration(_) => {
                // Intentionally empty.
            }
            // A `screen` is not pure metadata: its `view` is code. It
            // compiles to an ordinary zero-arg module-level closure
            // named `__screen__<id>`, which the synthesized dispatcher
            // (see `compile_module_with_imports`) calls by id. The only
            // thing that makes it different from a hand-written `let`
            // is `current_screen`, which is what puts the screen's own
            // `state` fields in scope for the body and nothing else.
            Statement::ScreenDeclaration(decl) => {
                let index = self
                    .schema
                    .as_ref()
                    .and_then(|s| s.screens.keys().position(|k| k == &decl.id))
                    .ok_or_else(|| {
                        VMError::InvalidOperation(format!(
                            "screen `{}` is not in the module schema",
                            decl.id
                        ))
                    })?;
                let func = Function {
                    arguments: Vec::new(),
                    return_type: crate::parser::Identifier::synthetic("infer"),
                    body: Block {
                        // A `Return`, not an expression statement: a
                        // function body's trailing value reaches its caller
                        // only through one, because `finish_child` appends
                        // `Void; Return` and would otherwise bury it. This
                        // is what the parser does for every hand-written
                        // `fn` whose last expression has no semicolon.
                        statement_list: vec![Statement::new_return(
                            Some(decl.view.clone()),
                            decl.span,
                        )],
                        span: decl.span,
                    },
                    span: decl.span,
                };
                let name = screen_fn_name(index);
                let outer = self.current_screen.replace(decl.id.clone());
                let result = self.compile_function(&func, &name);
                self.current_screen = outer;
                result?;
                self.add_local(name, false);
            }
        }
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Import
    // -----------------------------------------------------------------------

    fn compile_import_stmt(
        &mut self,
        import: &crate::parser::ImportStatement,
    ) -> Result<(), VMError> {
        let meta = ImportMeta {
            names: import.get_names().clone(),
            path: import.get_path().to_string(),
        };
        let meta_value = Value::String(serde_import_meta(&meta));
        let idx = self.chunk().add_constant(meta_value);
        self.emit(OpCode::Import(idx));
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Conditional
    // -----------------------------------------------------------------------

    fn compile_conditional(
        &mut self,
        cond: &crate::parser::ConditionalStatement,
    ) -> Result<(), VMError> {
        let branches = cond.get_branches();
        let else_block = cond.get_else_block();

        // Collect jump-to-end patches.
        let mut end_jumps: Vec<usize> = Vec::new();

        for (condition, block) in branches.iter() {
            self.compile_expression(condition)?;
            let false_jump = self.emit_jump(OpCode::JumpIfFalse(0));

            // Then branch – new scope.
            self.begin_scope();
            self.compile_block(block)?;
            self.end_scope();

            // Jump over the remaining branches / else.
            let end_jump = self.emit_jump(OpCode::Jump(0));
            end_jumps.push(end_jump);

            // Patch the false-jump to land here (start of next branch).
            self.patch_jump(false_jump);
        }

        // Else branch.
        if let Some(else_block) = else_block {
            self.begin_scope();
            self.compile_block(else_block)?;
            self.end_scope();
        }

        // Patch all end-jumps.
        for j in end_jumps {
            self.patch_jump(j);
        }
        Ok(())
    }

    // -----------------------------------------------------------------------
    // For-loop shared core
    // -----------------------------------------------------------------------

    /// Shared scaffolding for both for-loop statements and for-loop
    /// expressions. Sets up the counter/end locals, loop header, increment,
    /// and exit jump. The caller supplies `body_fn` to emit the body-specific
    /// bytecode; it receives the counter slot index.
    fn compile_for_loop_core(
        &mut self,
        for_loop_variable: &str,
        range_start: &Expression,
        range_end: &Expression,
        body_fn: impl FnOnce(&mut Self, u8) -> Result<(), VMError>,
    ) -> Result<(), VMError> {
        self.begin_scope();

        self.compile_expression(range_start)?;
        // The counter local is only ever referenced by slot (for the bounds
        // check and the increment), never by name from the loop body — body
        // name lookups resolve to the per-iteration copy added below, which
        // shadows this one. Give it an internal name to make that explicit.
        let counter_slot = self.add_local(format!("$counter_{}", for_loop_variable), false);

        self.compile_expression(range_end)?;
        let end_slot = self.add_local(format!("$end_{}", for_loop_variable), false);

        let loop_start = self.function.chunk.code.len();
        self.emit(OpCode::GetLocal(counter_slot));
        self.emit(OpCode::GetLocal(end_slot));
        self.emit(OpCode::Lt);
        let exit_jump = self.emit_jump(OpCode::JumpIfFalse(0));

        // Per-iteration binding: copy the counter into a fresh local named
        // after the loop variable so closures created in the body capture a
        // distinct slot that gets closed (snapshotted) at the end of each
        // iteration, rather than all aliasing the single shared counter slot.
        self.begin_scope();
        self.emit(OpCode::GetLocal(counter_slot));
        self.add_local(for_loop_variable.to_string(), false);

        body_fn(self, counter_slot)?;

        self.end_scope();

        self.emit(OpCode::GetLocal(counter_slot));
        self.emit_constant(Value::Integer(1));
        self.emit(OpCode::Add);
        self.emit(OpCode::SetLocal(counter_slot));

        self.emit_loop(loop_start);
        self.patch_jump(exit_jump);

        self.end_scope();
        Ok(())
    }

    // -----------------------------------------------------------------------
    // For-loop statement
    // -----------------------------------------------------------------------

    fn compile_for_loop_statement(
        &mut self,
        for_loop: &crate::parser::ForLoopStatement,
    ) -> Result<(), VMError> {
        let var_name = for_loop.get_variable().get();
        let range_start = for_loop.get_range_start();
        let range_end = for_loop.get_range_end();
        let body = for_loop.get_body();

        self.compile_for_loop_core(&var_name, &range_start, &range_end, |compiler, _| {
            compiler.begin_scope();
            compiler.compile_block(&body)?;
            compiler.end_scope();
            Ok(())
        })
    }

    // -----------------------------------------------------------------------
    // For-loop expression (collects results into array)
    // -----------------------------------------------------------------------

    fn compile_for_loop_expression(&mut self, for_loop: &ForLoopExpression) -> Result<(), VMError> {
        self.emit(OpCode::BeginForExpr);

        let var_name = for_loop.variable.get();

        self.compile_for_loop_core(
            &var_name,
            &for_loop.range_start,
            &for_loop.range_end,
            |compiler, _| {
                compiler.begin_scope();
                compiler.compile_expression_block(&for_loop.body)?;
                compiler.end_scope_preserving_result();
                compiler.emit(OpCode::AppendForExpr);
                Ok(())
            },
        )
    }

    // -----------------------------------------------------------------------
    // Expression compilation
    // -----------------------------------------------------------------------

    fn compile_expression(&mut self, expr: &Expression) -> Result<(), VMError> {
        self.current_line = expr.span().start_line;
        match expr {
            Expression::Literal(lit) => self.compile_literal(lit),
            Expression::Unary(unary) => {
                self.compile_expression(&unary.value)?;
                match unary.operator {
                    Operator::Minus => self.emit(OpCode::Negate),
                    Operator::Not => self.emit(OpCode::Not),
                    _ => unreachable!(),
                };
                Ok(())
            }
            Expression::Binary(binary) => {
                self.compile_expression(&binary.left)?;
                self.compile_expression(&binary.right)?;
                match &binary.operator {
                    Operator::Plus => {
                        self.emit(OpCode::Add);
                    }
                    Operator::Minus => {
                        self.emit(OpCode::Sub);
                    }
                    Operator::Multiply => {
                        self.emit(OpCode::Mul);
                    }
                    Operator::Divide => {
                        self.emit(OpCode::Div);
                    }
                    Operator::Modulo => {
                        self.emit(OpCode::Mod);
                    }
                    Operator::Power => {
                        self.emit(OpCode::Pow);
                    }
                    Operator::Equals => {
                        self.emit(OpCode::Eq);
                    }
                    Operator::NotEquals => {
                        self.emit(OpCode::Ne);
                    }
                    Operator::GreaterThan => {
                        self.emit(OpCode::Gt);
                    }
                    Operator::GreaterThanOrEqualTo => {
                        self.emit(OpCode::Ge);
                    }
                    Operator::LessThan => {
                        self.emit(OpCode::Lt);
                    }
                    Operator::LessThanOrEqualTo => {
                        self.emit(OpCode::Le);
                    }
                    Operator::Not => {
                        self.emit(OpCode::Not);
                    }
                    Operator::And => {
                        self.emit(OpCode::And);
                    }
                    Operator::Or => {
                        self.emit(OpCode::Or);
                    }
                };
                Ok(())
            }
            Expression::Grouping(grouping) => self.compile_expression(&grouping.value),
            Expression::MemberAccess(access) => {
                self.compile_expression(&access.object)?;
                let name = access.property.get();
                let idx = self.chunk().add_constant(Value::String(name));
                self.emit(OpCode::GetProperty(idx));
                Ok(())
            }
            Expression::Call(call) => self.compile_call(call),
            Expression::IndexAccess(access) => {
                self.compile_expression(&access.object)?;
                self.compile_expression(&access.index)?;
                self.emit(OpCode::GetIndex);
                Ok(())
            }
            Expression::Widget(widget) => {
                let ident_name = widget.identifier.get();

                // Special-case: `Context { name, value, children }`. The
                // widget is a compile-time scope: we push (name, value) onto
                // the runtime context stack, compile the `children`
                // expression (which may contain `use_context(...)` calls
                // that observe the pushed scope), then pop. The final
                // emitted widget is a transparent Flex containing those
                // children, so Context composes into a parent's `children`
                // array like any other widget.
                if ident_name == "Context" {
                    let mut name_expr: Option<&crate::parser::Expression> = None;
                    let mut value_expr: Option<&crate::parser::Expression> = None;
                    let mut children_expr: Option<&crate::parser::Expression> = None;
                    for (key_ident, val_expr) in &widget.properties {
                        match key_ident.get().as_str() {
                            "name" => name_expr = Some(val_expr),
                            "value" => value_expr = Some(val_expr),
                            "children" => children_expr = Some(val_expr),
                            other => {
                                return Err(VMError::InvalidOperation(format!(
                                    "Context widget does not support property '{}'; only name, value, children are allowed",
                                    other
                                )));
                            }
                        }
                    }
                    let name_expr = name_expr.ok_or_else(|| {
                        VMError::InvalidOperation(
                            "Context widget requires a `name` property".to_string(),
                        )
                    })?;
                    let value_expr = value_expr.ok_or_else(|| {
                        VMError::InvalidOperation(
                            "Context widget requires a `value` property".to_string(),
                        )
                    })?;
                    let children_expr = children_expr.ok_or_else(|| {
                        VMError::InvalidOperation(
                            "Context widget requires a `children` property".to_string(),
                        )
                    })?;

                    // Push (name, value), activate the scope.
                    self.compile_expression(name_expr)?;
                    self.compile_expression(value_expr)?;
                    self.emit(OpCode::PushContext);

                    // Emit the wrapper Flex: push "children" key, compile
                    // the children expression, then pop the context.
                    let flex_const = self.chunk().add_constant(Value::String("Flex".to_string()));
                    let children_key = self
                        .chunk()
                        .add_constant(Value::String("children".to_string()));
                    self.emit(OpCode::Constant(children_key));
                    self.compile_expression(children_expr)?;
                    self.emit(OpCode::PopContext);

                    self.emit(OpCode::Widget {
                        identifier_constant: flex_const,
                        property_count: 1,
                    });
                    return Ok(());
                }

                let ident_const = self.chunk().add_constant(Value::String(ident_name));
                let prop_count = widget.properties.len() as u16;

                // Push each property: key constant, then value.
                for (key_ident, value_expr) in &widget.properties {
                    let key_const = self.chunk().add_constant(Value::String(key_ident.get()));
                    self.emit(OpCode::Constant(key_const));
                    self.compile_expression(value_expr)?;
                }

                self.emit(OpCode::Widget {
                    identifier_constant: ident_const,
                    property_count: prop_count,
                });
                Ok(())
            }
            Expression::Range(_range) => {
                // Ranges are not first-class values; they are only used
                // inside for-loops and are handled there.
                self.emit(OpCode::Void);
                Ok(())
            }
            Expression::ForLoop(for_loop) => self.compile_for_loop_expression(for_loop),
            Expression::SpreadForLoop(for_loop) => {
                // Same as ForLoop expression – the spread is handled by the
                // array literal compilation (see Literal::Array).
                self.compile_for_loop_expression(for_loop)
            }
            Expression::Spread(spread) => {
                self.compile_expression(&spread.inner)?;
                Ok(())
            }
            Expression::Match(m) => self.compile_match(m),
            Expression::PrefixIncrement(inc) => self.compile_increment(&inc.identifier, true),
            Expression::PostfixIncrement(inc) => self.compile_increment(&inc.identifier, false),
        }
    }

    /// Compile `++x` (prefix=true) or `x++` (prefix=false).
    /// Leaves the appropriate value on the stack: new for prefix, old for postfix.
    fn compile_increment(
        &mut self,
        ident: &crate::parser::Identifier,
        prefix: bool,
    ) -> Result<(), VMError> {
        let name = ident.get();

        if let Some(slot) = self.resolve_local(&name) {
            if !prefix {
                self.emit(OpCode::GetLocal(slot));
            }
            self.emit(OpCode::GetLocal(slot));
            self.emit_constant(Value::Integer(1));
            self.emit(OpCode::Add);
            if self.is_local_state(&name) {
                let name_const = self.chunk().add_constant(Value::String(name.clone()));
                self.emit(OpCode::SetState(name_const));
            }
            self.emit(OpCode::SetLocal(slot));
            if prefix {
                self.emit(OpCode::GetLocal(slot));
            }
            Ok(())
        } else if let Some(uv) = self.resolve_upvalue(&name) {
            if !prefix {
                self.emit(OpCode::GetUpvalue(uv));
            }
            self.emit(OpCode::GetUpvalue(uv));
            self.emit_constant(Value::Integer(1));
            self.emit(OpCode::Add);
            if self.is_local_state(&name) {
                let name_const = self.chunk().add_constant(Value::String(name.clone()));
                self.emit(OpCode::SetState(name_const));
            }
            self.emit(OpCode::SetUpvalue(uv));
            if prefix {
                self.emit(OpCode::GetUpvalue(uv));
            }
            Ok(())
        } else {
            Err(VMError::UndefinedVariable(name))
        }
    }

    // -----------------------------------------------------------------------
    // Literal compilation
    // -----------------------------------------------------------------------

    fn compile_literal(&mut self, lit: &Literal) -> Result<(), VMError> {
        match lit {
            Literal::Integer(i, _) => {
                self.emit_constant(Value::Integer(*i));
                Ok(())
            }
            Literal::Float(f, _) => {
                self.emit_constant(Value::Float(*f));
                Ok(())
            }
            Literal::Boolean(b, _) => {
                if *b {
                    self.emit(OpCode::True);
                } else {
                    self.emit(OpCode::False);
                }
                Ok(())
            }
            Literal::String(s, _) => {
                self.emit_constant(Value::String(s.clone()));
                Ok(())
            }
            Literal::Identifier(ident) => {
                let name = ident.get();
                // 1. Local?
                if let Some(slot) = self.resolve_local(&name) {
                    self.emit(OpCode::GetLocal(slot));
                    return Ok(());
                }
                // 2. Upvalue?
                if let Some(uv) = self.resolve_upvalue(&name) {
                    self.emit(OpCode::GetUpvalue(uv));
                    return Ok(());
                }
                // 3. Strict mode (host_state {} declared): the
                //    identifier MUST be a declared host_state field,
                //    a declared/imported record name, or a built-in.
                //    Otherwise it's a typo / missing declaration —
                //    error with a useful diagnostic.
                //
                //    Note: identifier resolution requires
                //    `host_state {}` specifically (not just any
                //    schema declaration), because we need a known
                //    list of valid host_state fields to check
                //    against. A module with only `events {}`
                //    declared keeps loose identifier resolution.
                if self.has_host_state_schema() && !self.is_known_in_schema(&name) {
                    let err = self.strict_unknown_identifier(
                        &name,
                        ident.span.start_line,
                        ident.span.start_column,
                    );
                    return Err(VMError::StrictMode(err));
                }
                // 4. A `state` field of the screen we are inside reads
                //    its namespaced key. This is the whole of scoped
                //    host state: two screens may both declare `rows`,
                //    and neither can name the other's.
                let name = match self.screen_field(&name) {
                    Some(scoped) => scoped,
                    None => name,
                };
                // 5. Loose mode (or strict-mode known identifier):
                //    emit GetState which falls through to host-state
                //    in the VM. We cannot distinguish state from
                //    host-state at compile time because state depends
                //    on the call-stack path at runtime.
                let idx = self.chunk().add_constant(Value::String(name));
                self.emit(OpCode::GetState(idx));
                Ok(())
            }
            Literal::Function(func) => {
                self.compile_function(func, "fn")?;
                Ok(())
            }
            Literal::Map(map) => {
                let n = map.properties.len() as u16;
                for (key_ident, value_expr) in &map.properties {
                    let key_const = self.chunk().add_constant(Value::String(key_ident.get()));
                    self.emit(OpCode::Constant(key_const));
                    self.compile_expression(value_expr)?;
                }
                self.emit(OpCode::Map(n));
                Ok(())
            }
            Literal::Array(array) => {
                // Handle spread and spread-for-loop within array literals.
                // We use BeginForExpr/AppendForExpr approach: push an empty
                // array, then append each element.
                self.emit(OpCode::BeginForExpr);
                for elem in &array.elements {
                    match elem {
                        Expression::SpreadForLoop(for_loop) => {
                            self.compile_for_loop_expression(for_loop)?;
                            self.emit(OpCode::SpreadForExpr);
                        }
                        Expression::Spread(spread) => {
                            self.compile_expression(&spread.inner)?;
                            self.emit(OpCode::SpreadForExpr);
                        }
                        _ => {
                            self.compile_expression(elem)?;
                            self.emit(OpCode::AppendForExpr);
                        }
                    }
                }
                // The collector array is on the stack.
                Ok(())
            }
        }
    }

    // -----------------------------------------------------------------------
    // Function / closure compilation
    // -----------------------------------------------------------------------

    fn compile_function(&mut self, func: &Function, name: &str) -> Result<(), VMError> {
        let arity = func.arguments.len() as u8;

        // Take `self` out and create a child compiler.
        let parent = std::mem::replace(self, Compiler::new("<dummy>".to_string(), 0));
        let mut child = parent.child(name.to_string(), arity);

        // Reserve slot 0 for the callee (the function/closure being called).
        // At runtime, slot_offset points to the callee on the stack, so
        // parameters must start at slot 1. We use add_param_local because
        // these values are pre-existing on the stack (placed by the caller),
        // not pushed by any emit() in this child compiler.
        //
        // Binding this slot to the function's own `name` is what enables
        // self-recursion: a reference to `name` inside the body resolves to
        // slot 0 (the executing closure). For named bindings (`let fact = fn
        // ...`) the caller threads the binding name through; for anonymous
        // `fn` literals `name` is the keyword `"fn"`, which can never appear
        // as a user identifier, so the slot stays effectively unreferenceable.
        child.add_param_local(name.to_string());

        // Declare parameters as locals in the child scope.
        for param in &func.arguments {
            child.add_param_local(param.get());
        }

        // Compile the function body.
        child.compile_block(&func.body)?;

        // Implicit return Void.
        // (finish_child adds Void + Return)
        let (mut restored_parent, proto) = child.finish_child();

        // Store the proto as a constant in the parent's chunk.
        // We use a special sentinel value to hold the proto; the VM
        // recognises Value::Void at the constant index of a Closure opcode
        // and looks up the proto from a side table. Instead, we'll store
        // the proto inside the compiler and reference it by index.
        // For simplicity, we store the FunctionProto in the constant pool
        // wrapped in a dummy Value. The VM will need to handle this.
        // Actually, the cleanest approach: store the proto in a Vec on the
        // Compiler/FunctionProto and reference by index. Let's add a
        // `sub_functions` field to FunctionProto.

        // We'll embed the proto index in the Closure opcode. The protos are
        // stored in a Vec on the parent FunctionProto.
        // Store the proto in the parent's protos table. The Closure opcode
        // index points into `function.protos`.
        restored_parent.function.protos.push(proto);
        let closure_idx = (restored_parent.function.protos.len() - 1) as u16;

        restored_parent.emit(OpCode::Closure(closure_idx));

        // Restore self.
        *self = *restored_parent;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Call compilation
    // -----------------------------------------------------------------------

    fn compile_call(&mut self, call: &Call) -> Result<(), VMError> {
        // Special-case: array.length()
        if let Expression::MemberAccess(access) = &*call.callee {
            if access.property.get() == "length" {
                self.compile_expression(&access.object)?;
                self.emit(OpCode::ArrayLength);
                return Ok(());
            }
            if access.property.get() == "join" {
                if call.arguments.len() != 1 {
                    return Err(VMError::InvalidOperation(
                        "join() takes exactly one separator argument".to_string(),
                    ));
                }
                self.compile_expression(&access.object)?;
                self.compile_expression(&call.arguments[0])?;
                self.emit(OpCode::ArrayJoin);
                return Ok(());
            }
        }

        // Special-case: event("name", ...)
        if let Expression::Literal(Literal::Identifier(ident)) = &*call.callee {
            if ident.get() == "event" {
                // Strict-mode validation: event name must be a
                // string literal that matches a declared event,
                // and the arg count must match the declared
                // signature. Loose mode passes through unchanged.
                if self.is_strict() {
                    self.check_event_call(call, ident)?;
                }
                // Push all args onto the stack (name first, then extra args).
                for arg in &call.arguments {
                    self.compile_expression(arg)?;
                }
                self.emit(OpCode::EmitEvent(call.arguments.len() as u8));
                return Ok(());
            }
        }

        // Special-case: use_context("name") — look up the nearest enclosing
        // Context provider with the matching name.
        if let Expression::Literal(Literal::Identifier(ident)) = &*call.callee {
            if ident.get() == "use_context" {
                if call.arguments.len() != 1 {
                    return Err(VMError::InvalidOperation(
                        "use_context() takes exactly one string argument".to_string(),
                    ));
                }
                // Statically resolve the name to a constant-pool index.
                let name = match &call.arguments[0] {
                    Expression::Literal(Literal::String(s, _)) => s.clone(),
                    _ => {
                        return Err(VMError::InvalidOperation(
                            "use_context() argument must be a string literal".to_string(),
                        ))
                    }
                };
                let idx = self.chunk().add_constant(Value::String(name));
                self.emit(OpCode::GetContext(idx));
                return Ok(());
            }
        }

        // General case: compile callee then args.
        self.compile_expression(&call.callee)?;
        for arg in &call.arguments {
            self.compile_expression(arg)?;
        }
        self.emit(OpCode::Call(call.arguments.len() as u8));
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Match expression
    // -----------------------------------------------------------------------

    fn compile_match(&mut self, m: &MatchExpression) -> Result<(), VMError> {
        self.emit(OpCode::MarkBranched);
        // Compile the scrutinee – its value stays on the stack and is
        // duplicated (via Dup) before each arm comparison.
        self.compile_expression(&m.scrutinee)?;

        let mut end_jumps: Vec<usize> = Vec::new();

        for (pattern, block) in &m.arms {
            let is_wildcard = matches!(
                pattern,
                Expression::Literal(Literal::Identifier(ident)) if ident.get() == "_"
            );

            if is_wildcard {
                // Pop the scrutinee (no longer needed) and compile the body.
                self.emit(OpCode::Pop);
                self.begin_scope();
                self.compile_expression_block(block)?;
                self.end_scope_preserving_result();
                let end_jump = self.emit_jump(OpCode::Jump(0));
                end_jumps.push(end_jump);
            } else {
                // Duplicate the scrutinee so it survives the comparison.
                self.emit(OpCode::Dup);
                self.compile_expression(pattern)?;
                self.emit(OpCode::Eq);
                let skip_jump = self.emit_jump(OpCode::JumpIfFalse(0));

                // Match found – pop the original scrutinee, then run the body.
                self.emit(OpCode::Pop);
                self.begin_scope();
                self.compile_expression_block(block)?;
                self.end_scope_preserving_result();
                let end_jump = self.emit_jump(OpCode::Jump(0));
                end_jumps.push(end_jump);

                self.patch_jump(skip_jump);
            }
        }

        // No arm matched – pop the scrutinee and push Void.
        self.emit(OpCode::Pop);
        self.emit(OpCode::Void);

        for j in end_jumps {
            self.patch_jump(j);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Encode ImportMeta as a string for storage in the constant pool. The VM
/// will decode it back.
pub fn serde_import_meta(meta: &ImportMeta) -> String {
    // Simple encoding: "IMPORT:<path>:<names_csv_or_*>"
    let names_str = match &meta.names {
        Some(names) => names.join(","),
        None => "*".to_string(),
    };
    format!("IMPORT:{}:{}", meta.path, names_str)
}

/// Decode an ImportMeta from its string representation in the constant pool.
pub fn deserialize_import_meta(s: &str) -> Option<ImportMeta> {
    let parts: Vec<&str> = s.splitn(3, ':').collect();
    if parts.len() != 3 || parts[0] != "IMPORT" {
        return None;
    }
    let path = parts[1].to_string();
    let names = if parts[2] == "*" {
        None
    } else {
        Some(parts[2].split(',').map(|s| s.to_string()).collect())
    };
    Some(ImportMeta { names, path })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::Parser;
    use crate::scanner::Scanner;

    fn compile(source: &str) -> FunctionProto {
        let tokens = Scanner::new(source.to_string()).scan();
        let module = Parser::new(tokens).parse().expect("parse");
        Compiler::compile_module(&module).expect("compile")
    }

    /// Recursively test whether any proto in the tree emits the
    /// given predicate over its opcodes.
    fn any_op(proto: &FunctionProto, pred: &impl Fn(&OpCode) -> bool) -> bool {
        proto.chunk.code.iter().any(pred) || proto.protos.iter().any(|p| any_op(p, pred))
    }

    /// Regression: `++`/`--` on a *captured non-state* upvalue must
    /// NOT emit a spurious `SetState` (which would write a bogus
    /// component-state entry). The local branch already guards
    /// `SetState` behind `is_local_state`; the upvalue branch must
    /// do the same.
    #[test]
    fn increment_on_captured_non_state_upvalue_emits_no_setstate() {
        // `n` is a plain `let` captured by the inner closure, which
        // increments it. It is not `state`, so no SetState should
        // be emitted anywhere.
        let proto = compile(
            r#"
let main = fn () {
    let n = 0;
    let bump = fn () { ++n };
    bump()
};
"#,
        );
        assert!(
            !any_op(&proto, &|op| matches!(op, OpCode::SetState(_))),
            "increment on a captured non-state upvalue must not emit SetState"
        );
    }

    /// Companion: `++` on an actual `state` local still emits
    /// `SetState` (the guard must not over-suppress).
    #[test]
    fn increment_on_state_local_emits_setstate() {
        let proto = compile(
            r#"
let main = fn () {
    state count = 0;
    ++count;
    count
};
"#,
        );
        assert!(
            any_op(&proto, &|op| matches!(op, OpCode::SetState(_))),
            "increment on a state local must still emit SetState"
        );
    }
}
