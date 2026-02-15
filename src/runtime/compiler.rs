// ---------------------------------------------------------------------------
// Bytecode Compiler – walks the AST and emits bytecode into a Chunk.
// ---------------------------------------------------------------------------

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
        let parent = self.enclosing.expect("finish_child called on top-level compiler");
        (parent, self.function)
    }

    // -- Helpers: emit bytecode ---------------------------------------------

    fn chunk(&mut self) -> &mut Chunk {
        &mut self.function.chunk
    }

    fn emit(&mut self, op: OpCode) -> usize {
        let line = self.current_line;
        self.chunk().emit(op, line)
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
                // The VM will close the upvalue.
                self.emit(OpCode::Pop); // placeholder – VM handles closing
            } else {
                self.emit(OpCode::Pop);
            }
            self.locals.pop();
        }
    }

    fn add_local(&mut self, name: String, is_state: bool) -> u8 {
        let slot = self.locals.len() as u8;
        self.locals.push(Local {
            name,
            depth: self.scope_depth,
            is_captured: false,
            is_state,
        });
        slot
    }

    /// Resolve an identifier as a local in the *current* function.
    /// Returns the slot index if found.
    fn resolve_local(&self, name: &str) -> Option<u8> {
        for (i, local) in self.locals.iter().enumerate().rev() {
            if local.name == name {
                return Some(i as u8);
            }
        }
        None
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
            enclosing.locals[local_slot as usize].is_captured = true;
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

    fn compile_statement(
        &mut self,
        statement: &Statement,
        is_last: bool,
    ) -> Result<(), VMError> {
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
                    if self.locals[slot as usize].is_state {
                        let name_const =
                            self.chunk().add_constant(Value::String(name.clone()));
                        self.emit(OpCode::SetState(name_const));
                    }
                    self.emit(OpCode::SetLocal(slot));
                } else if let Some(uv) = self.resolve_upvalue(&name) {
                    // Check if the upvalue is a state variable.
                    // We need a name constant either way for state.
                    let name_const =
                        self.chunk().add_constant(Value::String(name.clone()));
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

    fn compile_import(
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
    // For-loop statement
    // -----------------------------------------------------------------------

    fn compile_for_loop_statement(
        &mut self,
        for_loop: &crate::parser::ForLoopStatement,
    ) -> Result<(), VMError> {
        // Compile range bounds.
        self.compile_expression(&for_loop.get_range_start())?;
        self.compile_expression(&for_loop.get_range_end())?;

        let var_name = for_loop.get_variable().get();

        // Stack layout:  ... | range_end | range_start (we reverse below)
        // We want: ... | end | counter
        // The counter starts at range_start. We keep end on the stack too so
        // we can compare each iteration.
        // Actually let's keep it simple: store start and end as locals.

        self.begin_scope();
        // The start value is already on the stack at position locals.len().
        // But we pushed start first then end, so stack is: [start, end]
        // We need: counter local, end local.
        // Let's re-order: We'll use the start as the loop variable, and end as
        // a hidden local.

        // Stack currently: [start_val, end_val]
        // Add end as a hidden local.
        let _end_slot = self.add_local(format!("$end_{}", var_name), false);
        // Now start_val is at position (locals.len() - 2) ... wait, we added
        // end first. Let's think again.
        // Actually: we compiled range_start first → it's deeper on the stack.
        // Then range_end → it's on top.
        // So: stack[slot_of_start] = start_val, stack[slot_of_end] = end_val.
        // We want end_val to be the first local we add (it's on top).
        // Wait, no: locals are indexed from the base of the scope.
        // Actually, locals map 1:1 to stack positions relative to the frame.
        // We compiled start, then end, so:
        //   local slot N = start_val
        //   local slot N+1 = end_val
        // But we haven't added locals yet. Let's fix: we should add the
        // *start* local first (it was pushed first) then the *end* local.

        // Undo the end local we just added:
        self.locals.pop();

        // The start value was pushed first (lower on stack).
        let counter_slot = self.add_local(var_name.clone(), false);
        // The end value was pushed second (higher on stack).
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

    fn compile_for_loop_expression(
        &mut self,
        for_loop: &ForLoopExpression,
    ) -> Result<(), VMError> {
        // Push the results-collector array.
        self.emit(OpCode::BeginForExpr);

        // Compile range bounds.
        self.compile_expression(&for_loop.range_start)?;
        self.compile_expression(&for_loop.range_end)?;

        let var_name = for_loop.variable.get();

        self.begin_scope();

        // locals: counter (start), end
        let counter_slot = self.add_local(var_name.clone(), false);
        let end_slot = self.add_local(format!("$end_{}", var_name), false);

        // Loop header.
        let loop_start = self.function.chunk.code.len();
        self.emit(OpCode::GetLocal(counter_slot));
        self.emit(OpCode::GetLocal(end_slot));
        self.emit(OpCode::Lt);
        let exit_jump = self.emit_jump(OpCode::JumpIfFalse(0));

        // Loop body – compile the block; the result value is on the stack.
        self.begin_scope();
        self.compile_block(&for_loop.body)?;
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
                self.emit(OpCode::Negate);
                Ok(())
            }
            Expression::Binary(binary) => {
                self.compile_expression(&binary.left)?;
                self.compile_expression(&binary.right)?;
                match &binary.operator {
                    Operator::Plus => { self.emit(OpCode::Add); }
                    Operator::Minus => { self.emit(OpCode::Sub); }
                    Operator::Multiply => { self.emit(OpCode::Mul); }
                    Operator::Divide => { self.emit(OpCode::Div); }
                    Operator::Equals => { self.emit(OpCode::Eq); }
                    Operator::NotEquals => { self.emit(OpCode::Ne); }
                    Operator::GreaterThan => { self.emit(OpCode::Gt); }
                    Operator::GreaterThanOrEqualTo => { self.emit(OpCode::Ge); }
                    Operator::LessThan => { self.emit(OpCode::Lt); }
                    Operator::LessThanOrEqualTo => { self.emit(OpCode::Le); }
                    Operator::Not => { self.emit(OpCode::Not); }
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
            Expression::ForLoop(for_loop) => {
                self.compile_for_loop_expression(for_loop)
            }
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
                            // Compile the for-loop expression (produces an array).
                            self.compile_for_loop_expression(for_loop)?;
                            // The result is an array on top; we need to spread
                            // its elements into the collector. We'll handle
                            // this with a special approach: push it, then use
                            // runtime logic. Actually, let's compile each
                            // iteration individually to append.
                            // For simplicity, re-implement inline:
                            // Actually the for_loop_expression already produces
                            // an array. We need to extend the collector.
                            // Let's just use AppendForExpr which appends a
                            // single value. For spread, the VM will detect
                            // array values and extend.
                            self.emit(OpCode::AppendForExpr);
                        }
                        Expression::Spread(inner) => {
                            self.compile_expression(inner)?;
                            // AppendForExpr – VM will spread array values.
                            self.emit(OpCode::AppendForExpr);
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
        let parent = std::mem::replace(
            self,
            Compiler::new("<dummy>".to_string(), 0),
        );
        let mut child = parent.child(name.to_string(), arity);

        // Declare parameters as locals in the child scope.
        for param in &func.arguments {
            child.add_local(param.get(), false);
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
        // Compile the scrutinee.
        self.compile_expression(&m.scrutinee)?;

        let mut end_jumps: Vec<usize> = Vec::new();

        for (pattern, block) in &m.arms {
            // Check for wildcard `_`
            let is_wildcard = matches!(
                pattern,
                Expression::Literal(Literal::Identifier(ident)) if ident.get() == "_"
            );

            if is_wildcard {
                // Always matches – compile block directly.
                // Pop the scrutinee copy (we don't need it for wildcard).
                self.emit(OpCode::Pop);
                self.begin_scope();
                self.compile_block(block)?;
                self.end_scope();
                let end_jump = self.emit_jump(OpCode::Jump(0));
                end_jumps.push(end_jump);
            } else {
                // Duplicate the scrutinee for comparison (it stays for the
                // next arm if this one doesn't match).
                // We don't have a Dup instruction, so we'll use GetLocal
                // if the scrutinee is a local. For simplicity, let's
                // store the scrutinee as a hidden local.
                // Actually, let's just re-read it: the scrutinee is on
                // top of the stack. We can read it with GetLocal if we
                // track its slot.

                // The scrutinee was pushed before the match arms; its slot
                // depends on how many locals we have. Let's handle this by
                // noting the stack position.
                // Simplest approach: emit a duplicate by re-getting the
                // local. But we don't have the slot tracked because the
                // scrutinee isn't declared as a local.

                // Let's declare a hidden local for the scrutinee before
                // entering the arms. We'll refactor: add the scrutinee
                // as a local before the loop.
                // This requires restructuring. Let's do it.
                // ... Actually, let me just use a pattern of
                // re-compiling the scrutinee. That's not ideal but works.
                // For now, let's use a simpler approach:

                // Compile the pattern value.
                self.compile_expression(pattern)?;
                // Eq compares top two values and pushes bool.
                // But we need the scrutinee to remain for the next arm.
                // So we need to duplicate it first. Let's introduce
                // a hidden local approach.

                // The simplest correct approach without a Dup instruction:
                // We know the scrutinee is at a fixed stack position.
                // Since we pushed it before arms, it's at current
                // stack depth = number of locals at that point.
                // Let's just use GetLocal with the slot we're about to
                // compute.

                // Actually, the scrutinee is already on the stack but not
                // named. Its slot index is self.locals.len() (at the time
                // we push it). But we only compile the scrutinee once at
                // the top. Let me restructure the whole match compilation.
                // See restructured version below.

                // For correctness, let's fall back to the Eq approach:
                // the scrutinee has been consumed by the first comparison.
                // We need to re-push it. The easiest (if suboptimal) way
                // is to re-compile the scrutinee expression each arm.
                // That's what we'll do for now.

                // Pop the scrutinee we pushed at the top; we'll re-push
                // per arm.
                // This is getting complicated. Let me use the hidden-local
                // approach properly.
                return self.compile_match_with_local(m);
            }
        }

        // Patch end jumps.
        for j in end_jumps {
            self.patch_jump(j);
        }
        Ok(())
    }

    fn compile_match_with_local(
        &mut self,
        m: &MatchExpression,
    ) -> Result<(), VMError> {
        // We've already emitted MarkBranched and the scrutinee is on the
        // stack. But this function is called from compile_match partway
        // through. Let's redo from scratch – the caller should not have
        // emitted anything yet. Actually, compile_match already emitted
        // MarkBranched and the scrutinee. We need to account for that.

        // The scrutinee is on the stack. Treat it as a hidden local.
        let scrutinee_slot = self.add_local("$scrutinee".to_string(), false);

        let mut end_jumps: Vec<usize> = Vec::new();

        for (pattern, block) in &m.arms {
            let is_wildcard = matches!(
                pattern,
                Expression::Literal(Literal::Identifier(ident)) if ident.get() == "_"
            );

            if is_wildcard {
                self.begin_scope();
                self.compile_block(block)?;
                self.end_scope();
                // Jump to end (skip remaining arms).
                let end_jump = self.emit_jump(OpCode::Jump(0));
                end_jumps.push(end_jump);
            } else {
                // Push scrutinee, push pattern, compare.
                self.emit(OpCode::GetLocal(scrutinee_slot));
                self.compile_expression(pattern)?;
                self.emit(OpCode::Eq);
                let skip_jump = self.emit_jump(OpCode::JumpIfFalse(0));

                // Match: compile body.
                self.begin_scope();
                self.compile_block(block)?;
                self.end_scope();
                let end_jump = self.emit_jump(OpCode::Jump(0));
                end_jumps.push(end_jump);

                self.patch_jump(skip_jump);
            }
        }

        // If no arm matched, push Void.
        self.emit(OpCode::Void);

        for j in end_jumps {
            self.patch_jump(j);
        }

        // Pop the hidden scrutinee local.
        // (end_scope won't do it because we're at the same scope depth.)
        // We leave the match result on the stack; the scrutinee slot will
        // be cleaned up by the enclosing scope.
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
