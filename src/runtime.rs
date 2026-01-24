//! High-level runtime API for integrating Ogham into Rust applications.
//!
//! This module provides a plug-and-play solution for executing Ogham source code
//! and converting it into executable UI components. It handles the full pipeline:
//! scanner -> parser -> Runtime -> UI bridge.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::Arc;

use notify::{Event as NotifyEvent, EventKind, RecommendedWatcher, RecursiveMode, Watcher};

use crate::parser::{
    Block, Call, Expression, Function, Identifier, Literal, Operator, Parser, Statement,
    SyntaxError,
};
use crate::scanner::Scanner;
use crate::tree::ast_bridge;

// Core runtime types (previously in vm module)

/// A widget value produced by the Runtime. Unlike the parser's `Widget`, all properties
/// are evaluated to runtime `Value`s at the time the widget expression is evaluated.
#[derive(Clone, Debug, PartialEq)]
pub struct RuntimeWidget {
    pub identifier: Identifier,
    pub properties: HashMap<String, Value>,
}

/// A closure that captures both a function and its lexical environment.
/// This allows functions to access variables from their enclosing scope.
#[derive(Clone, Debug)]
pub struct Closure {
    pub function: Function,
    pub captured_env: Environment,
}

impl PartialEq for Closure {
    fn eq(&self, other: &Self) -> bool {
        // Compare functions for equality (environments may differ but that's okay for comparison)
        self.function == other.function
    }
}

/// Runtime value types that can be stored and manipulated during execution
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Integer(i32),
    Float(f64),
    Boolean(bool),
    String(String),
    Closure(Closure),
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

/// A node in the call tree representing a function call.
/// Each node tracks the function being called, its position in the parent's child list,
/// and stores state declared during that call.
#[derive(Clone, Debug)]
struct CallTreeNode {
    /// Name of the function being called (or "module" for module-level execution)
    function_name: String,
    /// Index of this call in the parent's children list
    call_index: usize,
    /// Child function calls made from this call
    children: Vec<CallTreeNode>,
    /// State variables declared in this call, stored in declaration order
    state: HashMap<String, Value>,
    /// Counter for tracking the order of state declarations
    state_counter: usize,
}

impl CallTreeNode {
    fn new(function_name: String, call_index: usize) -> Self {
        Self {
            function_name,
            call_index,
            children: Vec::new(),
            state: HashMap::new(),
            state_counter: 0,
        }
    }

    /// Get or create a child node for a function call.
    /// If a child at the given index doesn't exist, creates it.
    /// This ensures that sequential calls to the same function create sequential child nodes.
    fn get_or_create_child(
        &mut self,
        function_name: String,
        call_index: usize,
    ) -> &mut CallTreeNode {
        // Ensure we have enough children
        while self.children.len() <= call_index {
            self.children.push(CallTreeNode::new(
                function_name.clone(),
                self.children.len(),
            ));
        }
        &mut self.children[call_index]
    }

    /// Set state value for this call node.
    fn set_state(&mut self, name: String, value: Value) {
        self.state.insert(name, value);
    }

    /// Get state value from this call node.
    fn get_state(&self, name: &str) -> Option<&Value> {
        self.state.get(name)
    }
}

/// Call tree that tracks function call stacks and maps them to persistent state.
/// This follows React's useState pattern where each sequential function call
/// is added as a child in the parent's children list.
#[derive(Clone, Debug)]
struct CallTree {
    /// Root node representing module-level execution
    root: CallTreeNode,
    /// Current path through the tree (indices into children vectors)
    /// This represents the call stack
    current_path: Vec<usize>,
    /// Track the call sequence for rerendering (to reuse nodes in order)
    /// Each entry is (path, function_name) representing a function call
    call_sequence: Vec<(Vec<usize>, String)>,
    /// Flag indicating if we're currently rerendering
    is_rerendering: bool,
    /// Counter for tracking call order during rerender
    rerender_call_index: usize,
}

