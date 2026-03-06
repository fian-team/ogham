use ogham::parser::{Function, Parser};
use ogham::runtime::value::Value;
use ogham::scanner::Scanner;

/// Full pipeline: scan/parse/compile/execute, returning the result Value.
pub fn run(source: &str) -> Value {
    let mut runtime =
        ogham::runtime::from_source(source, None).expect("from_source should succeed");
    let module = runtime.get_module().expect("module").clone();
    runtime
        .execute_module(&module)
        .expect("execute_module should succeed")
}

/// Wrap an expression in a main function and execute it.
pub fn eval(expr: &str) -> Value {
    let wrapped = format!("let main = fn () {{ {} }};", expr);
    run(&wrapped)
}

/// Execute from a file path.
pub fn execute_file(path: &str) -> Value {
    let mut runtime =
        ogham::runtime::from_file(path, None).expect("from_file should succeed");
    let module = runtime.get_module().expect("module").clone();
    runtime
        .execute_module(&module)
        .expect("execute_module should succeed")
}

/// Parse source into a module AST.
pub fn parse_module(source: &str) -> Function {
    let mut scanner = Scanner::new(source.to_string());
    let tokens = scanner.scan();
    let mut parser = Parser::new(tokens);
    parser.parse().expect("parse should succeed")
}
