use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::parser::MemberAccess;
use crate::parser::{
    Block, Call, Expression, ForLoopExpression, Function, ImportStatement, Literal,
    MatchExpression, Operator, Parser, Statement,
};
use crate::runtime::closure::Closure;
use crate::runtime::config::RuntimeConfig;
use crate::runtime::environment::Environment;
use crate::runtime::error::{RuntimeError, VMError};
use crate::runtime::value::Value;
use crate::runtime::widget::RuntimeWidget;
use crate::scanner::Scanner;

pub mod closure;
pub mod config;
pub mod environment;
pub mod error;
pub mod value;
pub mod widget;

pub struct Runtime {
    environment: Environment,
    host_state: HashMap<String, Value>,
    event_handlers: HashMap<String, Arc<dyn Fn(&[Value]) -> bool + Send + Sync>>,
    needs_rerender: bool,
    module: Option<Function>,
    // State management
    component_state: HashMap<String, Value>, // Key format: "{call_stack_path}:{variable_name}"
    call_stack: Vec<String>,                 // Current execution path
    active_state_paths: HashSet<String>,     // Paths that declared state in current render
    has_branched: bool,                      // Track if branching has occurred in current function
    call_counters: HashMap<String, usize>, // Track call counts per function to generate unique paths
    // Import resolution
    project_root: Option<PathBuf>,
    import_loading_stack: Vec<PathBuf>,
    import_loaded: HashSet<PathBuf>,
    /// Cached environment per resolved path so re-imports (e.g. view_two importing button)
    /// can merge that module's exports into the current scope without re-executing.
    import_cache: HashMap<PathBuf, Environment>,
}

impl Runtime {
    pub fn new() -> Self {
        Self {
            environment: Environment::new(),
            host_state: HashMap::new(),
            event_handlers: HashMap::new(),
            needs_rerender: false,
            module: None,
            component_state: HashMap::new(),
            call_stack: Vec::new(),
            active_state_paths: HashSet::new(),
            has_branched: false,
            call_counters: HashMap::new(),
            project_root: None,
            import_loading_stack: Vec::new(),
            import_loaded: HashSet::new(),
            import_cache: HashMap::new(),
        }
    }

    pub fn set_project_root(&mut self, path: PathBuf) {
        self.project_root = Some(path);
    }

    pub fn project_root(&self) -> Option<&PathBuf> {
        self.project_root.as_ref()
    }

    pub fn inject_host_state(&mut self, name: String, value: Value) {
        self.host_state.insert(name, value);
    }

    pub fn get_host_state(&self, name: &str) -> Option<Value> {
        self.host_state.get(name).cloned()
    }

    pub fn register_event_handler<S, F>(&mut self, name: S, handler: F)
    where
        S: Into<String>,
        F: Fn(&[Value]) -> bool + Send + Sync + 'static,
    {
        self.event_handlers.insert(name.into(), Arc::new(handler));
    }

    pub fn register_event_handler_arc(
        &mut self,
        name: String,
        handler: Arc<dyn Fn(&[Value]) -> bool + Send + Sync>,
    ) {
        self.event_handlers.insert(name, handler);
    }

    pub fn emit_event(&self, name: &str, args: &[Value]) -> bool {
        self.event_handlers
            .get(name)
            .map(|handler| handler(args))
            .unwrap_or(false)
    }

    pub fn needs_rerender(&self) -> bool {
        self.needs_rerender
    }

    pub fn request_rerender(&mut self) {
        self.needs_rerender = true;
    }

    pub fn clear_rerender_flag(&mut self) {
        self.needs_rerender = false;
    }

    pub fn set_module(&mut self, module: Function) {
        self.module = Some(module);
    }

    pub fn get_module(&self) -> Option<&Function> {
        self.module.as_ref()
    }

    /// Returns the canonical paths of all modules that were imported during the last
    /// execute_module/rerender. Used by the file watcher to watch every file that
    /// affects the current UI.
    pub fn get_imported_paths(&self) -> Vec<PathBuf> {
        self.import_loaded.iter().cloned().collect()
    }

