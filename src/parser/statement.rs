use super::typed_bindings::{EventsDecl, HostStateDecl, RecordDecl};
use super::{block::*, expression::*, identifier::*, span::Span};

#[derive(PartialEq, Clone, Debug)]
pub enum Statement {
    Expression(ExpressionStatement),
    Declare(DeclareStatement),
    DeclareState(DeclareStateStatement),
    Assign(AssignStatement),
    Return(ReturnStatement),
    Conditional(ConditionalStatement),
    Log(LogStatement),
    ForLoop(ForLoopStatement),
    Import(ImportStatement),
    /// `record Foo { ... };` — top-level only.
    RecordDeclaration(RecordDecl),
    /// `host_state { ... };` — top-level only, at most one per module.
    HostStateDeclaration(HostStateDecl),
    /// `events { ... };` — top-level only, at most one per module.
    EventsDeclaration(EventsDecl),
    /// `on_mount { ... };` — Phase 2 lifecycle hook. Body runs
    /// once when the surrounding fn's call-stack path first
    /// becomes active. Legal only inside an fn body.
    OnMount(LifecycleHookStatement),
    /// `on_unmount { ... };` — Phase 2 lifecycle hook. Body
    /// runs at drain-time when the surrounding fn's call-stack
    /// path stops being visited. Legal only inside an fn body.
    OnUnmount(LifecycleHookStatement),
    /// `effect (dep_a, dep_b) { body }` — Phase 2 lifecycle.
    /// Body re-runs whenever any dep changes between renders.
    /// Empty deps `effect ()` legal (fires once on mount,
    /// cleanup on unmount). Cleanup blocks inside the body
    /// run before re-fire and at path unmount.
    Effect(EffectStatement),
    /// `cleanup { body }` — Phase 2. Legal ONLY inside an
    /// effect body. The compiler emits a compile-time error
    /// for cleanup statements outside an effect.
    Cleanup(LifecycleHookStatement),
}

impl Statement {
    pub fn new_expression(value: Expression, span: Span) -> Statement {
        Statement::Expression(ExpressionStatement { value, span })
    }

    pub fn new_declare(identifier: &Identifier, value: Expression, span: Span) -> Statement {
        Statement::Declare(DeclareStatement {
            identifier: identifier.clone(),
            value,
            span,
        })
    }

    pub fn new_declare_state(
        identifier: &Identifier,
        value: Expression,
        span: Span,
    ) -> Statement {
        Statement::DeclareState(DeclareStateStatement {
            identifier: identifier.clone(),
            value,
            span,
        })
    }

    pub fn new_assign(identifier: &Identifier, value: Expression, span: Span) -> Statement {
        Statement::Assign(AssignStatement {
            identifier: identifier.clone(),
            value,
            span,
        })
    }

    pub fn new_return(value: Option<Expression>, span: Span) -> Statement {
        Statement::Return(ReturnStatement { value, span })
    }

    pub fn new_conditional(
        branches: Vec<(Expression, Block)>,
        else_block: Option<Block>,
        span: Span,
    ) -> Statement {
        Statement::Conditional(ConditionalStatement {
            branches,
            else_block,
            span,
        })
    }

    pub fn new_log(value: Expression, span: Span) -> Statement {
        Statement::Log(LogStatement { value, span })
    }

    pub fn new_for_loop(
        variable: Identifier,
        range_start: Expression,
        range_end: Expression,
        body: Block,
        span: Span,
    ) -> Statement {
        Statement::ForLoop(ForLoopStatement {
            variable,
            range_start,
            range_end,
            body,
            span,
        })
    }

    pub fn new_import(names: Option<Vec<String>>, path: String, span: Span) -> Statement {
        Statement::Import(ImportStatement { names, path, span })
    }

    pub fn span(&self) -> Span {
        match self {
            Statement::Expression(s) => s.span,
            Statement::Declare(s) => s.span,
            Statement::DeclareState(s) => s.span,
            Statement::Assign(s) => s.span,
            Statement::Return(s) => s.span,
            Statement::Conditional(s) => s.span,
            Statement::Log(s) => s.span,
            Statement::ForLoop(s) => s.span,
            Statement::Import(s) => s.span,
            Statement::RecordDeclaration(s) => s.span,
            Statement::HostStateDeclaration(s) => s.span,
            Statement::EventsDeclaration(s) => s.span,
            Statement::OnMount(s) => s.span,
            Statement::OnUnmount(s) => s.span,
            Statement::Effect(s) => s.span,
            Statement::Cleanup(s) => s.span,
        }
    }
}

