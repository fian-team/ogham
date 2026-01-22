//! High-level runtime API for integrating Ogham into Rust applications.
//!
//! This module provides a plug-and-play solution for executing Ogham source code
//! and converting it into executable UI components. It handles the full pipeline:
//! scanner -> parser -> Runtime -> UI bridge.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::Arc;
use std::collections::HashMap;
use std::sync::Mutex;

use notify::{Event as NotifyEvent, EventKind, RecommendedWatcher, RecursiveMode, Watcher};

use crate::parser::{Parser, SyntaxError, Block, Statement, Expression, Literal, Operator, Call, Function, Identifier};
use crate::scanner::Scanner;
use crate::tree::{ast_bridge, UI};

// Core runtime types (previously in vm module)

/// A widget value produced by the Runtime. Unlike the parser's `Widget`, all properties
/// are evaluated to runtime `Value`s at the time the widget expression is evaluated.
#[derive(Clone, Debug, PartialEq)]
pub struct RuntimeWidget {
    pub identifier: Identifier,
    pub properties: HashMap<String, Value>,
}

/// Runtime value types that can be stored and manipulated during execution
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Integer(i32),
    Float(f64),
    Boolean(bool),
    String(String),
    Function(Function),
    Map(HashMap<String, Value>),
    Array(Vec<Value>),
    Widget(RuntimeWidget),
    Void,
}

/// Environment for storing variables during execution
#[derive(Clone, Debug)]
pub struct Environment {
    variables: HashMap<String, Value>,
    parent: Option<Box<Environment>>,
}

impl Environment {
    pub fn new() -> Self {
        Self {
            variables: HashMap::new(),
            parent: None,
        }
    }

    pub fn new_with_parent(parent: Environment) -> Self {
        Self {
            variables: HashMap::new(),
            parent: Some(Box::new(parent)),
        }
    }

    pub fn define(&mut self, name: String, value: Value) {
        self.variables.insert(name, value);
    }

    pub fn get(&self, name: &str) -> Option<Value> {
        if let Some(value) = self.variables.get(name) {
            Some(value.clone())
        } else if let Some(parent) = &self.parent {
            parent.get(name)
        } else {
            None
        }
    }

    pub fn assign(&mut self, name: &str, value: Value) -> bool {
        if self.variables.contains_key(name) {
            self.variables.insert(name.to_string(), value);
            true
        } else if let Some(parent) = &mut self.parent {
            parent.assign(name, value)
        } else {
            false
        }
    }
}

/// Runtime error types for execution errors
#[derive(Debug)]
pub enum VMError {
    UndefinedVariable(String),
    TypeMismatch(String),
    InvalidOperation(String),
    Return(Value),
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

/// Configuration for runtime execution, allowing customization of the execution process.
///
/// This struct provides hooks for host application integration including
/// host state injection and event handling.
#[derive(Clone, Default)]
pub struct RuntimeConfig {
    /// Optional host state that can be accessed via a special keyword (e.g., `global` or `host`).
    /// This is separate from environment-scoped variables and `state` keyword values.
    ///
    /// Host state is global and persists across function calls, allowing the host application
    /// to provide data that the Ogham script can access but not modify.
    pub host_state: Option<std::collections::HashMap<String, Value>>,

    /// Per-event handlers for `event("name", arg1, arg2, ...)` emitted from Ogham.
    ///
    /// Only events present in this map will be dispatched.
    pub event_handlers: HashMap<String, Arc<dyn Fn(&[Value]) -> bool + Send + Sync>>,
}

impl RuntimeConfig {
    /// Create a new default runtime configuration.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set host state that can be accessed via a special keyword.
    /// This state is separate from environment variables and `state` keyword values.
    pub fn with_host_state(mut self, state: std::collections::HashMap<String, Value>) -> Self {
        self.host_state = Some(state);
        self
    }

