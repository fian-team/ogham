use ogham::{parser::Parser, scanner::Scanner};

#[test]
fn array_types_are_accepted_in_type_annotations() {
    let src = r#"
let main = fn (xs: int[], widgets: widget[][]): widget[] {
  return [];
};
"#;

    let mut scanner = Scanner::new(src.to_string());
    let tokens = scanner.scan();
    let mut parser = Parser::new(tokens);
    let module = parser.parse().expect("parse failed");

    // Find `main` and assert its return type was parsed with `[]`.
    let mut found = false;
    for stmt in &module.body.statement_list {
        if let ogham::parser::Statement::Declare(declare_stmt) = stmt {
            if declare_stmt.get_identifier_value() == "main" {
                found = true;
                let expr = declare_stmt.get_value();
                match expr {
                    ogham::parser::Expression::Literal(ogham::parser::Literal::Function(f)) => {
                        assert_eq!(f.return_type.get(), "widget[]");
                    }
                    other => panic!("expected main to be a function literal, got {:?}", other),
                }
            }
        }
    }

    assert!(found, "expected to find `main` declaration");
}
