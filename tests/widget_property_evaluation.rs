use ui::{
    parser::Parser,
    scanner::Scanner,
    tree::ast_bridge,
    tree::{flex_widget::FlexWidget, text_widget::TextWidget, Widget},
    vm::VM,
};

#[test]
fn widget_properties_are_evaluated_when_widget_expression_is_evaluated() {
    // This mirrors the failing scenario:
    // Text { text: text } where `text` is a local variable.
    let src = r#"
let main = fn () {
  let text = "Hello, world!";
  Flex {
    children: [
      Text {
        text: text,
      }
    ],
  }
};
"#;

    let mut scanner = Scanner::new(src.to_string());
    let tokens = scanner.scan();
    let mut parser = Parser::new(tokens);
    let module = parser.parse().expect("parse failed");

    let mut vm = VM::new();
    let value = vm.execute_module(&module).expect("vm execution failed");
    let root = ast_bridge::widget_value_to_widget_ref(&mut vm, &value).expect("bridge failed");

    let root_guard = root.lock().unwrap();
    let flex = root_guard
        .downcast_ref::<FlexWidget>()
        .expect("expected Flex root");
    let children = flex.get_children();
    assert_eq!(children.len(), 1);

    let child_guard = children[0].lock().unwrap();
    let text_widget = child_guard
        .downcast_ref::<TextWidget>()
        .expect("expected Text child");
    assert_eq!(text_widget.text, "Hello, world!");
}

#[test]
fn flex_style_map_is_applied_via_style_property() {
    let src = r#"
let main = fn () {
  let page_style = {
    width: "grow",
    height: "grow",
    background_color: {
      r: 200,
      g: 200,
      b: 255,
      a: 255,
    },
  };
  Flex {
    children: [
      Text {
        text: "Hello, world!",
      }
    ],
    style: page_style,
  }
};
"#;

    let mut scanner = Scanner::new(src.to_string());
    let tokens = scanner.scan();
    let mut parser = Parser::new(tokens);
    let module = parser.parse().expect("parse failed");

    let mut vm = VM::new();
    let value = vm.execute_module(&module).expect("vm execution failed");
    let root = ast_bridge::widget_value_to_widget_ref(&mut vm, &value).expect("bridge failed");

    let root_guard = root.lock().unwrap();
    let flex = root_guard
        .downcast_ref::<FlexWidget>()
        .expect("expected Flex root");

    // Basic sanity check: style got applied from the nested `style` map.
    assert!(matches!(flex.style.width, ui::tree::style::Size::Grow(_)));
    assert!(matches!(flex.style.height, ui::tree::style::Size::Grow(_)));
    let bg = flex
        .style
        .background_color
        .as_ref()
        .expect("expected background");
    assert_eq!(bg.r, 200);
    assert_eq!(bg.g, 200);
    assert_eq!(bg.b, 255);
    assert_eq!(bg.a, 255);
}