    /// Register an event handler for `event("name", ...)`.
    ///
    /// If the same event name is registered multiple times, the most recent handler wins.
    pub fn with_event_handler<S, F>(mut self, name: S, handler: F) -> Self
    where
        S: Into<String>,
        F: Fn(&[Value]) -> bool + Send + Sync + 'static,
    {
        self.event_handlers.insert(name.into(), Arc::new(handler));
        self
    }
}

/// Runtime for executing parsed AST and managing execution state.
///
/// This struct combines the functionality of the virtual machine with high-level
/// runtime operations. It handles execution of Ogham code, manages variables,
/// host state, and event handlers.
pub struct Runtime {
    environment: Environment,
    host_state: HashMap<String, Value>,
    event_handlers: HashMap<String, Arc<dyn Fn(&[Value]) -> bool + Send + Sync>>,
}

impl Runtime {
    /// Create a new runtime instance.
    pub fn new() -> Self {
        Self {
            environment: Environment::new(),
            host_state: HashMap::new(),
            event_handlers: HashMap::new(),
        }
    }

    /// Inject host state that can be accessed by the Ogham script.
    ///
    /// Host state is separate from the execution environment and persists
    /// across function calls. This allows the host application to provide
    /// data that the script can read but not modify.
    ///
    /// # Arguments
    ///
    /// * `name` - The name of the variable to inject
    /// * `value` - The value to inject
    ///
    /// # Example
    ///
    /// ```
    /// use ogham::runtime::Runtime;
    /// use ogham::runtime::Value;
    ///
    /// let mut runtime = Runtime::new();
    /// runtime.inject_host_state("user_name".to_string(), Value::String("Alice".to_string()));
    /// ```
    pub fn inject_host_state(&mut self, name: String, value: Value) {
        self.host_state.insert(name, value);
    }

    /// Get host state value by name.
    ///
    /// This is used internally when the script accesses host state.
    /// Host state is checked after the environment when resolving identifiers.
    ///
    /// # Arguments
    ///
    /// * `name` - The name of the variable to retrieve
    ///
    /// # Returns
    ///
    /// Returns `Some(Value)` if the host state exists, `None` otherwise.
    pub fn get_host_state(&self, name: &str) -> Option<Value> {
        self.host_state.get(name).cloned()
    }

    /// Register a handler for an event name that can be emitted from Ogham via `event("name", ...)`.
    ///
    /// If the same event name is registered multiple times, the most recent handler wins.
    pub fn register_event_handler<S, F>(&mut self, name: S, handler: F)
    where
        S: Into<String>,
        F: Fn(&[Value]) -> bool + Send + Sync + 'static,
    {
        self.event_handlers.insert(name.into(), Arc::new(handler));
    }

    /// Register a handler for an event name that can be emitted from Ogham via `event("name", ...)`.
    ///
    /// If the same event name is registered multiple times, the most recent handler wins.
    pub fn register_event_handler_arc(
        &mut self,
        name: String,
        handler: Arc<dyn Fn(&[Value]) -> bool + Send + Sync>,
    ) {
        self.event_handlers.insert(name, handler);
    }

    /// Emit an event to the host application if a handler is registered for it.
    ///
    /// Returns `true` if an event handler was invoked and returned `true`.
    pub fn emit_event(&self, name: &str, args: &[Value]) -> bool {
        self.event_handlers
            .get(name)
            .map(|handler| handler(args))
            .unwrap_or(false)
    }

    /// Execute a module (Function) and look for a main function to call
    pub fn execute_module(&mut self, module: &Function) -> Result<Value, VMError> {
        // Execute the module body to populate the environment
        self.execute_block(&module.body)?;

        // Look for a 'main' variable that is a function
        if let Some(Value::Function(main_func)) = self.environment.get("main") {
            // Call the main function
            self.call_function(&main_func, &[])
        } else {
            Ok(Value::Void)
        }
    }