/// Phase 2 lifecycle hook block: `on_mount { ... }` or
/// `on_unmount { ... }`. The body is executed by the runtime at
/// well-defined points in the surrounding fn's path-lifetime.
/// See `LIFECYCLE_AND_PORTAL.md` §"Hook firing timing".
#[derive(PartialEq, Clone, Debug)]
pub struct LifecycleHookStatement {
    pub body: Block,
    pub span: Span,
}

/// Phase 2 effect: `effect (dep_a, dep_b) { body }`. Each dep
/// is an expression evaluated each render and compared via
/// structural equality to the previous render's value. The
/// body re-runs when any dep changes; the body may register
/// a `cleanup { ... }` that fires before the next re-run and
/// at path unmount.
///
/// `cleanup` blocks live inside the body's statement list as
/// `Statement::Expression` containing a synthetic
/// CleanupBlock-marker — for M2 we lift them to a dedicated
/// AST shape via the parser walker.
#[derive(PartialEq, Clone, Debug)]
pub struct EffectStatement {
    pub deps: Vec<Expression>,
    pub body: Block,
    pub span: Span,
}

#[derive(PartialEq, Clone, Debug)]
pub struct ExpressionStatement {
    pub value: Expression,
    pub span: Span,
}

impl ExpressionStatement {
    pub fn get_value(&self) -> Expression {
        self.value.clone()
    }
}

#[derive(PartialEq, Clone, Debug)]
pub struct ReturnStatement {
    pub value: Option<Expression>,
    pub span: Span,
}

impl ReturnStatement {
    pub fn get_value(&self) -> Option<Expression> {
        self.value.clone()
    }
}

#[derive(PartialEq, Clone, Debug)]
pub struct DeclareStatement {
    pub identifier: Identifier,
    pub value: Expression,
    pub span: Span,
}

impl DeclareStatement {
    pub fn get_identifier(&self) -> Identifier {
        self.identifier.clone()
    }

    pub fn get_identifier_value(&self) -> String {
        self.identifier.get()
    }

    pub fn get_value(&self) -> Expression {
        self.value.clone()
    }
}

#[derive(PartialEq, Clone, Debug)]
pub struct DeclareStateStatement {
    pub identifier: Identifier,
    pub value: Expression,
    pub span: Span,
}

impl DeclareStateStatement {
    pub fn get_identifier(&self) -> Identifier {
        self.identifier.clone()
    }

    pub fn get_identifier_value(&self) -> String {
        self.identifier.get()
    }

    pub fn get_value(&self) -> Expression {
        self.value.clone()
    }
}

#[derive(PartialEq, Clone, Debug)]
pub struct AssignStatement {
    pub identifier: Identifier,
    pub value: Expression,
    pub span: Span,
}

impl AssignStatement {
    pub fn get_identifier(&self) -> Identifier {
        self.identifier.clone()
    }

    pub fn get_identifier_value(&self) -> String {
        self.identifier.get()
    }

    pub fn get_value(&self) -> Expression {
        self.value.clone()
    }
}

#[derive(PartialEq, Clone, Debug)]
pub struct ConditionalStatement {
    pub branches: Vec<(Expression, Block)>,
    pub else_block: Option<Block>,
    pub span: Span,
}

impl ConditionalStatement {
    pub fn get_branches(&self) -> &Vec<(Expression, Block)> {
        &self.branches
    }

    pub fn get_else_block(&self) -> &Option<Block> {
        &self.else_block
    }
}

#[derive(PartialEq, Clone, Debug)]
pub struct LogStatement {
    pub value: Expression,
    pub span: Span,
}

impl LogStatement {
    pub fn get_value(&self) -> Expression {
        self.value.clone()
    }
}

#[derive(PartialEq, Clone, Debug)]
pub struct ForLoopStatement {
    pub variable: Identifier,
    pub range_start: Expression,
    pub range_end: Expression,
    pub body: Block,
    pub span: Span,
}

impl ForLoopStatement {
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
    pub names: Option<Vec<String>>,
    pub path: String,
    pub span: Span,
}

impl ImportStatement {
    pub fn get_names(&self) -> &Option<Vec<String>> {
        &self.names
    }

    pub fn get_path(&self) -> &str {
        &self.path
    }
}
