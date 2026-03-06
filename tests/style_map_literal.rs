//! Tests that Flex widget style properties with nested map literals
//! (background_color, padding, margin) parse and evaluate correctly.

mod common;

use common::parse_module;
use ogham::parser::{Expression, Literal, Statement};
use ogham::runtime::value::Value;

const FLEX_STYLE_WITH_NESTED_MAPS: &str = r#"
Flex {
  style: {
    width: "grow",
    height: "shrink",
    direction: "row",
    main_alignment: "center",
    cross_alignment: "center",
    background_color: { r: 50, g: 90, b: 60, a: 255 },
    padding: { top: 8, right: 12, bottom: 8, left: 12 },
    margin: { top: 12, right: 0, bottom: 0, left: 0 },
    corner_radius: 8,
  }
}
"#;

#[test]
fn parse_flex_style_with_nested_map_literals() {
    let module = parse_module(FLEX_STYLE_WITH_NESTED_MAPS);
    let statements = &module.body.statement_list;
    assert!(
        !statements.is_empty(),
        "module should have at least one statement"
    );

    let ret = match &statements[0] {
        Statement::Return(r) => r.0.as_ref(),
        _ => panic!("expected implicit return statement"),
    };
    let ret = ret.expect("return should have a value");

    let widget = match ret {
        Expression::Widget(w) => w,
        _ => panic!("expected Widget expression, got {:?}", ret),
    };

    assert_eq!(widget.identifier.get(), "Flex");

    // style property must be present and be a map literal
    let style_expr = widget
        .properties
        .get("style")
        .expect("Flex should have a 'style' property");
    let style_map = match style_expr {
        Expression::Literal(Literal::Map(m)) => m,
        _ => panic!("style value should be a map literal, got {:?}", style_expr),
    };

    // background_color: map with r, g, b, a
    let bg = style_map
        .properties
        .get("background_color")
        .expect("style should have background_color");
    let bg_map = match bg {
        Expression::Literal(Literal::Map(m)) => m,
        _ => panic!("background_color should be a map literal, got {:?}", bg),
    };
    assert!(bg_map.properties.contains_key("r"));
    assert!(bg_map.properties.contains_key("g"));
    assert!(bg_map.properties.contains_key("b"));
    assert!(bg_map.properties.contains_key("a"));

    // padding: map with top, right, bottom, left
    let pad = style_map
        .properties
        .get("padding")
        .expect("style should have padding");
    let pad_map = match pad {
        Expression::Literal(Literal::Map(m)) => m,
        _ => panic!("padding should be a map literal, got {:?}", pad),
    };
    assert!(pad_map.properties.contains_key("top"));
    assert!(pad_map.properties.contains_key("right"));
    assert!(pad_map.properties.contains_key("bottom"));
    assert!(pad_map.properties.contains_key("left"));

    // margin: map with top, right, bottom, left
    let margin = style_map
        .properties
        .get("margin")
        .expect("style should have margin");
    let margin_map = match margin {
        Expression::Literal(Literal::Map(m)) => m,
        _ => panic!("margin should be a map literal, got {:?}", margin),
    };
    assert!(margin_map.properties.contains_key("top"));
    assert!(margin_map.properties.contains_key("right"));
    assert!(margin_map.properties.contains_key("bottom"));
    assert!(margin_map.properties.contains_key("left"));
}

/// Full program that defines main() returning the Flex with nested style maps.
const FULL_PROGRAM_WITH_STYLE_MAPS: &str = r#"
let main = fn () {
  Flex {
    style: {
      width: "grow",
      height: "shrink",
      direction: "row",
      main_alignment: "center",
      cross_alignment: "center",
      background_color: { r: 50, g: 90, b: 60, a: 255 },
      padding: { top: 8, right: 12, bottom: 8, left: 12 },
      margin: { top: 12, right: 0, bottom: 0, left: 0 },
      corner_radius: 8,
    }
  }
};
"#;