    /// Execute a block of statements
    fn execute_block(&mut self, block: &Block) -> Result<Value, VMError> {
        for statement in &block.statement_list {
            match self.execute_statement(statement)? {
                Value::Void => continue,
                value => return Ok(value),
            }
        }
        Ok(Value::Void)
    }

    /// Execute a statement
    fn execute_statement(&mut self, statement: &Statement) -> Result<Value, VMError> {
        match statement {
            Statement::Expression(expr_stmt) => {
                self.evaluate_expression(&expr_stmt.get_value())?;
                Ok(Value::Void)
            }
            Statement::Declare(declare_stmt) => {
                let name = declare_stmt.get_identifier_value();
                let value = self.evaluate_expression(&declare_stmt.get_value())?;
                self.environment.define(name, value);
                Ok(Value::Void)
            }
            Statement::DeclareState(state_stmt) => {
                // State is treated the same as regular variables for now
                let name = state_stmt.get_identifier_value();
                let value = self.evaluate_expression(&state_stmt.get_value())?;
                self.environment.define(name, value);
                Ok(Value::Void)
            }
            Statement::Assign(assign_stmt) => {
                let name = assign_stmt.get_identifier_value();
                let value = self.evaluate_expression(&assign_stmt.get_value())?;
                if !self.environment.assign(&name, value.clone()) {
                    return Err(VMError::UndefinedVariable(name));
                }
                Ok(Value::Void)
            }
            Statement::Return(return_stmt) => {
                if let Some(expr) = return_stmt.get_value() {
                    let value = self.evaluate_expression(&expr)?;
                    Err(VMError::Return(value))
                } else {
                    Err(VMError::Return(Value::Void))
                }
            }
            Statement::Conditional(cond_stmt) => {
                let condition = self.evaluate_expression(&cond_stmt.get_condition())?;
                if let Value::Boolean(true) = condition {
                    self.execute_block(&cond_stmt.get_block())
                } else {
                    Ok(Value::Void)
                }
            }
            Statement::Log(log_stmt) => {
                let _value = self.evaluate_expression(&log_stmt.get_value())?;
                Ok(Value::Void)
            }
            Statement::Event(_) => {
                // Events are not executed by the runtime
                Ok(Value::Void)
            }
        }
    }

    /// Evaluate an expression to a value
    pub fn evaluate_expression(&mut self, expression: &Expression) -> Result<Value, VMError> {
        match expression {
            Expression::Literal(literal) => self.evaluate_literal(literal),
            Expression::Unary(unary) => {
                let value = self.evaluate_expression(&unary.value)?;
                // Unary operations (currently not fully implemented in parser)
                Ok(value)
            }
            Expression::Binary(binary) => {
                let left = self.evaluate_expression(&binary.left)?;
                let right = self.evaluate_expression(&binary.right)?;
                self.evaluate_binary_operation(&left, &binary.operator, &right)
            }
            Expression::Grouping(grouping) => self.evaluate_expression(&grouping.value),
            Expression::MemberAccess(access) => {
                let object = self.evaluate_expression(&access.object)?;
                let key = access.property.get();
                match object {
                    Value::Map(map) => map.get(&key).cloned().ok_or_else(|| {
                        VMError::InvalidOperation(format!("Map has no property '{}'", key))
                    }),
                    Value::Widget(widget) => widget.properties.get(&key).cloned().ok_or_else(|| {
                        VMError::InvalidOperation(format!("Widget has no property '{}'", key))
                    }),
                    other => Err(VMError::TypeMismatch(format!(
                        "Cannot access property '{}' on {:?}",
                        key, other
                    ))),
                }
            }
            Expression::Widget(widget) => {
                // Evaluate widget properties now (while variables are still in scope) so
                // widget values do not depend on later environment lookups.
                let mut evaluated_props: HashMap<String, Value> = HashMap::new();
                for (key, expr) in &widget.properties {
                    let value = self.evaluate_expression(expr)?;
                    evaluated_props.insert(key.clone(), value);
                }
                Ok(Value::Widget(RuntimeWidget {
                    identifier: widget.identifier.clone(),
                    properties: evaluated_props,
                }))
            }
        }
    }