impl CallTree {
    fn new() -> Self {
        Self {
            root: CallTreeNode::new("module".to_string(), 0),
            current_path: Vec::new(),
            call_sequence: Vec::new(),
            is_rerendering: false,
            rerender_call_index: 0,
        }
    }

    /// Start a rerender cycle - this resets the path and enables node reuse
    fn start_rerender(&mut self) {
        self.current_path.clear();
        self.is_rerendering = true;
        self.rerender_call_index = 0;
    }

    /// End a rerender cycle
    fn end_rerender(&mut self) {
        self.is_rerendering = false;
        self.rerender_call_index = 0;
    }

    /// Enter a function call, creating or reusing the appropriate node in the tree.
    /// When rerendering, this will reuse existing nodes at the same indices to preserve state.
    /// Returns the index of the call in the parent's children list.
    fn enter_call(&mut self, function_name: String) -> usize {
        let mut current = &mut self.root;

        // Navigate to the current node based on the path
        for &index in &self.current_path {
            // Ensure we have enough children
            while current.children.len() <= index {
                current.children.push(CallTreeNode::new(
                    function_name.clone(),
                    current.children.len(),
                ));
            }
            current = &mut current.children[index];
        }

        let call_index = if self.is_rerendering {
            // When rerendering, reuse nodes in the same order as the first execution
            // Use the call sequence to determine which node to reuse
            if self.rerender_call_index < self.call_sequence.len() {
                // Reuse the node from the sequence
                let (path, _) = &self.call_sequence[self.rerender_call_index];
                // Verify the path matches up to the current point
                if path.len() > self.current_path.len()
                    && path[..self.current_path.len()] == self.current_path[..]
                {
                    // The next index in the path is the call index we should reuse
                    let reuse_index = path[self.current_path.len()];
                    self.rerender_call_index += 1;
                    reuse_index
                } else {
                    // Path doesn't match - fallback to first available
                    self.rerender_call_index += 1;
                    if !current.children.is_empty() {
                        0
                    } else {
                        current.children.len()
                    }
                }
            } else {
                // Sequence not long enough - use first available or create new
                self.rerender_call_index += 1;
                if !current.children.is_empty() {
                    0
                } else {
                    current.children.len()
                }
            }
        } else {
            // First execution - always create new node and record in sequence
            let new_index = current.children.len();
            let mut path = self.current_path.clone();
            path.push(new_index);
            self.call_sequence.push((path, function_name.clone()));
            new_index
        };

        // Create the new child node if it doesn't exist
        while current.children.len() <= call_index {
            current.children.push(CallTreeNode::new(
                function_name.clone(),
                current.children.len(),
            ));
        }

        // Update path to point to the node (reusing existing node if rerendering)
        self.current_path.push(call_index);

        call_index
    }

    /// Exit the current function call, moving back up the tree.
    fn exit_call(&mut self) {
        if !self.current_path.is_empty() {
            self.current_path.pop();
        }
    }

    /// Recursively collect all state from the call tree into a map.
    /// Later state declarations override earlier ones (child nodes override parent nodes).
    fn collect_state_from_tree(node: &CallTreeNode, state_map: &mut HashMap<String, Value>) {
        // Add state from current node (this will override parent state if there are conflicts)
        for (name, value) in &node.state {
            state_map.insert(name.clone(), value.clone());
        }

        // Recursively collect from children
        for child in &node.children {
            Self::collect_state_from_tree(child, state_map);
        }
    }

    /// Get the current node in the call tree.
    fn get_current_node(&mut self) -> &mut CallTreeNode {
        let mut current = &mut self.root;
        for &index in &self.current_path {
            current = &mut current.children[index];
        }
        current
    }

    /// Set state in the call tree node where it was originally declared.
    /// This searches the tree to find the node containing the state variable
    /// and updates it there, allowing state to be updated from child function calls.
    fn set_state(&mut self, name: String, value: Value) {
        // Find the node that contains this state variable and update it
        if Self::find_and_set_state_in_tree(&mut self.root, &name, &value) {
            return;
        }

        // If state doesn't exist anywhere, create it in the current node
        let node = self.get_current_node();
        node.set_state(name, value);
    }

