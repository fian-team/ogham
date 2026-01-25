use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::Arc;

use crate::parser::{Block, Call, Expression, Function, Literal, Operator, Parser, Statement};
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
}

impl Runtime {
    pub fn new() -> Self {
        Self {
            environment: Environment::new(),
            host_state: HashMap::new(),
            event_handlers: HashMap::new(),
            needs_rerender: false,
            module: None,
        }
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

    pub fn clear_rerender_flag(&mut self) {
        self.needs_rerender = false;
    }

    pub fn set_module(&mut self, module: Function) {
        self.module = Some(module);
    }

    pub fn get_module(&self) -> Option<&Function> {
        self.module.as_ref()
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
        let result = self.execute_module(&module);
        result
    }

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
                // Treat state declarations like regular variable declarations
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
                } else if let Some(value) = self.get_host_state(&name) {
                    Ok(value)
                } else {
                    Err(VMError::UndefinedVariable(name))
                }
            }
            Literal::Call(call) => self.execute_call(call),
            Literal::Function(func) => {
                // Capture the current environment and call tree path when creating the closure
                Ok(Value::Closure(Closure {
                    function: func.clone(),
                    captured_env: self.environment.clone(),
                    captured_path: Vec::new(),
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

        // Create new environment with the captured environment as parent
        // This allows the closure to access variables from its lexical scope
        let mut func_env = Environment::new_with_parent(closure.captured_env.clone());
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
            captured_path: Vec::new(),
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