    /// Evaluate a literal
    fn evaluate_literal(&mut self, literal: &Literal) -> Result<Value, VMError> {
        match literal {
            Literal::Integer(i) => Ok(Value::Integer(*i)),
            Literal::Float(f) => Ok(Value::Float(*f)),
            Literal::Boolean(b) => Ok(Value::Boolean(*b)),
            Literal::String(s) => Ok(Value::String(s.clone())),
            Literal::Identifier(ident) => {
                let name = ident.get();
                // First check environment, then host state
                if let Some(value) = self.environment.get(&name) {
                    Ok(value)
                } else if let Some(value) = self.get_host_state(&name) {
                    Ok(value)
                } else {
                    Err(VMError::UndefinedVariable(name))
                }
            }
            Literal::Call(call) => self.execute_call(call),
            Literal::Function(func) => Ok(Value::Function(func.clone())),
            Literal::Map(map) => {
                let mut value_map = HashMap::new();
                for (key, expr) in &map.properties {
                    let value = self.evaluate_expression(expr)?;
                    value_map.insert(key.clone(), value);
                }
                Ok(Value::Map(value_map))
            }
            Literal::Array(array) => {
                let mut value_array = Vec::new();
                for expr in &array.elements {
                    let value = self.evaluate_expression(expr)?;
                    value_array.push(value);
                }
                Ok(Value::Array(value_array))
            }
        }
    }

    /// Evaluate a binary operation
    fn evaluate_binary_operation(
        &mut self,
        left: &Value,
        operator: &Operator,
        right: &Value,
    ) -> Result<Value, VMError> {
        match operator {
            Operator::Plus => self.add(left, right),
            Operator::Minus => self.subtract(left, right),
            Operator::Multiply => self.multiply(left, right),
            Operator::Divide => self.divide(left, right),
            Operator::Equals => Ok(Value::Boolean(left == right)),
            Operator::NotEquals => Ok(Value::Boolean(left != right)),
            Operator::GreaterThan => self.compare_greater_than(left, right),
            Operator::GreaterThanOrEqualTo => self.compare_greater_than_or_equal(left, right),
            Operator::LessThan => self.compare_less_than(left, right),
            Operator::LessThanOrEqualTo => self.compare_less_than_or_equal(left, right),
            Operator::Not => Err(VMError::InvalidOperation(
                "Not operator is not a binary operator".to_string(),
            )),
        }
    }

    fn add(&self, left: &Value, right: &Value) -> Result<Value, VMError> {
        match (left, right) {
            (Value::Integer(a), Value::Integer(b)) => Ok(Value::Integer(a + b)),
            (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a + b)),
            (Value::Integer(a), Value::Float(b)) => Ok(Value::Float(*a as f64 + b)),
            (Value::Float(a), Value::Integer(b)) => Ok(Value::Float(a + *b as f64)),
            (Value::String(a), Value::String(b)) => Ok(Value::String(format!("{}{}", a, b))),
            (Value::String(a), b) => Ok(Value::String(format!("{}{:?}", a, b))),
            (a, Value::String(b)) => Ok(Value::String(format!("{:?}{}", a, b))),
            _ => Err(VMError::TypeMismatch(format!(
                "Cannot add {:?} and {:?}",
                left, right
            ))),
        }
    }

