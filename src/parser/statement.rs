use super::{block::*, expression::*, function::*, identifier::*};

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
    ForLoop(ForLoopStatement),
    Import(ImportStatement),
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

    pub fn new_conditional(
        branches: Vec<(Expression, Block)>,
        else_block: Option<Block>,
    ) -> Statement {
        Statement::Conditional(ConditionalStatement::new(branches, else_block))
    }

    pub fn new_log(value: Expression) -> Statement {
        Statement::Log(LogStatement::new(value))
    }

    pub fn new_for_loop(
        variable: Identifier,
        range_start: Expression,
        range_end: Expression,
        body: Block,
    ) -> Statement {
        Statement::ForLoop(ForLoopStatement::new(
            variable,
            range_start,
            range_end,
            body,
        ))
    }

    pub fn new_import(names: Option<Vec<String>>, path: String) -> Statement {
        Statement::Import(ImportStatement::new(names, path))
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
    branches: Vec<(Expression, Block)>,
    else_block: Option<Block>,
}

impl ConditionalStatement {
    pub fn new(
        branches: Vec<(Expression, Block)>,
        else_block: Option<Block>,
    ) -> ConditionalStatement {
        ConditionalStatement {
            branches,
            else_block,
        }
    }

    pub fn get_branches(&self) -> &Vec<(Expression, Block)> {
        &self.branches
    }

    pub fn get_else_block(&self) -> &Option<Block> {
        &self.else_block
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

#[derive(PartialEq, Clone, Debug)]
pub struct ForLoopStatement {
    variable: Identifier,
    range_start: Expression,
    range_end: Expression,
    body: Block,
}

impl ForLoopStatement {
    pub fn new(
        variable: Identifier,
        range_start: Expression,
        range_end: Expression,
        body: Block,
    ) -> ForLoopStatement {
        ForLoopStatement {
            variable,
            range_start,
            range_end,
            body,
        }
    }

    pub fn get_variable(&self) -> Identifier {
        self.variable.clone()
    }

    pub fn get_range_start(&self) -> Expression {
        self.range_start.clone()
    }

    pub fn get_range_end(&self) -> Expression {
        self.range_end.clone()
    }

    pub fn get_body(&self) -> Block {
        self.body.clone()
    }
}

#[derive(PartialEq, Clone, Debug)]
pub struct ImportStatement {
    /// Some(names) for named import, None for "import all"
    names: Option<Vec<String>>,
    path: String,
}

impl ImportStatement {
    pub fn new(names: Option<Vec<String>>, path: String) -> ImportStatement {
        ImportStatement { names, path }
    }

    pub fn get_names(&self) -> &Option<Vec<String>> {
        &self.names
    }

    pub fn get_path(&self) -> &str {
        &self.path
    }
}