    pub fn rerender(&mut self) -> Result<Value, VMError> {
        // Clone the module to avoid borrow checker issues
        let module = self.module.clone().ok_or_else(|| {
            VMError::InvalidOperation("No module stored in runtime. Cannot rerender.".to_string())
        })?;
        // Clear the rerender flag before re-executing
        self.needs_rerender = false;
        // Reset environment to module level
        self.environment = Environment::new();
        // Clear active state paths for new render
        self.active_state_paths.clear();
        // Clear call stack
        self.call_stack.clear();
        // Reset call counters for new render
        self.call_counters.clear();
        let result = self.execute_module(&module);
        // Cleanup state for unmounted components
        self.cleanup_unmounted_state();
        result
    }

    pub fn execute_module(&mut self, module: &Function) -> Result<Value, VMError> {
        // Clear active state paths for new render
        self.active_state_paths.clear();
        // Clear call stack
        self.call_stack.clear();
        // Reset call counters for new render
        self.call_counters.clear();
        // Clear import state so each run re-imports and can detect cycles
        self.import_loading_stack.clear();
        self.import_loaded.clear();
        self.import_cache.clear();
        // Execute the module body to populate the environment
        self.execute_block(&module.body)?;
        // Look for a 'main' variable that is a function
        let result = if let Some(Value::Closure(main_closure)) = self.environment.get("main") {
            // Call the main function
            self.call_closure(&main_closure, &[], "main")
        } else {
            Ok(Value::Void)
        };
        // Cleanup state for unmounted components
        self.cleanup_unmounted_state();
        result
    }

    /// Get the current call stack path as a string
    fn get_call_stack_path(&self) -> String {
        if self.call_stack.is_empty() {
            "".to_string()
        } else {
            self.call_stack.join("/")
        }
    }

    /// Generate a state key from the current call stack path and variable name
    fn get_state_key(&self, variable_name: &str) -> String {
        let path = self.get_call_stack_path();
        if path.is_empty() {
            format!(":{}", variable_name)
        } else {
            format!("{}:{}", path, variable_name)
        }
    }

    /// Get state value for a variable, searching up the call stack
    fn get_state_value(&self, variable_name: &str) -> Option<Value> {
        // Search up the call stack, from most specific to least specific
        let mut search_path = self.call_stack.clone();

        // First try with full path
        loop {
            let path_str = if search_path.is_empty() {
                "".to_string()
            } else {
                search_path.join("/")
            };
            let key = if path_str.is_empty() {
                format!(":{}", variable_name)
            } else {
                format!("{}:{}", path_str, variable_name)
            };

            if let Some(value) = self.component_state.get(&key) {
                return Some(value.clone());
            }

            // If we've exhausted the path, break
            if search_path.is_empty() {
                break;
            }

            // Try with shorter path (pop one level)
            search_path.pop();
        }

        None
    }

    /// Check if a variable is a state variable, searching up the call stack
    fn is_state_variable(&self, variable_name: &str) -> bool {
        self.get_state_value(variable_name).is_some()
    }

    /// Set state value for a variable at the current call stack path
    /// This will update the state at the most specific path where it exists,
    /// or create it at the current path if it doesn't exist
    fn set_state_value(&mut self, variable_name: &str, value: Value) {
        // First, try to find existing state up the call stack
        let mut search_path = self.call_stack.clone();
        let mut found = false;

        loop {
            let path_str = if search_path.is_empty() {
                "".to_string()
            } else {
                search_path.join("/")
            };
            let key = if path_str.is_empty() {
                format!(":{}", variable_name)
            } else {
                format!("{}:{}", path_str, variable_name)
            };

            if self.component_state.contains_key(&key) {
                // Found existing state, update it
                self.component_state.insert(key, value.clone());
                found = true;
                break;
            }

            // If we've exhausted the path, break
            if search_path.is_empty() {
                break;
            }

            // Try with shorter path (pop one level)
            search_path.pop();
        }

        // If not found, create at current path
        if !found {
            let key = self.get_state_key(variable_name);
            self.component_state.insert(key, value);
        }
    }

    /// Cleanup state for components that are no longer mounted
    fn cleanup_unmounted_state(&mut self) {
        // Collect keys to remove
        let keys_to_remove: Vec<String> = self
            .component_state
            .keys()
            .filter(|key| {
                // Extract path from key (format: "{path}:{variable_name}")
                if let Some(colon_pos) = key.find(':') {
                    let path = &key[..colon_pos];
                    // Remove if path is not in active_state_paths
                    !self.active_state_paths.contains(path)
                } else {
                    // Malformed key, remove it
                    true
                }
            })
            .cloned()
            .collect();

        // Remove the keys
        for key in keys_to_remove {
            self.component_state.remove(&key);
        }
    }