    fn subtract(&self, left: &Value, right: &Value) -> Result<Value, VMError> {
        match (left, right) {
            (Value::Integer(a), Value::Integer(b)) => Ok(Value::Integer(a - b)),
            (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a - b)),
            (Value::Integer(a), Value::Float(b)) => Ok(Value::Float(*a as f64 - b)),
            (Value::Float(a), Value::Integer(b)) => Ok(Value::Float(a - *b as f64)),
            _ => Err(VMError::TypeMismatch(format!(
                "Cannot subtract {:?} from {:?}",
                right, left
            ))),
        }
    }

    fn multiply(&self, left: &Value, right: &Value) -> Result<Value, VMError> {
        match (left, right) {
            (Value::Integer(a), Value::Integer(b)) => Ok(Value::Integer(a * b)),
            (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a * b)),
            (Value::Integer(a), Value::Float(b)) => Ok(Value::Float(*a as f64 * b)),
            (Value::Float(a), Value::Integer(b)) => Ok(Value::Float(a * *b as f64)),
            _ => Err(VMError::TypeMismatch(format!(
                "Cannot multiply {:?} and {:?}",
                left, right
            ))),
        }
    }

    fn divide(&self, left: &Value, right: &Value) -> Result<Value, VMError> {
        match (left, right) {
            (Value::Integer(a), Value::Integer(b)) => {
                if *b == 0 {
                    Err(VMError::InvalidOperation("Division by zero".to_string()))
                } else {
                    Ok(Value::Float(*a as f64 / *b as f64))
                }
            }
            (Value::Float(a), Value::Float(b)) => {
                if *b == 0.0 {
                    Err(VMError::InvalidOperation("Division by zero".to_string()))
                } else {
                    Ok(Value::Float(a / b))
                }
            }
            (Value::Integer(a), Value::Float(b)) => {
                if *b == 0.0 {
                    Err(VMError::InvalidOperation("Division by zero".to_string()))
                } else {
                    Ok(Value::Float(*a as f64 / b))
                }
            }
            (Value::Float(a), Value::Integer(b)) => {
                if *b == 0 {
                    Err(VMError::InvalidOperation("Division by zero".to_string()))
                } else {
                    Ok(Value::Float(a / *b as f64))
                }
            }
            _ => Err(VMError::TypeMismatch(format!(
                "Cannot divide {:?} by {:?}",
                left, right
            ))),
        }
    }

    fn compare_greater_than(&self, left: &Value, right: &Value) -> Result<Value, VMError> {
        match (left, right) {
            (Value::Integer(a), Value::Integer(b)) => Ok(Value::Boolean(a > b)),
            (Value::Float(a), Value::Float(b)) => Ok(Value::Boolean(a > b)),
            (Value::Integer(a), Value::Float(b)) => Ok(Value::Boolean((*a as f64) > *b)),
            (Value::Float(a), Value::Integer(b)) => Ok(Value::Boolean(*a > (*b as f64))),
            _ => Err(VMError::TypeMismatch(format!(
                "Cannot compare {:?} > {:?}",
                left, right
            ))),
        }
    }

    fn compare_greater_than_or_equal(&self, left: &Value, right: &Value) -> Result<Value, VMError> {
        match (left, right) {
            (Value::Integer(a), Value::Integer(b)) => Ok(Value::Boolean(a >= b)),
            (Value::Float(a), Value::Float(b)) => Ok(Value::Boolean(a >= b)),
            (Value::Integer(a), Value::Float(b)) => Ok(Value::Boolean((*a as f64) >= *b)),
            (Value::Float(a), Value::Integer(b)) => Ok(Value::Boolean(*a >= (*b as f64))),
            _ => Err(VMError::TypeMismatch(format!(
                "Cannot compare {:?} >= {:?}",
                left, right
            ))),
        }
    }

    fn compare_less_than(&self, left: &Value, right: &Value) -> Result<Value, VMError> {
        match (left, right) {
            (Value::Integer(a), Value::Integer(b)) => Ok(Value::Boolean(a < b)),
            (Value::Float(a), Value::Float(b)) => Ok(Value::Boolean(a < b)),
            (Value::Integer(a), Value::Float(b)) => Ok(Value::Boolean((*a as f64) < *b)),
            (Value::Float(a), Value::Integer(b)) => Ok(Value::Boolean(*a < (*b as f64))),
            _ => Err(VMError::TypeMismatch(format!(
                "Cannot compare {:?} < {:?}",
                left, right
            ))),
        }
    }

    fn compare_less_than_or_equal(&self, left: &Value, right: &Value) -> Result<Value, VMError> {
        match (left, right) {
            (Value::Integer(a), Value::Integer(b)) => Ok(Value::Boolean(a <= b)),
            (Value::Float(a), Value::Float(b)) => Ok(Value::Boolean(a <= b)),
            (Value::Integer(a), Value::Float(b)) => Ok(Value::Boolean((*a as f64) <= *b)),
            (Value::Float(a), Value::Integer(b)) => Ok(Value::Boolean(*a <= (*b as f64))),
            _ => Err(VMError::TypeMismatch(format!(
                "Cannot compare {:?} <= {:?}",
                left, right
            ))),
        }
    }

    /// Execute a function call
    fn execute_call(&mut self, call: &Call) -> Result<Value, VMError> {
        // Evaluate arguments
        let mut args = Vec::new();
        for arg_expr in &call.arguments {
            args.push(self.evaluate_expression(arg_expr)?);
        }

        // Look up the function
        let func_name = call.identifier.get();
        if func_name == "event" {
            if args.is_empty() {
                return Err(VMError::InvalidOperation(
                    "event() requires at least an event name".to_string(),
                ));
            }
            let event_name = match &args[0] {
                Value::String(s) => s.clone(),
                other => {
                    return Err(VMError::TypeMismatch(format!(
                        "event() requires a string event name as the first argument, got {:?}",
                        other
                    )));
                }
            };

            // Only dispatch to the host if the event name was registered.
            let _handled = self.emit_event(&event_name, &args[1..]);
            return Ok(Value::Void);
        }
        if let Some(Value::Function(func)) = self.environment.get(&func_name) {
            self.call_function(&func, &args)
        } else {
            Err(VMError::UndefinedVariable(format!(
                "Function '{}' not found",
                func_name
            )))
        }
    }

    /// Call a function with arguments.
    ///
    /// This is used internally by the runtime and also by the UI bridge when wiring
    /// Ogham functions to widget event listeners (e.g. `mouse_down` handlers).
    pub fn call_function(&mut self, func: &Function, args: &[Value]) -> Result<Value, VMError> {
        // Check argument count
        if args.len() != func.arguments.len() {
            return Err(VMError::InvalidOperation(format!(
                "Expected {} arguments, got {}",
                func.arguments.len(),
                args.len()
            )));
        }

        // Create new environment with function arguments
        let mut func_env = Environment::new_with_parent(self.environment.clone());
        for (param, arg_value) in func.arguments.iter().zip(args.iter()) {
            func_env.define(param.get(), arg_value.clone());
        }

        // Save current environment and switch to function environment
        let old_env = std::mem::replace(&mut self.environment, func_env);

        // Execute function body
        let result = match self.execute_block(&func.body) {
            Ok(Value::Void) => Ok(Value::Void),
            Err(VMError::Return(value)) => Ok(value),
            Err(e) => Err(e),
            Ok(value) => Ok(value),
        };

        // Restore environment
        self.environment = old_env;

        result
    }
}

