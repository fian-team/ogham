//! Stack-based bytecode VM that executes compiled Ogham programs.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::parser::Identifier;
use crate::runtime::compiler::deserialize_import_meta;
use crate::runtime::error::VMError;
use crate::runtime::opcode::{FunctionProto, OpCode, Upvalue, UpvalueDescriptor, VMClosure};
use crate::runtime::ops;
use crate::runtime::value::Value;
use crate::runtime::descriptor::WidgetDescriptor;
use crate::runtime::Runtime;

// ---------------------------------------------------------------------------
// Safety limits
// ---------------------------------------------------------------------------

const MAX_STACK_SIZE: usize = 10_000;
const MAX_ITERATIONS: u64 = 1_000_000;
const MAX_CALL_DEPTH: usize = 1_000;

// ---------------------------------------------------------------------------
// CallFrame – one per active function invocation.
// ---------------------------------------------------------------------------

struct CallFrame {
    closure: Rc<VMClosure>,
    /// Instruction pointer – index into `closure.proto.chunk.code`.
    ip: usize,
    /// Where this frame's locals begin on the value stack.
    slot_offset: usize,
    /// Saved runtime call stack – restored when this frame returns.
    saved_call_stack: Option<Vec<String>>,
    /// Saved `has_branched` flag – restored when this frame returns.
    saved_has_branched: Option<bool>,
}

// ---------------------------------------------------------------------------
// VM
// ---------------------------------------------------------------------------

pub struct VM {
    stack: Vec<Value>,
    frames: Vec<CallFrame>,
    /// Open upvalues keyed by the stack index they point to. Kept sorted by
    /// index so we can close them efficiently when a scope exits.
    open_upvalues: Vec<Rc<RefCell<Upvalue>>>,
    /// Iteration counter for detecting infinite loops.
    iteration_count: u64,
}

impl VM {
    pub fn new() -> Self {
        Self {
            stack: Vec::with_capacity(256),
            frames: Vec::with_capacity(64),
            open_upvalues: Vec::new(),
            iteration_count: 0,
        }
    }

    /// Extract top-level local variables from the stack after a module has
    /// finished executing. `local_names` maps variable names to their stack
    /// slot indices (produced by [`Compiler::compile_import`]).
    pub fn read_stack_locals(&self, local_names: &[(String, u8)]) -> HashMap<String, Value> {
        let mut map = HashMap::new();
        for (name, slot) in local_names {
            if let Some(value) = self.stack.get(*slot as usize) {
                map.insert(name.clone(), value.clone());
            }
        }
        map
    }

    // -- Stack helpers ------------------------------------------------------

    fn push(&mut self, value: Value) -> Result<(), VMError> {
        if self.stack.len() >= MAX_STACK_SIZE {
            return Err(VMError::StackOverflow);
        }
        self.stack.push(value);
        Ok(())
    }

    fn pop(&mut self) -> Result<Value, VMError> {
        self.stack.pop().ok_or(VMError::StackUnderflow)
    }

    fn peek(&self, distance: usize) -> &Value {
        &self.stack[self.stack.len() - 1 - distance]
    }

    // -- Frame helpers ------------------------------------------------------

    fn current_frame(&self) -> &CallFrame {
        self.frames.last().expect("no call frame")
    }

    fn current_frame_mut(&mut self) -> &mut CallFrame {
        self.frames.last_mut().expect("no call frame")
    }

    fn read_instruction(&mut self) -> Result<OpCode, VMError> {
        let frame = self.current_frame_mut();
        if frame.ip >= frame.closure.proto.chunk.code.len() {
            return Err(VMError::InvalidOperation(
                "Instruction pointer out of bounds".to_string(),
            ));
        }
        let op = frame.closure.proto.chunk.code[frame.ip].clone();
        frame.ip += 1;
        Ok(op)
    }

    fn read_constant(&self, idx: u16) -> &Value {
        &self.current_frame().closure.proto.chunk.constants[idx as usize]
    }

    /// Read a constant at `idx` and return it as a `String`, or error if it
    /// is not a `Value::String`.
    fn read_string_constant(&self, idx: u16) -> Result<String, VMError> {
        match self.read_constant(idx) {
            Value::String(s) => Ok(s.clone()),
            _ => Err(VMError::InvalidOperation(
                "expected string constant".to_string(),
            )),
        }
    }

    // -- Upvalue helpers ----------------------------------------------------

