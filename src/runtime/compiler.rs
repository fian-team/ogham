//! Bytecode compiler: walks the AST and emits [`OpCode`] instructions into
//! a [`Chunk`] that the VM can execute.

use crate::parser::{
    Block, Call, Expression, ForLoopExpression, Function, Literal, MatchExpression, Operator,
    Statement,
};
use crate::runtime::error::VMError;
use crate::runtime::opcode::{Chunk, FunctionProto, ImportMeta, OpCode, UpvalueDescriptor};
use crate::runtime::value::Value;

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
        }
    }

    /// Create a child compiler for a nested function and move `self` into
    /// its `enclosing` slot. Returns the child.
    fn child(self, name: String, arity: u8) -> Self {
        Self {
            function: FunctionProto::new(name, arity),
            locals: Vec::new(),
            upvalue_descs: Vec::new(),
            scope_depth: 0,
            enclosing: Some(Box::new(self)),
            current_line: 0,
            stack_depth: 0,
        }
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
            | OpCode::Eq
            | OpCode::Ne
            | OpCode::Gt
            | OpCode::Ge
            | OpCode::Lt
            | OpCode::Le
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
    /// the whole file).
    pub fn compile_module(module: &Function) -> Result<FunctionProto, VMError> {
        let mut compiler = Compiler::new("<module>".to_string(), 0);
        compiler.compile_block(&module.body)?;

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
                self.compile_expression(&decl.get_value())?;
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
            Statement::Event(_) => {
                // Events are not executed by the runtime.
            }
            Statement::Import(import_stmt) => {
                self.compile_import(import_stmt)?;
            }
            Statement::ForLoop(for_loop) => {
                self.emit(OpCode::MarkBranched);
                self.compile_for_loop_statement(for_loop)?;
            }
        }
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Import
    // -----------------------------------------------------------------------

    fn compile_import(&mut self, import: &crate::parser::ImportStatement) -> Result<(), VMError> {
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
    // For-loop statement
    // -----------------------------------------------------------------------

    fn compile_for_loop_statement(
        &mut self,
        for_loop: &crate::parser::ForLoopStatement,
    ) -> Result<(), VMError> {
        let var_name = for_loop.get_variable().get();

        self.begin_scope();

        // Compile range start, then immediately add it as a local so its
        // slot reflects the actual stack depth.
        self.compile_expression(&for_loop.get_range_start())?;
        let counter_slot = self.add_local(var_name.clone(), false);

        // Same for range end.
        self.compile_expression(&for_loop.get_range_end())?;
        let _end_slot = self.add_local(format!("$end_{}", var_name), false);

        // Loop header: compare counter < end.
        let loop_start = self.function.chunk.code.len();
        self.emit(OpCode::GetLocal(counter_slot));
        self.emit(OpCode::GetLocal(_end_slot));
        self.emit(OpCode::Lt);
        let exit_jump = self.emit_jump(OpCode::JumpIfFalse(0));

        // Loop body.
        self.begin_scope();
        self.compile_block(&for_loop.get_body())?;
        self.end_scope();

        // Increment counter: counter = counter + 1.
        self.emit(OpCode::GetLocal(counter_slot));
        self.emit_constant(Value::Integer(1));
        self.emit(OpCode::Add);
        self.emit(OpCode::SetLocal(counter_slot));

        // Jump back to loop header.
        self.emit_loop(loop_start);

        // Patch exit jump.
        self.patch_jump(exit_jump);

        self.end_scope();
        Ok(())
    }

    // -----------------------------------------------------------------------
    // For-loop expression (collects results into array)
    // -----------------------------------------------------------------------

    fn compile_for_loop_expression(&mut self, for_loop: &ForLoopExpression) -> Result<(), VMError> {
        // Push the results-collector array.
        self.emit(OpCode::BeginForExpr);

        let var_name = for_loop.variable.get();

        self.begin_scope();

        // Compile range start, then immediately add it as a local so its
        // slot reflects the actual stack depth.
        self.compile_expression(&for_loop.range_start)?;
        let counter_slot = self.add_local(var_name.clone(), false);

        // Same for range end.
        self.compile_expression(&for_loop.range_end)?;
        let end_slot = self.add_local(format!("$end_{}", var_name), false);

        // Loop header.
        let loop_start = self.function.chunk.code.len();
        self.emit(OpCode::GetLocal(counter_slot));
        self.emit(OpCode::GetLocal(end_slot));
        self.emit(OpCode::Lt);
        let exit_jump = self.emit_jump(OpCode::JumpIfFalse(0));

        // Loop body – compile the block; the result value is on the stack.
        // Use compile_expression_block so that the parser's implicit Return
        // (for the last expression) leaves the value on the stack instead of
        // exiting the function.
        self.begin_scope();
        self.compile_expression_block(&for_loop.body)?;
        self.end_scope();

        // Append result to collector.
        self.emit(OpCode::AppendForExpr);

        // Increment counter.
        self.emit(OpCode::GetLocal(counter_slot));
        self.emit_constant(Value::Integer(1));
        self.emit(OpCode::Add);
        self.emit(OpCode::SetLocal(counter_slot));

        self.emit_loop(loop_start);
        self.patch_jump(exit_jump);

        self.end_scope();
        // The results array is now on top of the stack.
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Expression compilation
    // -----------------------------------------------------------------------

    fn compile_expression(&mut self, expr: &Expression) -> Result<(), VMError> {
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
                let ident_const = self.chunk().add_constant(Value::String(ident_name));
                let prop_count = widget.properties.len() as u16;

                // Push each property: key constant, then value.
                for (key, value_expr) in &widget.properties {
                    let key_const = self.chunk().add_constant(Value::String(key.clone()));
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
            Expression::Spread(inner) => {
                // Standalone spread outside an array literal is an error
                // at runtime in the tree-walk interpreter. We'll compile
                // the inner expression; the array literal handler deals
                // with merging.
                self.compile_expression(inner)?;
                Ok(())
            }
            Expression::Match(m) => self.compile_match(m),
        }
    }

    // -----------------------------------------------------------------------
    // Literal compilation
    // -----------------------------------------------------------------------

    fn compile_literal(&mut self, lit: &Literal) -> Result<(), VMError> {
        match lit {
            Literal::Integer(i) => {
                self.emit_constant(Value::Integer(*i));
                Ok(())
            }
            Literal::Float(f) => {
                self.emit_constant(Value::Float(*f));
                Ok(())
            }
            Literal::Boolean(b) => {
                if *b {
                    self.emit(OpCode::True);
                } else {
                    self.emit(OpCode::False);
                }
                Ok(())
            }
            Literal::String(s) => {
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
                // 3. State? (resolved at runtime via the constant name)
                // 4. Host state?
                // We cannot distinguish state vs host-state at compile time
                // because state depends on the call-stack path at runtime.
                // Emit GetState which falls through to host-state in the VM.
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
                for (key, value_expr) in &map.properties {
                    let key_const = self.chunk().add_constant(Value::String(key.clone()));
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
                        Expression::Spread(inner) => {
                            self.compile_expression(inner)?;
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
        child.add_param_local(String::new());

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
        }

        // Special-case: event("name", ...)
        if let Expression::Literal(Literal::Identifier(ident)) = &*call.callee {
            if ident.get() == "event" {
                // Push all args onto the stack (name first, then extra args).
                for arg in &call.arguments {
                    self.compile_expression(arg)?;
                }
                self.emit(OpCode::EmitEvent(call.arguments.len() as u8));
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
                self.end_scope();
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
                self.end_scope();
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