impl Default for Runtime {
    fn default() -> Self {
        Self::new()
    }
}

/// Compile an Ogham source file into a UI.
///
/// This function handles the complete compilation pipeline:
/// 1. Read the source file
/// 2. Scan the source into tokens
/// 3. Parse tokens into an AST
/// 4. Execute the AST in the Runtime
/// 5. Convert the result to a UI widget tree
///
/// # Arguments
///
/// * `path` - Path to the Ogham source file (`.ogh` extension)
/// * `config` - Optional runtime configuration
///
/// # Returns
///
/// Returns a `UI` instance on success, or a `RuntimeError` if any stage fails.
///
/// # Example
///
/// ```no_run
/// use ogham::runtime;
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
///
/// let ui = runtime::from_file("ui.ogh", None)?;
/// # Ok(())
/// # }
/// ```
pub fn from_file<P: AsRef<Path>>(
    path: P,
    config: Option<RuntimeConfig>,
) -> Result<UI, RuntimeError> {
    let source = fs::read_to_string(path)?;
    from_source(&source, config)
}

/// Compile Ogham source code from a string into a UI.
///
/// This function handles the complete compilation pipeline:
/// 1. Scan the source into tokens
/// 2. Parse tokens into an AST
/// 3. Execute the AST in the Runtime
/// 4. Convert the result to a UI widget tree
///
/// # Arguments
///
/// * `source` - The Ogham source code as a string
/// * `config` - Optional runtime configuration
///
/// # Returns
///
/// Returns a `UI` instance on success, or a `RuntimeError` if any stage fails.
///
/// # Example
///
/// ```no_run
/// use ogham::runtime;
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
///
/// let source = r#"
///     fn main() {
///         return flex {
///             children: [text { text: "Hello, World!" }]
///         }
///     }
/// "#;
///
/// let ui = runtime::from_source(source, None)?;
/// # Ok(())
/// # }
/// ```
pub fn from_source(source: &str, config: Option<RuntimeConfig>) -> Result<UI, RuntimeError> {
    // Step 1: Scan source into tokens
    let mut scanner = Scanner::new(source.to_string());
    let tokens = scanner.scan();

    // Step 2: Parse tokens into AST
    let mut parser = Parser::new(tokens);
    let module = parser.parse()?;

    // Step 3: Execute in Runtime (kept alive for UI event handlers)
    let runtime = Arc::new(Mutex::new(Runtime::new()));

    // Inject host state if provided
    if let Some(config) = config.as_ref() {
        if let Some(ref state) = config.host_state.as_ref() {
            for (name, value) in state.iter() {
                runtime.lock()
                    .unwrap()
                    .inject_host_state(name.clone(), value.clone());
            }
        }

        // Register per-event handlers (for `event("name", ...)`).
        for (name, handler) in config.event_handlers.iter() {
            runtime.lock()
                .unwrap()
                .register_event_handler_arc(name.clone(), handler.clone());
        }
    }

    let value = runtime.lock().unwrap().execute_module(&module)?;

    // Step 4: Convert Runtime value to UI widget
    let widget = ast_bridge::widget_value_to_widget_ref(&runtime, &value)?;

    // Step 5: Create UI
    Ok(UI::new(widget))
}