    fn capture_upvalue(&mut self, stack_index: usize) -> Rc<RefCell<Upvalue>> {
        // Check if we already have an open upvalue for this slot.
        for uv in &self.open_upvalues {
            if let Upvalue::Open(idx) = *uv.borrow() {
                if idx == stack_index {
                    return uv.clone();
                }
            }
        }
        let uv = Rc::new(RefCell::new(Upvalue::Open(stack_index)));
        self.open_upvalues.push(uv.clone());
        uv
    }

    /// Close all open upvalues whose stack index is >= `last`.
    fn close_upvalues(&mut self, last: usize) {
        let mut i = 0;
        while i < self.open_upvalues.len() {
            let should_close = {
                let uv = self.open_upvalues[i].borrow();
                match *uv {
                    Upvalue::Open(idx) => idx >= last,
                    Upvalue::Closed(_) => false,
                }
            };
            if should_close {
                let stack_idx = match *self.open_upvalues[i].borrow() {
                    Upvalue::Open(idx) => idx,
                    _ => unreachable!(),
                };
                let value = self.stack[stack_idx].clone();
                *self.open_upvalues[i].borrow_mut() = Upvalue::Closed(value);
                self.open_upvalues.swap_remove(i);
            } else {
                i += 1;
            }
        }
    }

    // -- Read/write upvalue values ------------------------------------------

    fn get_upvalue(&self, uv: &Rc<RefCell<Upvalue>>) -> Value {
        match &*uv.borrow() {
            Upvalue::Open(idx) => self.stack[*idx].clone(),
            Upvalue::Closed(val) => val.clone(),
        }
    }

    fn set_upvalue(&mut self, uv: &Rc<RefCell<Upvalue>>, value: Value) {
        let mut uv_ref = uv.borrow_mut();
        match &mut *uv_ref {
            Upvalue::Open(idx) => {
                self.stack[*idx] = value;
            }
            Upvalue::Closed(ref mut val) => {
                *val = value;
            }
        }
    }

    // -----------------------------------------------------------------------
    // Main execution loop
    // -----------------------------------------------------------------------

    /// Run a compiled `FunctionProto` in the context of the given `Runtime`.
    /// Returns the final value produced by the module/function.
    pub fn run(&mut self, proto: &FunctionProto, runtime: &mut Runtime) -> Result<Value, VMError> {
        // Set up the top-level closure and call frame.
        // NOTE: Unlike regular function calls, we do NOT push the module closure
        // onto the stack, because the module's local variables should start at
        // stack[0], not stack[1].
        let closure = Rc::new(VMClosure {
            proto: Rc::new(proto.clone()),
            upvalues: Vec::new(),
            captured_path: Vec::new(),
        });
        self.frames.push(CallFrame {
            closure,
            ip: 0,
            slot_offset: 0,
            saved_call_stack: None,
            saved_has_branched: None,
        });

        self.execute(runtime)
    }

    /// Run a `VMClosure` that was stored as an event handler or callback.
    /// Used when the widget tree fires an event and the handler is a bytecode
    /// closure.
    pub fn call_closure(
        &mut self,
        closure: &Rc<VMClosure>,
        args: &[Value],
        runtime: &mut Runtime,
    ) -> Result<Value, VMError> {
        // Push the closure, then the arguments.
        self.push(Value::BytecodeClosure(closure.clone()))?;
        for arg in args {
            self.push(arg.clone())?;
        }
        self.call_vm_closure(closure, args.len() as u8)?;
        self.execute(runtime)
    }

    // -- Internal call setup ------------------------------------------------

