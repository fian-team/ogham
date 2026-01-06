use super::{super::parser::*, block::*, function::*, identifier::*};

#[derive(PartialEq, Clone, Debug)]
pub enum Statement {
  Expression(ExpressionStatement),
  Declare(DeclareStatement),
  DeclareState(DeclareStateStatement),
  Assign(AssignStatement),
  Return(ReturnStatement),
  Event(EventStatement),
  Conditional(ConditionalStatement),
  Log(LogStatement),
}

impl Statement {
  pub fn new_expression(value: Expression) -> Statement {
    Statement::Expression(ExpressionStatement::new(value))
  }

  pub fn new_declare(identifier: &Identifier, value: Expression) -> Statement {
    Statement::Declare(DeclareStatement::new(identifier, value))
  }

  pub fn new_declare_state(identifier: &Identifier, value: Expression) -> Statement {
    Statement::DeclareState(DeclareStateStatement::new(identifier, value))
  }

  pub fn new_assign(identifier: &Identifier, value: Expression) -> Statement {
    Statement::Assign(AssignStatement::new(identifier, value))
  }

  pub fn new_return(value: Option<Expression>) -> Statement {
    Statement::Return(ReturnStatement::new(value))
  }

  pub fn new_event(identifier: &Identifier, value: Function) -> Statement {
    Statement::Event(EventStatement::new(identifier, value))
  }

  pub fn new_conditional(condition: Expression, block: Block) -> Statement {
    Statement::Conditional(ConditionalStatement::new(condition, block))
  }

  pub fn new_log(value: Expression) -> Statement {
    Statement::Log(LogStatement::new(value))
  }
}

#[derive(PartialEq, Clone, Debug)]
pub struct ExpressionStatement(pub Expression);

impl ExpressionStatement {
  pub fn new(value: Expression) -> ExpressionStatement {
    ExpressionStatement(value)
  }

  pub fn get_value(&self) -> Expression {
    self.0.clone()
  }
}

#[derive(PartialEq, Clone, Debug)]
pub struct ReturnStatement(pub Option<Expression>);

impl ReturnStatement {
  pub fn new(value: Option<Expression>) -> ReturnStatement {
    ReturnStatement(value)
  }

  pub fn get_value(&self) -> Option<Expression> {
    self.0.clone()
  }
}

#[derive(PartialEq, Clone, Debug)]
pub struct DeclareStatement(Identifier, Expression);

impl DeclareStatement {
  pub fn new(identifier: &Identifier, value: Expression) -> DeclareStatement {
    DeclareStatement(identifier.clone(), value)
  }

  pub fn get_identifier(&self) -> Identifier {
    self.0.clone()
  }

  pub fn get_identifier_value(&self) -> String {
    self.0.get()
  }

  pub fn get_value(&self) -> Expression {
    self.1.clone()
  }
}

#[derive(PartialEq, Clone, Debug)]
pub struct DeclareStateStatement(Identifier, Expression);

impl DeclareStateStatement {
  pub fn new(identifier: &Identifier, value: Expression) -> DeclareStateStatement {
    DeclareStateStatement(identifier.clone(), value)
  }

  pub fn get_identifier(&self) -> Identifier {
    self.0.clone()
  }

  pub fn get_identifier_value(&self) -> String {
    self.0.get()
  }

  pub fn get_value(&self) -> Expression {
    self.1.clone()
  }
}

#[derive(PartialEq, Clone, Debug)]
pub struct AssignStatement(Identifier, Expression);

impl AssignStatement {
  pub fn new(identifier: &Identifier, value: Expression) -> AssignStatement {
    AssignStatement(identifier.clone(), value)
  }

  pub fn get_identifier(&self) -> Identifier {
    self.0.clone()
  }

  pub fn get_identifier_value(&self) -> String {
    self.0.get()
  }

  pub fn get_value(&self) -> Expression {
    self.1.clone()
  }
}

#[derive(PartialEq, Clone, Debug)]
pub struct EventStatement(pub Identifier, pub Function);

impl EventStatement {
  pub fn new(identifier: &Identifier, value: Function) -> EventStatement {
    EventStatement(identifier.clone(), value)
  }

  pub fn get_identifier(&self) -> Identifier {
    self.0.clone()
  }

  pub fn get_identifier_value(&self) -> String {
    self.0.get()
  }

  pub fn get_value(&self) -> Function {
    self.1.clone()
  }
}

#[derive(PartialEq, Clone, Debug)]
pub struct ConditionalStatement {
  condition: Expression,
  block: Block,
}

impl ConditionalStatement {
  pub fn new(condition: Expression, block: Block) -> ConditionalStatement {
    ConditionalStatement { condition, block }
  }

  pub fn get_condition(&self) -> Expression {
    self.condition.clone()
  }

  pub fn get_block(&self) -> Block {
    self.block.clone()
  }
}

#[derive(PartialEq, Clone, Debug)]
pub struct LogStatement(Expression);

impl LogStatement {
  pub fn new(value: Expression) -> LogStatement {
    LogStatement(value)
  }

  pub fn get_value(&self) -> Expression {
    self.0.clone()
  }
}