/// File watcher for monitoring Ogham source files for changes.
///
/// This struct wraps the underlying file system watcher and provides
/// a simple API for watching a file and receiving change notifications.
pub struct FileWatcher {
    _watcher: RecommendedWatcher,
    receiver: mpsc::Receiver<Result<NotifyEvent, notify::Error>>,
    watched_path: PathBuf,
}

impl FileWatcher {
    /// Create a new file watcher for the specified file.
    ///
    /// # Arguments
    ///
    /// * `path` - Path to the file to watch
    ///
    /// # Returns
    ///
    /// Returns a `FileWatcher` on success, or a `RuntimeError` if the watcher
    /// could not be created or the file path is invalid.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use ogham::runtime;
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    ///
    /// let watcher = runtime::FileWatcher::new("ui.ogh")?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self, RuntimeError> {
        let path_buf = PathBuf::from(path.as_ref());

        // Verify the file exists
        if !path_buf.exists() {
            return Err(RuntimeError::IoError(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("File not found: {}", path_buf.display()),
            )));
        }

        let (tx, rx) = mpsc::channel();
        let mut watcher = notify::recommended_watcher(tx).map_err(|e| {
            RuntimeError::IoError(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("Failed to create file watcher: {}", e),
            ))
        })?;

        // Watch the parent directory (non-recursive) to detect changes to the file
        if let Some(parent) = path_buf.parent() {
            watcher
                .watch(parent, RecursiveMode::NonRecursive)
                .map_err(|e| {
                    RuntimeError::IoError(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        format!("Failed to watch directory: {}", e),
                    ))
                })?;
        }

        Ok(Self {
            _watcher: watcher,
            receiver: rx,
            watched_path: path_buf,
        })
    }

    /// Check if the watched file has changed.
    ///
    /// This method should be called periodically (e.g., in an event loop)
    /// to check for file changes. It returns `true` if the watched file
    /// was modified or created.
    ///
    /// # Returns
    ///
    /// Returns `true` if the watched file changed, `false` otherwise.
    /// Errors from the underlying watcher are logged but don't cause this
    /// method to return an error.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use ogham::runtime;
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    ///
    /// let mut watcher = runtime::FileWatcher::new("ui.ogh")?;
    /// let mut ui = runtime::from_file("ui.ogh", None)?;
    ///
    /// // In your event loop:
    /// if watcher.check_for_changes() {
    ///     // File changed, recompile
    ///     ui = runtime::from_file("ui.ogh", None)?;
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub fn check_for_changes(&self) -> bool {
        // Try to receive all pending events
        let mut file_changed = false;

        while let Ok(Ok(event)) = self.receiver.try_recv() {
            match event.kind {
                EventKind::Modify(_) | EventKind::Create(_) => {
                    // Check if the changed file matches our watched file
                    if event.paths.iter().any(|p| p == &self.watched_path) {
                        file_changed = true;
                    }
                }
                _ => {}
            }
        }

        file_changed
    }

    /// Get a reference to the watched file path.
    pub fn path(&self) -> &Path {
        &self.watched_path
    }

    /// Recompile the watched file with the given configuration.
    ///
    /// This is a convenience method that reads the file and compiles it.
    ///
    /// # Arguments
    ///
    /// * `config` - Optional runtime configuration
    ///
    /// # Returns
    ///
    /// Returns a new `UI` instance on success, or a `RuntimeError` if compilation fails.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use ogham::runtime;
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    ///
    /// let mut watcher = runtime::FileWatcher::new("ui.ogh")?;
    /// let mut ui = runtime::from_file("ui.ogh", None)?;
    ///
    /// // In your event loop:
    /// if watcher.check_for_changes() {
    ///     ui = watcher.recompile(None)?;
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub fn recompile(&self, config: Option<RuntimeConfig>) -> Result<UI, RuntimeError> {
        from_file(&self.watched_path, config)
    }
}

/// Create a file watcher and compile the file in one step.
///
/// This is a convenience function that creates a watcher and compiles
/// the file, returning both the UI and the watcher.
///
/// # Arguments
///
/// * `path` - Path to the Ogham source file to watch and compile
/// * `config` - Optional runtime configuration
///
/// # Returns
///
/// Returns a tuple of `(UI, FileWatcher)` on success, or a `RuntimeError` if
/// compilation or watcher creation fails.
///
/// # Example
///
/// ```no_run
/// use ogham::runtime;
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
///
/// let (mut ui, mut watcher) = runtime::watch_and_compile("ui.ogh", None)?;
///
/// // In your event loop:
/// if watcher.check_for_changes() {
///     ui = watcher.recompile(None)?;
/// }
/// # Ok(())
/// # }
/// ```
pub fn watch_and_compile<P: AsRef<Path>>(
    path: P,
    config: Option<RuntimeConfig>,
) -> Result<(UI, FileWatcher), RuntimeError> {
    let watcher = FileWatcher::new(&path)?;
    let ui = from_file(&path, config)?;
    Ok((ui, watcher))
}