    /// Recursively search the call tree for a node containing the state variable and update it.
    /// Returns true if the state was found and updated, false otherwise.
    fn find_and_set_state_in_tree(node: &mut CallTreeNode, name: &str, value: &Value) -> bool {
        // Check current node
        if node.get_state(name).is_some() {
            node.set_state(name.to_string(), value.clone());
            return true;
        }

        // Recursively search children
        for child in &mut node.children {
            if Self::find_and_set_state_in_tree(child, name, value) {
                return true;
            }
        }

        false
    }

    /// Get state from the current call node or any parent node.
    /// This allows child function calls (like event handlers) to access
    /// state declared in their parent function calls.
    ///
    /// Since event handlers may be called in a different call tree context
    /// than where they were created, we search all nodes in the call tree
    /// to find the state. In practice, state should be unique per function
    /// call, so we return the first match found.
    fn get_state(&self, name: &str) -> Option<Value> {
        // First, try the current path (most specific)
        let mut current = &self.root;
        for &index in &self.current_path {
            if index >= current.children.len() {
                break;
            }
            current = &current.children[index];
            if let Some(value) = current.get_state(name) {
                return Some(value.clone());
            }
        }

        // If not found in current path, search all nodes in the tree
        // This handles the case where event handlers are called in a
        // different context than where they were created
        self.search_state_in_tree(&self.root, name)
    }

    /// Recursively search the call tree for state with the given name.
    fn search_state_in_tree(&self, node: &CallTreeNode, name: &str) -> Option<Value> {
        // Check current node
        if let Some(value) = node.get_state(name) {
            return Some(value.clone());
        }

        // Recursively search children
        for child in &node.children {
            if let Some(value) = self.search_state_in_tree(child, name) {
                return Some(value);
            }
        }

        None
    }
}

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
    /// Call tree tracking function call stacks and mapping them to persistent state
    call_tree: CallTree,
    /// Flag indicating that state has been updated and a rerender is needed
    needs_rerender: bool,
    /// The parsed module AST (stored for rerendering)
    module: Option<Function>,
}

