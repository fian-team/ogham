//! Error types for the Ogham runtime and VM.

use std::path::PathBuf;

use crate::{parser::SyntaxError, runtime::Value, tree::ast_bridge};

/// Errors that can occur during bytecode compilation or VM execution.
#[derive(Debug)]
pub enum VMError {
    UndefinedVariable(String),
    TypeMismatch(String),
    InvalidOperation(String),
    Return(Value),
    ImportCycle(Vec<PathBuf>),
    ImportError(String),
    ImportConflict(String),
    StackOverflow,
    StackUnderflow,
    ExecutionLimitExceeded(String),
    CallStackOverflow,
}

/// Aggregated error type for all runtime execution stages.
#[derive(Debug)]
pub enum RuntimeError {
    /// File I/O error (file not found, permission denied, etc.)
    IoError(std::io::Error),
    /// Syntax error during parsing
    SyntaxError(SyntaxError),
    /// Runtime error during execution
    VmError(VMError),
    /// Error during AST to UI bridge conversion
    BridgeError(ast_bridge::BridgeError),
}

impl From<std::io::Error> for RuntimeError {
    fn from(err: std::io::Error) -> Self {
        RuntimeError::IoError(err)
    }
}

impl From<SyntaxError> for RuntimeError {
    fn from(err: SyntaxError) -> Self {
        RuntimeError::SyntaxError(err)
    }
}

impl From<VMError> for RuntimeError {
    fn from(err: VMError) -> Self {
        RuntimeError::VmError(err)
    }
}

impl From<ast_bridge::BridgeError> for RuntimeError {
    fn from(err: ast_bridge::BridgeError) -> Self {
        RuntimeError::BridgeError(err)
    }
}

impl std::fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RuntimeError::IoError(e) => write!(f, "I/O error: {}", e),
            RuntimeError::SyntaxError(e) => {
                write!(f, "Syntax error at {}:{}: {}", e.line, e.column, e.message)
            }
            RuntimeError::VmError(e) => {
                write!(f, "Runtime error: {:?}", e)
            }
            RuntimeError::BridgeError(e) => {
                write!(f, "Bridge error: {:?}", e)
            }
        }
    }
}

impl std::error::Error for RuntimeError {}
