use std::cell::RefCell;
use std::fmt;
use std::rc::Rc;

use crate::runtime::value::Value;

// ---------------------------------------------------------------------------
// OpCode – the instruction set for the Ogham bytecode VM
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum OpCode {
    // -- Constants ----------------------------------------------------------
    /// Push `constants[idx]` onto the stack.
    Constant(u16),
    True,
    False,
    Void,

    // -- Stack --------------------------------------------------------------
    Pop,
    /// Close any open upvalue pointing to the top-of-stack slot, then pop.
    /// Emitted by the compiler when a captured local goes out of scope.
    CloseUpvalue,
    /// Duplicate the value on top of the stack.
    Dup,

    // -- Local variables (by stack-slot index) ------------------------------
    GetLocal(u8),
    SetLocal(u8),

    // -- Upvalues (closed-over variables) -----------------------------------
    GetUpvalue(u8),
    SetUpvalue(u8),

    // -- State variables (constant-pool index for the variable name) --------
    /// Push the current state value for the named variable.
    GetState(u16),
    /// Declare a state variable (initialize or retrieve persisted value).
    /// The initializer value is on top of the stack.
    DeclareState(u16),
    /// Pop value, write it to state, and flag a rerender.
    SetState(u16),

    // -- Host state ---------------------------------------------------------
    GetHostState(u16),

    // -- Arithmetic ---------------------------------------------------------
    Add,
    Sub,
    Mul,
    Div,
    Negate,

    // -- Comparison ---------------------------------------------------------
    Eq,
    Ne,
    Gt,
    Ge,
    Lt,
    Le,
    Not,

    // -- Control flow -------------------------------------------------------
    /// Unconditional relative jump (signed offset added to ip).
    Jump(i16),
    /// Pop top of stack; jump if falsy.
    JumpIfFalse(i16),
    /// Jump backwards by `offset` instructions (used for loops).
    Loop(u16),
    /// Return from the current function.
    Return,

    // -- Functions / closures -----------------------------------------------
    /// Create a closure from `FunctionProto` stored at `constants[idx]`.
    Closure(u16),
    /// Call the value at stack position `stack[top - arg_count - 1]` with
    /// `arg_count` arguments that sit above it on the stack.
    Call(u8),

    // -- Collections --------------------------------------------------------
    /// Pop N values from the stack and push them as an `Array`.
    Array(u16),
    /// Pop N key-value pairs (key: String constant index pushed before value)
    /// and push a `Map`. Each pair is (key_constant_idx, value) on the stack,
    /// so 2*N values are consumed.
    Map(u16),
    /// Pop index, pop array, push `array[index]`.
    GetIndex,
    /// Pop object, push `object.property` (property name from constants).
    GetProperty(u16),

    // -- Widgets ------------------------------------------------------------
    /// Build a widget. The identifier string constant index and property count
    /// are encoded. The stack contains N key-constant-index / value pairs
    /// (2*N items) with the identifier already encoded in the instruction.
    Widget {
        identifier_constant: u16,
        property_count: u16,
    },

    // -- Built-ins ----------------------------------------------------------
    /// Pop array, push its length as an integer.
    ArrayLength,
    /// Emit an event: pop `arg_count` args, then pop the event-name string.
    EmitEvent(u8),
    /// Pop value and log it.
    Log,

    // -- Branching flag -----------------------------------------------------
    /// Mark that branching has occurred in the current function (for state
    /// declaration validation).
    MarkBranched,

    // -- Import -------------------------------------------------------------
    /// Import a module. The constant-pool index points to import metadata.
    Import(u16),

    // -- For-loop expression (collect results into array) -------------------
    /// Begin a for-loop expression that collects results.
    /// Pushes an empty results array onto the stack.
    BeginForExpr,
    /// Append the top-of-stack value to the results array that sits below it.
    AppendForExpr,
    /// Spread-append: pop top-of-stack value. If it is an Array, extend the
    /// collector with its elements; otherwise push it as a single element.
    SpreadForExpr,
}

// ---------------------------------------------------------------------------
// Chunk – a flat sequence of instructions plus a constant pool
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct Chunk {
    pub code: Vec<OpCode>,
    pub constants: Vec<Value>,
    /// One entry per instruction – the source line that produced it.
    pub lines: Vec<usize>,
}

impl Chunk {
    pub fn new() -> Self {
        Self {
            code: Vec::new(),
            constants: Vec::new(),
            lines: Vec::new(),
        }
    }

    /// Append an instruction and return its index.
    pub fn emit(&mut self, op: OpCode, line: usize) -> usize {
        let idx = self.code.len();
        self.code.push(op);
        self.lines.push(line);
        idx
    }

    /// Add a constant to the pool and return its index.
    pub fn add_constant(&mut self, value: Value) -> u16 {
        let idx = self.constants.len();
        self.constants.push(value);
        idx as u16
    }

    /// Patch a previously emitted jump instruction with the correct offset.
    pub fn patch_jump(&mut self, jump_idx: usize) {
        // The offset is from the instruction *after* the jump to the current
        // position.
        let offset = self.code.len() as i16 - jump_idx as i16 - 1;
        match &mut self.code[jump_idx] {
            OpCode::Jump(ref mut o) => *o = offset,
            OpCode::JumpIfFalse(ref mut o) => *o = offset,
            _ => panic!("patch_jump called on non-jump instruction"),
        }
    }
}

// ---------------------------------------------------------------------------
// UpvalueDescriptor – tells the VM how to capture an upvalue when creating
// a closure at runtime.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum UpvalueDescriptor {
    /// Captures a local variable from the immediately enclosing function at
    /// the given stack-slot index.
    Local(u8),
    /// Captures an upvalue from the immediately enclosing function at the
    /// given upvalue index.
    Upvalue(u8),
}

// ---------------------------------------------------------------------------
// FunctionProto – the compiled (immutable) representation of a function.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct FunctionProto {
    pub name: String,
    pub arity: u8,
    pub chunk: Chunk,
    pub upvalue_count: u8,
    pub upvalues: Vec<UpvalueDescriptor>,
    /// Sub-function prototypes (referenced by Closure opcodes).
    pub protos: Vec<FunctionProto>,
}

impl FunctionProto {
    pub fn new(name: String, arity: u8) -> Self {
        Self {
            name,
            arity,
            chunk: Chunk::new(),
            upvalue_count: 0,
            upvalues: Vec::new(),
            protos: Vec::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// VMClosure – a runtime closure: a FunctionProto + captured upvalues.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct VMClosure {
    pub proto: Rc<FunctionProto>,
    pub upvalues: Vec<Rc<RefCell<Upvalue>>>,
    /// The call-stack path captured at closure-creation time (mirrors the
    /// tree-walk interpreter's `captured_path` for state management).
    pub captured_path: Vec<String>,
}

impl PartialEq for VMClosure {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.proto, &other.proto)
    }
}

// ---------------------------------------------------------------------------
// Upvalue – either an open reference to a stack slot, or a closed-over value.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum Upvalue {
    /// Points to a value that is still live on the VM stack at `index`.
    Open(usize),
    /// The value has been moved off the stack and is owned here.
    Closed(Value),
}

// ---------------------------------------------------------------------------
// ImportMeta – metadata stored in the constant pool for Import instructions.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct ImportMeta {
    /// Optional list of names to import (None = import all).
    pub names: Option<Vec<String>>,
    /// Module path (e.g. "components/button").
    pub path: String,
}

impl fmt::Display for ImportMeta {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "<import {}>", self.path)
    }
}