impl Runtime {
    /// Create a new runtime instance.
    pub fn new() -> Self {
        Self {
            environment: Environment::new(),
            host_state: HashMap::new(),
            event_handlers: HashMap::new(),
            call_tree: CallTree::new(),
            needs_rerender: false,
            module: None,
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

    /// Check if a rerender is needed due to state updates.
    ///
    /// This flag is set to `true` when any state variable is updated via assignment.
    /// Multiple state updates in a single render will only set this flag once,
    /// ensuring only one rerender is triggered per render cycle.
    ///
    /// # Returns
    ///
    /// Returns `true` if a rerender is needed, `false` otherwise.
    pub fn needs_rerender(&self) -> bool {
        self.needs_rerender
    }

    /// Clear the rerender flag.
    ///
    /// This should be called after a rerender has been triggered to reset
    /// the flag for the next render cycle.
    pub fn clear_rerender_flag(&mut self) {
        self.needs_rerender = false;
    }

    /// Set the module for this runtime.
    ///
    /// This stores the parsed module AST so it can be re-executed when rerendering.
    pub fn set_module(&mut self, module: Function) {
        self.module = Some(module);
    }

    /// Get a reference to the stored module.
    pub fn get_module(&self) -> Option<&Function> {
        self.module.as_ref()
    }

    /// Re-execute the stored module and return the result.
    ///
    /// This preserves state in the call tree, allowing stateful components
    /// to maintain their state across rerenders. The call tree structure is
    /// preserved, and we reuse existing nodes when re-entering the same call
    /// sequence, ensuring state persists across rerenders.
    ///
    /// # Returns
    ///
    /// Returns the result of executing the module, or an error if no module
    /// is stored or execution fails.
    pub fn rerender(&mut self) -> Result<Value, VMError> {
        // Clone the module to avoid borrow checker issues
        let module = self.module.clone().ok_or_else(|| {
            VMError::InvalidOperation("No module stored in runtime. Cannot rerender.".to_string())
        })?;

        // Clear the rerender flag before re-executing
        self.needs_rerender = false;

        // Start rerender cycle - this enables node reuse
        self.call_tree.start_rerender();

        // Reset environment to module level
        self.environment = Environment::new();

        let result = self.execute_module(&module);

        // End rerender cycle
        self.call_tree.end_rerender();

        result
    }

    /// Execute a module (Function) and look for a main function to call
    pub fn execute_module(&mut self, module: &Function) -> Result<Value, VMError> {
        // Execute the module body to populate the environment
        self.execute_block(&module.body)?;

        // Look for a 'main' variable that is a function
        if let Some(Value::Closure(main_closure)) = self.environment.get("main") {
            // Call the main function
            self.call_closure(&main_closure, &[], "main")
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
                // State is stored in the call tree, not the environment
                let name = state_stmt.get_identifier_value();

                // Check if state already exists in the call tree (from a previous render)
                // If it exists, preserve it; otherwise, initialize with the expression value
                let value = if self.call_tree.get_state(&name).is_some() {
                    // State already exists - preserve it
                    self.call_tree.get_state(&name).unwrap()
                } else {
                    // State doesn't exist - initialize with the expression value
                    self.evaluate_expression(&state_stmt.get_value())?
                };

                // Store in call tree for persistent state tracking
                self.call_tree.set_state(name.clone(), value.clone());

                // Also store in environment for immediate access during execution
                self.environment.define(name, value);
                Ok(Value::Void)
            }
            Statement::Assign(assign_stmt) => {
                let name = assign_stmt.get_identifier_value();
                let value = self.evaluate_expression(&assign_stmt.get_value())?;

                // Check if this is a state variable (exists in call tree)
                if self.call_tree.get_state(&name).is_some() {
                    // Update state in call tree
                    self.call_tree.set_state(name.clone(), value.clone());
                    // Flag that a rerender is needed (multiple updates in one render
                    // will only set this flag once, which is the desired behavior)
                    self.needs_rerender = true;
                }

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
                    Value::Widget(widget) => {
                        widget.properties.get(&key).cloned().ok_or_else(|| {
                            VMError::InvalidOperation(format!("Widget has no property '{}'", key))
                        })
                    }
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
                // First check environment (for regular variables), then call tree state, then host state
                if let Some(value) = self.environment.get(&name) {
                    Ok(value)
                } else if let Some(value) = self.call_tree.get_state(&name) {
                    Ok(value)
                } else if let Some(value) = self.get_host_state(&name) {
                    Ok(value)
                } else {
                    Err(VMError::UndefinedVariable(name))
                }
            }
            Literal::Call(call) => self.execute_call(call),
            Literal::Function(func) => {
                // Capture the current environment when creating the closure
                Ok(Value::Closure(Closure {
                    function: func.clone(),
                    captured_env: self.environment.clone(),
                }))
            }
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
        if let Some(Value::Closure(closure)) = self.environment.get(&func_name) {
            self.call_closure(&closure, &args, &func_name)
        } else {
            Err(VMError::UndefinedVariable(format!(
                "Function '{}' not found",
                func_name
            )))
        }
    }

    /// Call a closure with arguments, using its captured environment.
    ///
    /// This is used internally by the runtime and also by the UI bridge when wiring
    /// Ogham functions to widget event listeners (e.g. `mouse_down` handlers).
    pub fn call_closure(
        &mut self,
        closure: &Closure,
        args: &[Value],
        function_name: &str,
    ) -> Result<Value, VMError> {
        let func = &closure.function;
        // Check argument count
        if args.len() != func.arguments.len() {
            return Err(VMError::InvalidOperation(format!(
                "Expected {} arguments, got {}",
                func.arguments.len(),
                args.len()
            )));
        }

        // Enter the function call in the call tree
        self.call_tree.enter_call(function_name.to_string());

        // Get state from the call tree (searching all nodes, not just current)
        // This allows event handlers to access state from parent function calls
        // We collect all state from the entire tree to make it available in the environment
        let all_state: Vec<(String, Value)> = {
            // Search the entire call tree for state variables
            let mut state_map = HashMap::new();
            CallTree::collect_state_from_tree(&self.call_tree.root, &mut state_map);
            state_map.into_iter().collect()
        };

        // Create new environment with the captured environment as parent
        // This allows the closure to access variables from its lexical scope
        let mut func_env = Environment::new_with_parent(closure.captured_env.clone());
        for (param, arg_value) in func.arguments.iter().zip(args.iter()) {
            func_env.define(param.get(), arg_value.clone());
        }

        // Restore all state from the call tree into the environment
        // This allows state to be accessed during execution, even from parent calls
        for (name, value) in all_state {
            func_env.define(name, value);
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

        // Exit the function call in the call tree
        self.call_tree.exit_call();

        result
    }

    /// Call a function with arguments (legacy method, kept for backward compatibility).
    /// This creates a closure on the fly without capturing the environment.
    ///
    /// Prefer using `call_closure` when you have a closure value.
    pub fn call_function(
        &mut self,
        func: &Function,
        args: &[Value],
        function_name: &str,
    ) -> Result<Value, VMError> {
        // Create a temporary closure without captured environment
        let closure = Closure {
            function: func.clone(),
            captured_env: self.environment.clone(),
        };
        self.call_closure(&closure, args, function_name)
    }
}

impl Default for Runtime {
    fn default() -> Self {
        Self::new()
    }
}

pub fn from_file<P: AsRef<Path>>(
    path: P,
    config: Option<RuntimeConfig>,
) -> Result<Runtime, RuntimeError> {
    let source = fs::read_to_string(path)?;
    from_source(&source, config)
}

pub fn from_source(source: &str, config: Option<RuntimeConfig>) -> Result<Runtime, RuntimeError> {
    // Step 1: Scan source into tokens
    let mut scanner = Scanner::new(source.to_string());
    let tokens = scanner.scan();

    // Step 2: Parse tokens into AST
    let mut parser = Parser::new(tokens);
    let module = parser.parse()?;

    // Step 3: Execute in Runtime (kept alive for UI event handlers)
    let mut runtime = Runtime::new();

    // Inject host state if provided
    if let Some(config) = config.as_ref() {
        if let Some(ref state) = config.host_state.as_ref() {
            for (name, value) in state.iter() {
                runtime.inject_host_state(name.clone(), value.clone());
            }
        }

        // Register per-event handlers (for `event("name", ...)`).
        for (name, handler) in config.event_handlers.iter() {
            runtime.register_event_handler_arc(name.clone(), handler.clone());
        }
    }

    // Store the module in the runtime for potential rerendering
    runtime.set_module(module.clone());

    // let value = runtime.execute_module(&module)?;

    Ok(runtime)
}

pub struct FileWatcher {
    _watcher: RecommendedWatcher,
    receiver: mpsc::Receiver<Result<NotifyEvent, notify::Error>>,
    watched_path: PathBuf,
}

impl FileWatcher {
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

    pub fn path(&self) -> &Path {
        &self.watched_path
    }

    pub fn recompile(&self, config: Option<RuntimeConfig>) -> Result<Runtime, RuntimeError> {
        from_file(&self.watched_path, config)
    }
}

pub fn watch_and_compile<P: AsRef<Path>>(
    path: P,
    config: Option<RuntimeConfig>,
) -> Result<(Runtime, FileWatcher), RuntimeError> {
    let watcher = FileWatcher::new(&path)?;
    let runtime = from_file(&path, config)?;
    Ok((runtime, watcher))
}