    fn call_vm_closure(&mut self, closure: &Rc<VMClosure>, arg_count: u8) -> Result<(), VMError> {
        if arg_count != closure.proto.arity {
            return Err(VMError::InvalidOperation(format!(
                "Expected {} arguments, got {}",
                closure.proto.arity, arg_count
            )));
        }
        if self.frames.len() >= MAX_CALL_DEPTH {
            return Err(VMError::CallStackOverflow);
        }
        let slot_offset = self.stack.len() - arg_count as usize - 1;
        self.frames.push(CallFrame {
            closure: closure.clone(),
            ip: 0,
            saved_call_stack: None,
            saved_has_branched: None,
            slot_offset,
        });
        // Reset iteration counter on function calls
        self.iteration_count = 0;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Core instruction dispatch
    // -----------------------------------------------------------------------

    fn execute(&mut self, runtime: &mut Runtime) -> Result<Value, VMError> {
        loop {
            let instruction = self.read_instruction()?;

            match instruction {
                // -- Constants -----------------------------------------------
                OpCode::Constant(idx) => {
                    let val = self.read_constant(idx).clone();
                    self.push(val)?;
                }
                OpCode::True => self.push(Value::Boolean(true))?,
                OpCode::False => self.push(Value::Boolean(false))?,
                OpCode::Void => self.push(Value::Void)?,

                // -- Stack ---------------------------------------------------
                OpCode::Pop => {
                    self.pop()?;
                }
                OpCode::CloseUpvalue => {
                    // Close any open upvalue pointing to the top-of-stack
                    // slot, then pop the value.
                    let top = self.stack.len() - 1;
                    self.close_upvalues(top);
                    self.pop()?;
                }
                OpCode::Dup => {
                    let val = self
                        .stack
                        .last()
                        .ok_or_else(|| VMError::InvalidOperation("Dup on empty stack".into()))?
                        .clone();
                    self.push(val)?;
                }

                // -- Locals --------------------------------------------------
                OpCode::GetLocal(slot) => {
                    let idx = self.current_frame().slot_offset + slot as usize;
                    let val = self.stack[idx].clone();
                    self.push(val)?;
                }
                OpCode::SetLocal(slot) => {
                    let idx = self.current_frame().slot_offset + slot as usize;
                    let val = self.peek(0).clone();
                    self.stack[idx] = val;
                    // SetLocal does NOT pop – the value remains on the stack
                    // for the case where assignment is part of an expression.
                    // The statement compilation emits Pop if needed.
                    self.pop()?; // Actually, assignments are statements, so pop.
                }

                // -- Upvalues ------------------------------------------------
                OpCode::GetUpvalue(idx) => {
                    let uv = self.current_frame().closure.upvalues[idx as usize].clone();
                    let val = self.get_upvalue(&uv);
                    self.push(val)?;
                }
                OpCode::SetUpvalue(idx) => {
                    let val = self.pop()?;
                    let uv = self.current_frame().closure.upvalues[idx as usize].clone();
                    self.set_upvalue(&uv, val);
                }

                // -- State ---------------------------------------------------
                OpCode::GetState(name_idx) => {
                    let name = self.read_string_constant(name_idx)?;
                    // Lookup order: state → host state → built-in screen
                    // variables. If none match, it's an undefined variable.
                    if let Some(val) = runtime.state.get_state_value(&name) {
                        self.push(val)?;
                    } else if let Some(val) = runtime.get_host_state(&name) {
                        self.push(val)?;
                    } else if name == "screen_width" {
                        self.push(Value::Float(runtime.screen_width as f64))?;
                    } else if name == "screen_height" {
                        self.push(Value::Float(runtime.screen_height as f64))?;
                    } else {
                        return Err(VMError::UndefinedVariable(name));
                    }
                }
                OpCode::DeclareState(name_idx) => {
                    let name = self.read_string_constant(name_idx)?;
                    // The initializer value is on top of the stack.
                    let init_value = self.pop()?;

                    // Check if state already exists at the current call-stack
                    // path. If so, use the persisted value; otherwise
                    // initialise.
                    let state_key = runtime.state.get_state_key(&name);
                    let value = if let Some(existing) = runtime.state.component_state.get(&state_key) {
                        existing.clone()
                    } else {
                        runtime.state
                            .component_state
                            .insert(state_key.clone(), init_value.clone());
                        init_value
                    };

                    // Record active path.
                    let path = runtime.state.get_call_stack_path();
                    if !path.is_empty() {
                        runtime.state.active_state_paths.insert(path);
                    }

                    // Push the value as a local.
                    self.push(value)?;
                }
                OpCode::SetState(name_idx) => {
                    let name = self.read_string_constant(name_idx)?;
                    let value = self.peek(0).clone();
                    runtime.state.set_state_value(&name, value);
                    runtime.request_rerender();
                }

                // -- Host state ----------------------------------------------
                OpCode::GetHostState(name_idx) => {
                    let name = self.read_string_constant(name_idx)?;
                    if let Some(val) = runtime.get_host_state(&name) {
                        self.push(val)?;
                    } else {
                        return Err(VMError::UndefinedVariable(name));
                    }
                }

                // -- Arithmetic ----------------------------------------------
                OpCode::Add => {
                    let b = self.pop()?;
                    let a = self.pop()?;
                    self.push(ops::add(&a, &b)?)?;
                }
                OpCode::Sub => {
                    let b = self.pop()?;
                    let a = self.pop()?;
                    self.push(ops::subtract(&a, &b)?)?;
                }
                OpCode::Mul => {
                    let b = self.pop()?;
                    let a = self.pop()?;
                    self.push(ops::multiply(&a, &b)?)?;
                }
                OpCode::Div => {
                    let b = self.pop()?;
                    let a = self.pop()?;
                    self.push(ops::divide(&a, &b)?)?;
                }
                OpCode::Mod => {
                    let b = self.pop()?;
                    let a = self.pop()?;
                    self.push(ops::modulo(&a, &b)?)?;
                }
                OpCode::Pow => {
                    let b = self.pop()?;
                    let a = self.pop()?;
                    self.push(ops::power(&a, &b)?)?;
                }
                OpCode::Negate => {
                    let val = self.pop()?;
                    match val {
                        Value::Integer(i) => self.push(Value::Integer(-i))?,
                        Value::Float(f) => self.push(Value::Float(-f))?,
                        _ => return Err(VMError::TypeMismatch(format!("Cannot negate {:?}", val))),
                    }
                }

                // -- Comparison ----------------------------------------------
                OpCode::Eq => {
                    let b = self.pop()?;
                    let a = self.pop()?;
                    self.push(ops::equals(&a, &b)?)?;
                }
                OpCode::Ne => {
                    let b = self.pop()?;
                    let a = self.pop()?;
                    self.push(ops::not_equals(&a, &b)?)?;
                }
                OpCode::Gt => {
                    let b = self.pop()?;
                    let a = self.pop()?;
                    self.push(ops::greater_than(&a, &b)?)?;
                }
                OpCode::Ge => {
                    let b = self.pop()?;
                    let a = self.pop()?;
                    self.push(ops::greater_than_or_equal(&a, &b)?)?;
                }
                OpCode::Lt => {
                    let b = self.pop()?;
                    let a = self.pop()?;
                    self.push(ops::less_than(&a, &b)?)?;
                }
                OpCode::Le => {
                    let b = self.pop()?;
                    let a = self.pop()?;
                    self.push(ops::less_than_or_equal(&a, &b)?)?;
                }
                OpCode::Not => {
                    let val = self.pop()?;
                    match val {
                        Value::Boolean(b) => self.push(Value::Boolean(!b))?,
                        _ => {
                            return Err(VMError::TypeMismatch(format!(
                                "Cannot negate (!) {:?}",
                                val
                            )))
                        }
                    }
                }
                OpCode::And => {
                    let b = self.pop()?;
                    let a = self.pop()?;
                    match (a, b) {
                        (Value::Boolean(a), Value::Boolean(b)) => {
                            self.push(Value::Boolean(a && b))?
                        }
                        (a, b) => {
                            return Err(VMError::TypeMismatch(format!(
                                "Cannot apply && to {:?} and {:?}",
                                a, b
                            )))
                        }
                    }
                }
                OpCode::Or => {
                    let b = self.pop()?;
                    let a = self.pop()?;
                    match (a, b) {
                        (Value::Boolean(a), Value::Boolean(b)) => {
                            self.push(Value::Boolean(a || b))?
                        }
                        (a, b) => {
                            return Err(VMError::TypeMismatch(format!(
                                "Cannot apply || to {:?} and {:?}",
                                a, b
                            )))
                        }
                    }
                }

                // -- Control flow --------------------------------------------
                OpCode::Jump(offset) => {
                    let frame = self.current_frame_mut();
                    frame.ip = (frame.ip as i64 + offset as i64) as usize;
                }
                OpCode::JumpIfFalse(offset) => {
                    let cond = self.pop()?;
                    if let Value::Boolean(false) = cond {
                        let frame = self.current_frame_mut();
                        frame.ip = (frame.ip as i64 + offset as i64) as usize;
                    }
                }
                OpCode::Loop(offset) => {
                    let frame = self.current_frame_mut();
                    if offset as usize > frame.ip {
                        return Err(VMError::InvalidOperation(
                            "Loop offset underflow".to_string(),
                        ));
                    }
                    frame.ip -= offset as usize;

                    // Increment iteration counter for infinite loop detection
                    self.iteration_count += 1;
                    if self.iteration_count > MAX_ITERATIONS {
                        return Err(VMError::ExecutionLimitExceeded(
                            "Maximum iterations exceeded".to_string(),
                        ));
                    }
                }
                OpCode::Return => {
                    let result = self.pop()?;
                    let frame_offset = self.current_frame().slot_offset;
                    // Restore runtime call-stack state saved by Call.
                    let saved_cs = self.current_frame().saved_call_stack.clone();
                    let saved_hb = self.current_frame().saved_has_branched;
                    self.close_upvalues(frame_offset);
                    self.frames.pop();
                    if let Some(cs) = saved_cs {
                        runtime.state.call_stack = cs;
                    }
                    if let Some(hb) = saved_hb {
                        runtime.state.has_branched = hb;
                    }
                    if self.frames.is_empty() {
                        return Ok(result);
                    }
                    // Discard the callee and args from the stack.
                    self.stack.truncate(frame_offset);
                    self.push(result)?;
                }

                // -- Closures ------------------------------------------------
                OpCode::Closure(proto_idx) => {
                    let proto = {
                        let frame_closure = &self.current_frame().closure;
                        Rc::new(frame_closure.proto.protos[proto_idx as usize].clone())
                    };
                    // Capture upvalues as described by the proto.
                    let mut upvalues = Vec::with_capacity(proto.upvalue_count as usize);
                    for desc in &proto.upvalues {
                        let uv = match desc {
                            UpvalueDescriptor::Local(slot) => {
                                let abs_slot = self.current_frame().slot_offset + *slot as usize;
                                self.capture_upvalue(abs_slot)
                            }
                            UpvalueDescriptor::Upvalue(idx) => {
                                self.current_frame().closure.upvalues[*idx as usize].clone()
                            }
                        };
                        upvalues.push(uv);
                    }
                    let captured_path = runtime.state.call_stack.clone();
                    let vm_closure = Rc::new(VMClosure {
                        proto,
                        upvalues,
                        captured_path,
                    });
                    self.push(Value::BytecodeClosure(vm_closure))?;
                }
                OpCode::Call(arg_count) => {
                    let callee_idx = self.stack.len() - 1 - arg_count as usize;
                    let callee = self.stack[callee_idx].clone();

                    match callee {
                        Value::BytecodeClosure(closure) => {
                            // Save and set up the call stack path in the
                            // runtime for state management.
                            let old_call_stack = runtime.state.call_stack.clone();
                            let old_has_branched = runtime.state.has_branched;
                            runtime.state.call_stack = closure.captured_path.clone();

                            // Generate unique call identifier.
                            let func_name = &closure.proto.name;
                            let call_site_key = if runtime.state.call_stack.is_empty() {
                                func_name.clone()
                            } else {
                                format!("{}/{}", runtime.state.call_stack.join("/"), func_name)
                            };
                            let call_index =
                                runtime.state.call_counters.entry(call_site_key).or_insert(0);
                            *call_index += 1;
                            let current_idx = *call_index;
                            let unique_id = format!("{}@{}", func_name, current_idx);
                            runtime.state.call_stack.push(unique_id);
                            runtime.state.has_branched = false;

                            // Phase 2 lifecycle: when the module
                            // uses any lifecycle hooks, mark every
                            // function-call's path as visited so
                            // the per-render diff can identify
                            // newly-mounted/unmounted paths even
                            // for hookless functions. Modules
                            // without hooks pay one branch-
                            // predicted bool check and skip the
                            // allocation.
                            if runtime.lifecycle_active {
                                let path = runtime.state.get_call_stack_path();
                                if !path.is_empty() {
                                    runtime.state.active_state_paths.insert(path);
                                }
                            }

                            self.call_vm_closure(&closure, arg_count)?;

                            // Store restore info on the new frame so
                            // Return can restore the call stack.
                            let new_frame = self
                                .frames
                                .last_mut()
                                .expect("frame should exist after call_vm_closure");
                            new_frame.saved_call_stack = Some(old_call_stack);
                            new_frame.saved_has_branched = Some(old_has_branched);
                        }
                        Value::BoundTrigger(mutation) => {
                            // Pop args off the stack.
                            let ac = arg_count as usize;
                            let start = self.stack.len() - ac;
                            let args: Vec<Value> = self.stack[start..].to_vec();
                            self.stack.truncate(start);
                            // Pop the callee (the BoundTrigger itself).
                            self.pop()?;

                            let event_name = mutation.borrow().event_name.clone();
                            mutation.borrow_mut().status =
                                crate::runtime::value::MutationStatus::Pending;
                            match runtime.emit_event(&event_name, &args) {
                                Ok(data) => {
                                    let mut m = mutation.borrow_mut();
                                    m.status = crate::runtime::value::MutationStatus::Success;
                                    m.data = data;
                                    m.error.clear();
                                }
                                Err(msg) => {
                                    let mut m = mutation.borrow_mut();
                                    m.status = crate::runtime::value::MutationStatus::Error;
                                    m.data = Value::Void;
                                    m.error = msg;
                                }
                            }
                            runtime.request_rerender();
                            self.push(Value::Void)?;
                        }
                        _ => {
                            return Err(VMError::TypeMismatch(
                                "Only functions can be called".to_string(),
                            ));
                        }
                    }
                }

                // -- Collections ---------------------------------------------
                OpCode::Array(count) => {
                    let mut arr = Vec::with_capacity(count as usize);
                    // Elements were pushed in order, so the first element is
                    // deepest on the stack.
                    let start = self.stack.len() - count as usize;
                    for i in start..self.stack.len() {
                        arr.push(self.stack[i].clone());
                    }
                    self.stack.truncate(start);
                    self.push(Value::Array(arr))?;
                }
                OpCode::Map(count) => {
                    let mut map = HashMap::new();
                    // Each pair is (key_string_constant, value) – 2*count items.
                    let pair_count = count as usize;
                    let start = self.stack.len() - pair_count * 2;
                    let mut i = start;
                    while i < self.stack.len() {
                        let key = match &self.stack[i] {
                            Value::String(s) => s.clone(),
                            _ => {
                                return Err(VMError::InvalidOperation(
                                    "Map key must be a string".to_string(),
                                ))
                            }
                        };
                        let value = self.stack[i + 1].clone();
                        map.insert(key, value);
                        i += 2;
                    }
                    self.stack.truncate(start);
                    self.push(Value::Map(map))?;
                }
                OpCode::GetIndex => {
                    let index_val = self.pop()?;
                    let object = self.pop()?;
                    match (&object, &index_val) {
                        (Value::Array(arr), Value::Integer(i)) => {
                            if *i < 0 {
                                return Err(VMError::InvalidOperation(
                                    "Array index must be non-negative".to_string(),
                                ));
                            }
                            let idx = *i as usize;
                            if idx >= arr.len() {
                                return Err(VMError::InvalidOperation(format!(
                                    "Index {} out of bounds for array of length {}",
                                    idx,
                                    arr.len()
                                )));
                            }
                            self.push(arr[idx].clone())?;
                        }
                        _ => {
                            return Err(VMError::TypeMismatch(format!(
                                "Cannot index {:?} with {:?}",
                                object, index_val
                            )));
                        }
                    }
                }
                OpCode::GetProperty(name_idx) => {
                    let name = self.read_string_constant(name_idx)?;
                    let object = self.pop()?;
                    match object {
                        Value::Map(map) => {
                            let val = map.get(&name).cloned().ok_or_else(|| {
                                VMError::InvalidOperation(format!("Map has no property '{}'", name))
                            })?;
                            self.push(val)?;
                        }
                        Value::Widget(widget) => {
                            let val = widget.properties.get(&name).cloned().ok_or_else(|| {
                                VMError::InvalidOperation(format!(
                                    "Widget has no property '{}'",
                                    name
                                ))
                            })?;
                            self.push(val)?;
                        }
                        Value::Mutation(m) => {
                            let val = match name.as_str() {
                                "trigger" => Value::BoundTrigger(m.clone()),
                                other => {
                                    let s = m.borrow();
                                    match other {
                                        "status" => Value::String(s.status.as_str().to_string()),
                                        "pending" => Value::Boolean(
                                            s.status
                                                == crate::runtime::value::MutationStatus::Pending,
                                        ),
                                        "data" => s.data.clone(),
                                        "error" => Value::String(s.error.clone()),
                                        _ => Value::Void,
                                    }
                                }
                            };
                            self.push(val)?;
                        }
                        _ => {
                            return Err(VMError::TypeMismatch(format!(
                                "Cannot access property '{}' on {:?}",
                                name, object
                            )));
                        }
                    }
                }

                // -- Widgets -------------------------------------------------
                OpCode::Widget {
                    identifier_constant,
                    property_count,
                } => {
                    let ident_name = self.read_string_constant(identifier_constant)?;
                    let mut props = HashMap::new();
                    let pc = property_count as usize;
                    // Stack has pc pairs of (key_string, value).
                    let start = self.stack.len() - pc * 2;
                    let mut i = start;
                    while i < self.stack.len() {
                        let key = match &self.stack[i] {
                            Value::String(s) => s.clone(),
                            _ => {
                                return Err(VMError::InvalidOperation(
                                    "Widget property key must be a string".to_string(),
                                ))
                            }
                        };
                        let value = self.stack[i + 1].clone();
                        props.insert(key, value);
                        i += 2;
                    }
                    self.stack.truncate(start);
                    // Capture the call-stack path NOW, while the VM
                    // is still inside the function that produced
                    // this descriptor. By the time the widget
                    // builder runs (after execute_module returns),
                    // call_stack will be empty.
                    let owned_path = runtime.state.get_call_stack_path();
                    self.push(Value::Widget(WidgetDescriptor {
                        identifier: Identifier::synthetic(&ident_name),
                        properties: props,
                        owned_path,
                    }))?;
                }

                // -- Built-ins -----------------------------------------------
                OpCode::ArrayLength => {
                    let val = self.pop()?;
                    match val {
                        Value::Array(arr) => {
                            self.push(Value::Integer(arr.len() as i32))?;
                        }
                        _ => {
                            return Err(VMError::InvalidOperation(
                                "length() can only be called on an array".to_string(),
                            ));
                        }
                    }
                }
                OpCode::EmitEvent(arg_count) => {
                    let ac = arg_count as usize;
                    // First arg is the event name; rest are extra args.
                    let start = self.stack.len() - ac;
                    let all_args: Vec<Value> = self.stack[start..].to_vec();
                    self.stack.truncate(start);

                    if all_args.is_empty() {
                        return Err(VMError::InvalidOperation(
                            "event() requires at least an event name".to_string(),
                        ));
                    }
                    let event_name = match &all_args[0] {
                        Value::String(s) => s.clone(),
                        other => {
                            return Err(VMError::TypeMismatch(format!(
                                "event() first arg must be string, got {:?}",
                                other
                            )))
                        }
                    };
                    // Fire-and-forget: `event()` always yields Void regardless
                    // of handler result. Tracked flows go through mutations.
                    let _ = runtime.emit_event(&event_name, &all_args[1..]);
                    self.push(Value::Void)?;
                }
                OpCode::Log => {
                    let val = self.pop()?;
                    eprintln!("[ogham] {}", val);
                    self.push(Value::Void)?;
                }
                OpCode::CreateMutation => {
                    let name_val = self.pop()?;
                    let name = match name_val {
                        Value::String(s) => s,
                        other => {
                            return Err(VMError::TypeMismatch(format!(
                                "mutation() expects a string event name, got {:?}",
                                other
                            )))
                        }
                    };
                    let state = std::rc::Rc::new(std::cell::RefCell::new(
                        crate::runtime::value::MutationState::new(name),
                    ));
                    self.push(Value::Mutation(state))?;
                }
                OpCode::PushContext => {
                    // Stack: ..., name, value
                    let value = self.pop()?;
                    let name_val = self.pop()?;
                    let name = match name_val {
                        Value::String(s) => s,
                        other => {
                            return Err(VMError::TypeMismatch(format!(
                                "Context name must be a string, got {:?}",
                                other
                            )))
                        }
                    };
                    runtime.context_stack.push((name, value));
                }
                OpCode::PopContext => {
                    runtime.context_stack.pop();
                }
                OpCode::GetContext(name_idx) => {
                    let name = self.read_string_constant(name_idx)?;
                    let value = runtime
                        .context_stack
                        .iter()
                        .rev()
                        .find(|(k, _)| k == &name)
                        .map(|(_, v)| v.clone())
                        .unwrap_or(Value::Void);
                    self.push(value)?;
                }

                // -- Branching flag ------------------------------------------
                OpCode::MarkBranched => {
                    runtime.state.has_branched = true;
                }

                // -- Import --------------------------------------------------
                OpCode::Import(meta_idx) => {
                    let meta_str = self.read_string_constant(meta_idx)?;
                    let meta = deserialize_import_meta(&meta_str).ok_or_else(|| {
                        VMError::ImportError(format!("Invalid import metadata: {}", meta_str))
                    })?;

                    // Delegate to the Runtime import machinery which
                    // compiles the imported file to bytecode, runs it in
                    // a fresh VM, and merges exported names into the
                    // runtime's Environment.
                    let import_stmt = crate::parser::ImportStatement {
                        names: meta.names.clone(),
                        path: meta.path.clone(),
                        span: crate::parser::Span::zero(),
                    };
                    runtime.execute_import(&import_stmt)?;

                    // After import, the exported names are in
                    // runtime.environment. Inject them as host state so
                    // the VM can find them via GetState (which falls
                    // through to host state).
                    let env_vars = runtime.environment.top_level_variables().clone();
                    for (name, value) in env_vars {
                        runtime.inject_host_state(name, value);
                    }
                }

                // -- For-loop expression helpers -----------------------------
                OpCode::BeginForExpr => {
                    self.push(Value::Array(Vec::new()))?;
                }
                OpCode::AppendForExpr => {
                    let value = self.pop()?;
                    let stack_top = self.stack.len();
                    let mut found = false;
                    for i in (0..stack_top).rev() {
                        if let Value::Array(ref mut arr) = self.stack[i] {
                            arr.push(value.clone());
                            found = true;
                            break;
                        }
                    }
                    if !found {
                        return Err(VMError::InvalidOperation(
                            "AppendForExpr: no collector array found".to_string(),
                        ));
                    }
                }
                OpCode::SpreadForExpr => {
                    let value = self.pop()?;
                    let stack_top = self.stack.len();
                    let mut found = false;
                    for i in (0..stack_top).rev() {
                        if let Value::Array(ref mut arr) = self.stack[i] {
                            match value {
                                Value::Array(ref spread_arr) => {
                                    arr.extend(spread_arr.iter().cloned());
                                }
                                _ => {
                                    arr.push(value.clone());
                                }
                            }
                            found = true;
                            break;
                        }
                    }
                    if !found {
                        return Err(VMError::InvalidOperation(
                            "SpreadForExpr: no collector array found".to_string(),
                        ));
                    }
                }

                // -- Lifecycle hooks (Phase 2) -------------------------------
                // RegisterMountHook (M1): pop closure; queue for
                // post-layout fire IF the path is newly mounted
                // this frame (i.e. not in previous_active_paths).
                // Otherwise drop the closure — mount fires once
                // per path-lifetime.
                OpCode::RegisterMountHook(hook_id) => {
                    let closure_value = self.pop()?;
                    let closure = match closure_value {
                        Value::BytecodeClosure(c) => c,
                        other => {
                            return Err(VMError::InvalidOperation(format!(
                                "RegisterMountHook expected closure on stack, got {:?}",
                                other
                            )));
                        }
                    };
                    let path = runtime.state.get_call_stack_path();
                    if !path.is_empty()
                        && !runtime.state.previous_active_paths.contains(&path)
                    {
                        runtime
                            .state
                            .pending_mounts
                            .push((path, hook_id, closure));
                    }
                    // Else: path was already active last frame, OR
                    // we're at module top-level (empty path). Drop.
                }
                // RegisterUnmountHook (M1): pop closure; insert
                // into the persistent map at (path, hook_id),
                // overwriting any prior entry. Re-registration
                // every render keeps the closure's upvalues fresh.
                OpCode::RegisterUnmountHook(hook_id) => {
                    let closure_value = self.pop()?;
                    let closure = match closure_value {
                        Value::BytecodeClosure(c) => c,
                        other => {
                            return Err(VMError::InvalidOperation(format!(
                                "RegisterUnmountHook expected closure on stack, got {:?}",
                                other
                            )));
                        }
                    };
                    let path = runtime.state.get_call_stack_path();
                    if !path.is_empty() {
                        runtime
                            .state
                            .unmount_hooks
                            .insert((path, hook_id), closure);
                    }
                    // Top-level (path == "") unmount hooks would
                    // fire only on runtime shutdown; out of scope.
                }
                // M2 will implement these; M1 leaves them as
                // not-yet-implemented errors. The compiler in M1
                // does not emit either of these opcodes.
                OpCode::RegisterEffect { .. }
                | OpCode::RegisterEffectCleanup => {
                    return Err(VMError::InvalidOperation(
                        "effect / cleanup not implemented until M2"
                            .to_string(),
                    ));
                }
            }
        }
    }
}