    fn execute_block(&mut self, block: &Block) -> Result<Value, VMError> {
        for statement in &block.statement_list {
            match self.execute_statement(statement)? {
                Value::Void => continue,
                value => return Ok(value),
            }
        }
        Ok(Value::Void)
    }

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
                // Enforce that state declarations must be at the beginning of a function
                if self.has_branched {
                    return Err(VMError::InvalidOperation(
                        "State declarations must occur at the beginning of a function, before any branching statements".to_string(),
                    ));
                }

                let name = state_stmt.get_identifier_value();
                let call_stack_path = self.get_call_stack_path();

                // Add this path to active state paths
                if !call_stack_path.is_empty() {
                    self.active_state_paths.insert(call_stack_path.clone());
                }

                // Check if state already exists at the CURRENT call stack path (not up the stack)
                let state_key = self.get_state_key(&name);
                let value = if let Some(existing_value) = self.component_state.get(&state_key) {
                    // State exists at current path, use existing value (don't reinitialize)
                    existing_value.clone()
                } else {
                    // State doesn't exist at current path, initialize with provided value
                    let initial_value = self.evaluate_expression(&state_stmt.get_value())?;
                    self.component_state
                        .insert(state_key, initial_value.clone());
                    initial_value
                };

                // Also store in environment for immediate access during current execution
                self.environment.define(name, value);
                Ok(Value::Void)
            }
            Statement::Assign(assign_stmt) => {
                let name = assign_stmt.get_identifier_value();
                let value = self.evaluate_expression(&assign_stmt.get_value())?;

                // Check if this is a state variable
                if self.is_state_variable(&name) {
                    // Update state map
                    self.set_state_value(&name, value.clone());
                    // Trigger rerender
                    self.needs_rerender = true;
                    // Also update environment for immediate access
                    self.environment.assign(&name, value);
                    Ok(Value::Void)
                } else if self.environment.assign(&name, value.clone()) {
                    // Regular variable assignment
                    Ok(Value::Void)
                } else {
                    Err(VMError::UndefinedVariable(name))
                }
            }
            Statement::Return(return_stmt) => {
                // Mark that branching has occurred (return is a form of branching)
                self.has_branched = true;
                if let Some(expr) = return_stmt.get_value() {
                    let value = self.evaluate_expression(&expr)?;
                    Err(VMError::Return(value))
                } else {
                    Err(VMError::Return(Value::Void))
                }
            }
            Statement::Conditional(cond_stmt) => {
                // Mark that branching has occurred
                self.has_branched = true;
                // Check all branches (if and else if)
                let mut matched = false;
                for (condition, block) in cond_stmt.get_branches() {
                    let condition_value = self.evaluate_expression(condition)?;
                    if let Value::Boolean(true) = condition_value {
                        matched = true;
                        return self.execute_block(block);
                    }
                }
                // Execute else block if no branch matched
                if !matched {
                    if let Some(else_block) = cond_stmt.get_else_block() {
                        return self.execute_block(else_block);
                    }
                }
                Ok(Value::Void)
            }
            Statement::Log(log_stmt) => {
                let _value = self.evaluate_expression(&log_stmt.get_value())?;
                Ok(Value::Void)
            }
            Statement::Event(_) => {
                // Events are not executed by the runtime
                Ok(Value::Void)
            }
            Statement::Import(import_stmt) => self.execute_import(import_stmt),
            Statement::ForLoop(for_loop) => {
                // Mark that branching has occurred
                self.has_branched = true;
                let range_start = self.evaluate_expression(&for_loop.get_range_start())?;
                let range_end = self.evaluate_expression(&for_loop.get_range_end())?;

                let start = match range_start {
                    Value::Integer(i) => i,
                    _ => {
                        return Err(VMError::TypeMismatch(
                            "Range start must be an integer".to_string(),
                        ))
                    }
                };

                let end = match range_end {
                    Value::Integer(i) => i,
                    _ => {
                        return Err(VMError::TypeMismatch(
                            "Range end must be an integer".to_string(),
                        ))
                    }
                };

                let variable_name = for_loop.get_variable().get();
                let body = for_loop.get_body();

                // Create a new scope for the loop
                let parent_env = self.environment.clone();

                // Iterate from start to end (exclusive)
                for i in start..end {
                    // Create new environment for this iteration
                    self.environment = Environment::new_with_parent(parent_env.clone());
                    // Set loop variable
                    self.environment
                        .define(variable_name.clone(), Value::Integer(i));
                    // Execute body
                    self.execute_block(&body)?;
                }

                // Restore parent environment
                self.environment = parent_env;

                Ok(Value::Void)
            }
        }
    }

    fn execute_import(&mut self, import_stmt: &ImportStatement) -> Result<Value, VMError> {
        let project_root = self.project_root.as_ref().ok_or_else(|| {
            VMError::ImportError("project root not set; cannot resolve import path".to_string())
        })?;

        let path_str = import_stmt.get_path();
        let mut resolved = project_root.join(path_str);
        if resolved.extension().is_none() {
            resolved.set_extension("ogh");
        }

        let source = fs::read_to_string(&resolved).map_err(|e| {
            VMError::ImportError(format!("failed to read {}: {}", resolved.display(), e))
        })?;

        let key = resolved.canonicalize().unwrap_or(resolved.clone());

        if self.import_loading_stack.contains(&key) {
            let mut cycle = self.import_loading_stack.clone();
            cycle.push(key.clone());
            return Err(VMError::ImportCycle(cycle));
        }
        if self.import_loaded.contains(&key) {
            // Module already loaded (e.g. button.ogh by view_one). Merge its exports
            // into the current scope so this module can use them.
            if let Some(cached_env) = self.import_cache.get(&key) {
                let names_to_copy_opt: Option<Vec<String>> = import_stmt.get_names().clone();
                if let Some(ref names) = names_to_copy_opt {
                    for name in names {
                        if cached_env.get(name).is_none() {
                            return Err(VMError::ImportError(format!(
                                "export '{}' not found in {}",
                                name,
                                resolved.display()
                            )));
                        }
                    }
                }
                if let Err(conflict_name) =
                    self.environment
                        .copy_from(cached_env, names_to_copy_opt.as_deref(), true)
                {
                    return Err(VMError::ImportConflict(conflict_name));
                }
            }
            return Ok(Value::Void);
        }

        self.import_loading_stack.push(key.clone());

        let mut scanner = Scanner::new(source);
        let tokens = scanner.scan();
        let mut parser = Parser::new(tokens);
        let imported_module = parser.parse().map_err(|e| {
            VMError::ImportError(format!(
                "parse error in {} at {}:{}: {}",
                resolved.display(),
                e.line,
                e.column,
                e.message
            ))
        })?;

        let temp_env = Environment::new();
        let saved_env = std::mem::replace(&mut self.environment, temp_env);
        let block_result = self.execute_block(&imported_module.body);
        let temp_env = std::mem::replace(&mut self.environment, saved_env);

        if let Err(e) = block_result {
            match e {
                VMError::Return(_) => { /* imported module had return; ignore */ }
                other => {
                    self.import_loading_stack.pop();
                    return Err(other);
                }
            }
        }

        let names_to_copy: Option<Vec<String>> = import_stmt.get_names().clone();
        if let Some(ref names) = names_to_copy {
            for name in names {
                if temp_env.get(name).is_none() {
                    self.import_loading_stack.pop();
                    return Err(VMError::ImportError(format!(
                        "export '{}' not found in {}",
                        name,
                        resolved.display()
                    )));
                }
            }
        }

        // Cache this module's env so later re-imports (e.g. view_two importing button)
        // can merge its exports into their scope without re-executing.
        self.import_cache.insert(key.clone(), temp_env.clone());

        // Use check_conflict = false so re-exported names (e.g. button from multiple
        // view files that each import button.ogh) overwrite instead of conflicting.
        if let Err(conflict_name) =
            self.environment
                .copy_from(&temp_env, names_to_copy.as_deref(), false)
        {
            self.import_loading_stack.pop();
            return Err(VMError::ImportConflict(conflict_name));
        }

        self.import_loading_stack.pop();
        self.import_loaded.insert(key);

        Ok(Value::Void)
    }

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
            Expression::Call(call) => self.execute_call(call),
            Expression::IndexAccess(access) => {
                let object = self.evaluate_expression(&access.object)?;
                let index_val = self.evaluate_expression(&access.index)?;
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
                        Ok(arr[idx].clone())
                    }
                    (Value::Array(_), other) => Err(VMError::TypeMismatch(format!(
                        "Array index must be an integer, got {:?}",
                        other
                    ))),
                    (other, _) => Err(VMError::TypeMismatch(format!(
                        "Cannot index {:?}; only arrays support index access",
                        other
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
            Expression::Range(range) => {
                // Ranges are typically only used in for loops, but we can evaluate them
                let start = self.evaluate_expression(&range.start)?;
                let end = self.evaluate_expression(&range.end)?;
                // Return a tuple-like value? Actually, ranges are usually just used in parsing.
                // For now, we'll return the start value as a placeholder.
                // In practice, ranges are extracted before evaluation.
                Ok(start)
            }
            Expression::ForLoop(for_loop) => self.evaluate_for_loop_expression(for_loop, false),
            Expression::SpreadForLoop(for_loop) => {
                self.evaluate_for_loop_expression(for_loop, true)
            }
            Expression::Spread(_) => Err(VMError::InvalidOperation(
                "Spread is only allowed inside array literals".to_string(),
            )),
            Expression::Match(m) => self.evaluate_match_expression(m),
        }
    }

    fn evaluate_match_expression(&mut self, m: &MatchExpression) -> Result<Value, VMError> {
        self.has_branched = true;
        let scrutinee = self.evaluate_expression(&m.scrutinee)?;

        for (pattern_expr, block) in &m.arms {
            let matches = match pattern_expr {
                Expression::Literal(Literal::Identifier(ident)) if ident.get() == "_" => true,
                _ => {
                    let pattern_value = self.evaluate_expression(pattern_expr)?;
                    scrutinee == pattern_value
                }
            };
            if matches {
                return match self.execute_block(block) {
                    Ok(Value::Void) => Ok(Value::Void),
                    Err(VMError::Return(value)) => Ok(value),
                    Err(e) => Err(e),
                    Ok(value) => Ok(value),
                };
            }
        }

        Err(VMError::InvalidOperation(
            "match non-exhaustive: no arm matched".to_string(),
        ))
    }

    fn evaluate_for_loop_expression(
        &mut self,
        for_loop: &ForLoopExpression,
        _is_spread: bool,
    ) -> Result<Value, VMError> {
        let range_start = self.evaluate_expression(&for_loop.range_start)?;
        let range_end = self.evaluate_expression(&for_loop.range_end)?;

        let start = match range_start {
            Value::Integer(i) => i,
            _ => {
                return Err(VMError::TypeMismatch(
                    "Range start must be an integer".to_string(),
                ))
            }
        };

        let end = match range_end {
            Value::Integer(i) => i,
            _ => {
                return Err(VMError::TypeMismatch(
                    "Range end must be an integer".to_string(),
                ))
            }
        };

        let variable_name = for_loop.variable.get();
        let body = &for_loop.body;

        // Create a new scope for the loop
        let parent_env = self.environment.clone();
        let mut results = Vec::new();

        // Iterate from start to end (exclusive)
        for i in start..end {
            // Create new environment for this iteration
            self.environment = Environment::new_with_parent(parent_env.clone());
            // Set loop variable
            self.environment
                .define(variable_name.clone(), Value::Integer(i));
            // Execute body and collect return values
            // execute_block propagates VMError::Return, which we need to catch
            match self.execute_block(body) {
                Ok(Value::Void) => {
                    // No return value - this happens when all statements are void
                    // In for loop expressions, we typically want to collect values, so skip this iteration
                }
                Err(VMError::Return(value)) => {
                    // Explicit or implicit return statement (implicit returns are converted to Return by parser)
                    results.push(value);
                }
                Err(e) => {
                    // Restore environment before propagating error
                    self.environment = parent_env;
                    return Err(e);
                }
                Ok(value) => {
                    // Shouldn't happen normally, but handle it
                    results.push(value);
                }
            }
        }

        // Restore parent environment
        self.environment = parent_env;

        Ok(Value::Array(results))
    }

    fn evaluate_literal(&mut self, literal: &Literal) -> Result<Value, VMError> {
        match literal {
            Literal::Integer(i) => Ok(Value::Integer(*i)),
            Literal::Float(f) => Ok(Value::Float(*f)),
            Literal::Boolean(b) => Ok(Value::Boolean(*b)),
            Literal::String(s) => Ok(Value::String(s.clone())),
            Literal::Identifier(ident) => {
                let name = ident.get();
                // Lookup order: state map → environment → host state
                if let Some(value) = self.get_state_value(&name) {
                    Ok(value)
                } else if let Some(value) = self.environment.get(&name) {
                    Ok(value)
                } else if let Some(value) = self.get_host_state(&name) {
                    Ok(value)
                } else {
                    Err(VMError::UndefinedVariable(name))
                }
            }
            Literal::Function(func) => {
                // Capture the current environment and call stack path when creating the closure
                Ok(Value::Closure(Closure {
                    function: func.clone(),
                    captured_env: self.environment.clone(),
                    captured_path: self.call_stack.clone(),
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
                    match expr {
                        Expression::SpreadForLoop(for_loop) => {
                            // Evaluate the for loop and spread its results
                            let for_loop_results =
                                self.evaluate_for_loop_expression(for_loop, true)?;
                            if let Value::Array(results) = for_loop_results {
                                // Spread the results into the parent array
                                value_array.extend(results);
                            } else {
                                // Shouldn't happen, but handle gracefully
                                value_array.push(for_loop_results);
                            }
                        }
                        Expression::Spread(inner) => {
                            let spread_value = self.evaluate_expression(inner)?;
                            if let Value::Array(results) = spread_value {
                                value_array.extend(results);
                            } else {
                                return Err(VMError::TypeMismatch(format!(
                                    "Spread in array expects an array, got {:?}",
                                    spread_value
                                )));
                            }
                        }
                        _ => {
                            let value = self.evaluate_expression(expr)?;
                            value_array.push(value);
                        }
                    }
                }
                Ok(Value::Array(value_array))
            }
        }
    }

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
            Operator::Equals => self.compare_equals(left, right),
            Operator::NotEquals => self.compare_not_equals(left, right),
            Operator::GreaterThan => self.compare_greater_than(left, right),
            Operator::GreaterThanOrEqualTo => self.compare_greater_than_or_equal(left, right),
            Operator::LessThan => self.compare_less_than(left, right),
            Operator::LessThanOrEqualTo => self.compare_less_than_or_equal(left, right),
            Operator::Not => Err(VMError::InvalidOperation(
                "Not operator is not a binary operator".to_string(),
            )),
        }
    }

    fn compare_equals(&self, left: &Value, right: &Value) -> Result<Value, VMError> {
        if std::mem::discriminant(left) != std::mem::discriminant(right) {
            return Err(VMError::TypeMismatch(format!(
                "Cannot compare values of different types with ==: {:?} and {:?}",
                left, right
            )));
        }
        Ok(Value::Boolean(left == right))
    }

    fn compare_not_equals(&self, left: &Value, right: &Value) -> Result<Value, VMError> {
        if std::mem::discriminant(left) != std::mem::discriminant(right) {
            return Err(VMError::TypeMismatch(format!(
                "Cannot compare values of different types with !=: {:?} and {:?}",
                left, right
            )));
        }
        Ok(Value::Boolean(left != right))
    }

    fn add(&self, left: &Value, right: &Value) -> Result<Value, VMError> {
        match (left, right) {
            (Value::Integer(a), Value::Integer(b)) => Ok(Value::Integer(a + b)),
            (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a + b)),
            (Value::Integer(a), Value::Float(b)) => Ok(Value::Float(*a as f64 + b)),
            (Value::Float(a), Value::Integer(b)) => Ok(Value::Float(a + *b as f64)),
            (Value::String(a), Value::String(b)) => Ok(Value::String(format!("{}{}", a, b))),
            (Value::String(a), b) => Ok(Value::String(format!("{}{}", a, b))),
            (a, Value::String(b)) => Ok(Value::String(format!("{}{}", a, b))),
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

    fn execute_call(&mut self, call: &Call) -> Result<Value, VMError> {
        // Evaluate arguments
        let mut args = Vec::new();
        for arg_expr in &call.arguments {
            args.push(self.evaluate_expression(arg_expr)?);
        }

        // Special-case: array.length() — only method call, not property
        if let Expression::MemberAccess(MemberAccess { object, property }) = &*call.callee {
            if property.get() == "length" {
                let obj_val = self.evaluate_expression(object)?;
                if let Value::Array(arr) = obj_val {
                    if args.is_empty() {
                        return Ok(Value::Integer(arr.len() as i32));
                    }
                    return Err(VMError::InvalidOperation(
                        "length() takes no arguments".to_string(),
                    ));
                }
                return Err(VMError::InvalidOperation(
                    "length() can only be called on an array".to_string(),
                ));
            }
        }

        // Built-in event() when callee is identifier "event"
        if let Expression::Literal(Literal::Identifier(ident)) = &*call.callee {
            if ident.get() == "event" {
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
                let _handled = self.emit_event(&event_name, &args[1..]);
                return Ok(Value::Void);
            }
        }

        // Resolve callee and call closure
        let callee_val = self.evaluate_expression(&call.callee)?;
        let func_name = if let Expression::Literal(Literal::Identifier(ident)) = &*call.callee {
            ident.get()
        } else {
            "fn".to_string()
        };
        if let Value::Closure(closure) = callee_val {
            self.call_closure(&closure, &args, &func_name)
        } else {
            Err(VMError::TypeMismatch(
                "Only functions can be called".to_string(),
            ))
        }
    }

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

        // Save current state
        let old_env = std::mem::replace(&mut self.environment, Environment::new());
        let old_has_branched = self.has_branched;
        let old_call_stack = self.call_stack.clone();

        // For closures, we need to restore the captured path and then push the function name
        // This allows closures to access state from their lexical scope
        self.call_stack = closure.captured_path.clone();

        // Generate unique identifier for this function call
        // Use the current call stack path + function name as a key to track call count
        let call_site_key = if self.call_stack.is_empty() {
            function_name.to_string()
        } else {
            format!("{}/{}", self.call_stack.join("/"), function_name)
        };

        // Get or increment call counter for this call site
        let call_index = self.call_counters.entry(call_site_key.clone()).or_insert(0);
        *call_index += 1;
        let current_call_index = *call_index;

        // Push function name with index to call stack for this call
        // This ensures each call to the same function gets a unique path
        let unique_function_id = format!("{}@{}", function_name, current_call_index);
        self.call_stack.push(unique_function_id);

        // Reset has_branched for new function call
        self.has_branched = false;

        // Create new environment with the captured environment as parent
        // This allows the closure to access variables from its lexical scope
        let mut func_env = Environment::new_with_parent(closure.captured_env.clone());
        for (param, arg_value) in func.arguments.iter().zip(args.iter()) {
            func_env.define(param.get(), arg_value.clone());
        }
        self.environment = func_env;

        // Execute function body
        let result = match self.execute_block(&func.body) {
            Ok(Value::Void) => Ok(Value::Void),
            Err(VMError::Return(value)) => Ok(value),
            Err(e) => Err(e),
            Ok(value) => Ok(value),
        };

        // Restore state
        self.call_stack = old_call_stack;
        self.environment = old_env;
        self.has_branched = old_has_branched;

        result
    }

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
            captured_path: self.call_stack.clone(),
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
    let path_buf = path.as_ref().to_path_buf();
    let source = fs::read_to_string(&path_buf)?;
    let mut runtime = from_source(&source, config)?;
    if runtime.project_root().is_none() {
        runtime.set_project_root(
            path_buf
                .parent()
                .map(|p| p.to_path_buf())
                .unwrap_or_else(|| PathBuf::from(".")),
        );
    }
    Ok(runtime)
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

    // Inject host state and config if provided
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

        if let Some(ref project_root) = config.project_root {
            runtime.set_project_root(project_root.clone());
        }
    }

    // Store the module in the runtime for potential rerendering
    runtime.set_module(module.clone());

    // let value = runtime.execute_module(&module)?;

    Ok(runtime)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn array_access_length_and_index() {
        let source = r#"
let main = fn () {
  let array = [1, 2, 3, 4, 5];
  let last_index = array.length() - 1;
  let value = array[last_index];
  value
};
"#;
        let mut runtime = from_source(source, None).expect("parse and create runtime");
        let module = runtime.get_module().expect("module").clone();
        let result = runtime.execute_module(&module).expect("execute");
        // main() returns the last element: 5
        assert_eq!(result, Value::Integer(5));
    }
}