#[test]
fn pipeline_parse_evaluate_bridge_flex_style_maps() {
    let mut runtime = ogham::runtime::from_source(FULL_PROGRAM_WITH_STYLE_MAPS, None)
        .expect("from_source should succeed");
    let module = runtime.get_module().expect("module").clone();
    let widget_value = runtime
        .execute_module(&module)
        .expect("execute_module should succeed");

    let Value::Widget(ref rw) = widget_value else {
        panic!("main() should return a Widget, got {:?}", widget_value);
    };
    assert_eq!(rw.identifier.get(), "Flex");

    let style_value = rw.properties.get("style").expect("Flex should have style");
    let Value::Map(style_map) = style_value else {
        panic!("style should be Value::Map, got {:?}", style_value);
    };

    // background_color should be a map and applied by the bridge
    let bg = style_map
        .get("background_color")
        .expect("style map should have background_color");
    let Value::Map(bg_map) = bg else {
        panic!("background_color should be Value::Map, got {:?}", bg);
    };
    assert_eq!(bg_map.get("r"), Some(&Value::Integer(50)));
    assert_eq!(bg_map.get("g"), Some(&Value::Integer(90)));
    assert_eq!(bg_map.get("b"), Some(&Value::Integer(60)));
    assert_eq!(bg_map.get("a"), Some(&Value::Integer(255)));

    // padding and margin as maps
    let padding = style_map
        .get("padding")
        .expect("style map should have padding");
    let Value::Map(pad_map) = padding else {
        panic!("padding should be Value::Map, got {:?}", padding);
    };
    assert_eq!(pad_map.get("top"), Some(&Value::Integer(8)));
    assert_eq!(pad_map.get("right"), Some(&Value::Integer(12)));
    assert_eq!(pad_map.get("bottom"), Some(&Value::Integer(8)));
    assert_eq!(pad_map.get("left"), Some(&Value::Integer(12)));

    let margin = style_map
        .get("margin")
        .expect("style map should have margin");
    let Value::Map(margin_map) = margin else {
        panic!("margin should be Value::Map, got {:?}", margin);
    };
    assert_eq!(margin_map.get("top"), Some(&Value::Integer(12)));
    assert_eq!(margin_map.get("right"), Some(&Value::Integer(0)));
    assert_eq!(margin_map.get("bottom"), Some(&Value::Integer(0)));
    assert_eq!(margin_map.get("left"), Some(&Value::Integer(0)));

    // Bridge: widget_value_to_widget_ref should succeed and apply style
    let runtime_ref = std::sync::Arc::new(std::sync::Mutex::new(runtime));
    let widget_ref =
        ogham::tree::ast_bridge::widget_value_to_widget_ref(&runtime_ref, &widget_value)
            .expect("bridge should succeed");
    // Downcast to FlexWidget and check style was applied
    let guard = widget_ref.lock().expect("widget lock poisoned");
    let flex = guard
        .downcast_ref::<ogham::tree::flex_widget::FlexWidget>()
        .expect("should be FlexWidget");
    assert!(
        flex.style.background_color.is_some(),
        "background_color should be set by bridge"
    );
    let color = flex.style.background_color.unwrap();
    assert_eq!(color.r, 50);
    assert_eq!(color.g, 90);
    assert_eq!(color.b, 60);
    assert_eq!(color.a, 255);
    assert_eq!(flex.style.padding.get_top(), 8.0);
    assert_eq!(flex.style.padding.get_right(), 12.0);
    assert_eq!(flex.style.padding.get_bottom(), 8.0);
    assert_eq!(flex.style.padding.get_left(), 12.0);
    assert_eq!(flex.style.margin.get_top(), 12.0);
    assert_eq!(flex.style.margin.get_right(), 0.0);
    assert_eq!(flex.style.margin.get_bottom(), 0.0);
    assert_eq!(flex.style.margin.get_left(), 0.0);
}
