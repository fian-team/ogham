use super::parser::*;
use std::collections::HashMap;

/// A widget value produced by the VM. Unlike the parser's `Widget`, all properties
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

/// Virtual machine for executing parsed AST
pub struct VM {
    environment: Environment,
    host_state: HashMap<String, Value>,
}

#[derive(Debug)]
pub enum VMError {
    UndefinedVariable(String),
    TypeMismatch(String),
    InvalidOperation(String),
    Return(Value),
}

impl VM {
    pub fn new() -> Self {
        Self {
            environment: Environment::new(),
            host_state: HashMap::new(),
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
    /// use ogham::vm::{VM, Value};
    ///
    /// let mut vm = VM::new();
    /// vm.inject_host_state("user_name".to_string(), Value::String("Alice".to_string()));
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
                // Events are not executed by the VM
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
        if let Some(Value::Function(func)) = self.environment.get(&func_name) {
            self.call_function(&func, &args)
        } else {
            Err(VMError::UndefinedVariable(format!(
                "Function '{}' not found",
                func_name
            )))
        }
    }

    /// Call a function with arguments
    fn call_function(&mut self, func: &Function, args: &[Value]) -> Result<Value, VMError> {
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

impl Default for VM {
    fn default() -> Self {
        Self::new()
    }
}
