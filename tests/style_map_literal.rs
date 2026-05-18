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
        Statement::Return(r) => r.value.as_ref(),
        _ => panic!("expected implicit return statement"),
    };
    let ret = ret.expect("return should have a value");

    let widget = match ret {
        Expression::Widget(w) => w,
        _ => panic!("expected Widget expression, got {:?}", ret),
    };

    assert_eq!(widget.identifier.get(), "Flex");

    fn find_prop<'a>(
        props: &'a [(ogham::parser::Identifier, Expression)],
        key: &str,
    ) -> Option<&'a Expression> {
        props.iter().find(|(k, _)| k.get() == key).map(|(_, v)| v)
    }

    fn has_prop(props: &[(ogham::parser::Identifier, Expression)], key: &str) -> bool {
        props.iter().any(|(k, _)| k.get() == key)
    }

    // style property must be present and be a map literal
    let style_expr = find_prop(&widget.properties, "style")
        .expect("Flex should have a 'style' property");
    let style_map = match style_expr {
        Expression::Literal(Literal::Map(m)) => m,
        _ => panic!("style value should be a map literal, got {:?}", style_expr),
    };

    // background_color: map with r, g, b, a
    let bg = find_prop(&style_map.properties, "background_color")
        .expect("style should have background_color");
    let bg_map = match bg {
        Expression::Literal(Literal::Map(m)) => m,
        _ => panic!("background_color should be a map literal, got {:?}", bg),
    };
    assert!(has_prop(&bg_map.properties, "r"));
    assert!(has_prop(&bg_map.properties, "g"));
    assert!(has_prop(&bg_map.properties, "b"));
    assert!(has_prop(&bg_map.properties, "a"));

    // padding: map with top, right, bottom, left
    let pad = find_prop(&style_map.properties, "padding")
        .expect("style should have padding");
    let pad_map = match pad {
        Expression::Literal(Literal::Map(m)) => m,
        _ => panic!("padding should be a map literal, got {:?}", pad),
    };
    assert!(has_prop(&pad_map.properties, "top"));
    assert!(has_prop(&pad_map.properties, "right"));
    assert!(has_prop(&pad_map.properties, "bottom"));
    assert!(has_prop(&pad_map.properties, "left"));

    // margin: map with top, right, bottom, left
    let margin = find_prop(&style_map.properties, "margin")
        .expect("style should have margin");
    let margin_map = match margin {
        Expression::Literal(Literal::Map(m)) => m,
        _ => panic!("margin should be a map literal, got {:?}", margin),
    };
    assert!(has_prop(&margin_map.properties, "top"));
    assert!(has_prop(&margin_map.properties, "right"));
    assert!(has_prop(&margin_map.properties, "bottom"));
    assert!(has_prop(&margin_map.properties, "left"));
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
    let mut runtime = ogham::runtime::Runtime::from_source(FULL_PROGRAM_WITH_STYLE_MAPS, None)
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
    let registry = ogham::widget::builder::WidgetRegistry::with_defaults();
    let widget_ref =
        ogham::widget::builder::widget_value_to_widget_ref(&registry, &runtime_ref, &widget_value)
            .expect("bridge should succeed");
    // Downcast to FlexWidget and check style was applied
    let guard = widget_ref.lock().expect("widget lock poisoned");
    let flex = guard
        .downcast_ref::<ogham::widget::flex_widget::FlexWidget>()
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

// ---- Corner shape + inner glow parser coverage -----------------------------
//
// `corner_radius` and `corner_chamfer` both write into the unified
// `corners: Corners` field. The merge rules are documented on
// `merge_round_corners` / `merge_chamfer_corners` in `builder.rs`:
// chamfer wins on conflict, sharp inputs are no-ops, so mixing the
// two in any source order yields the same result. These tests pin
// that behaviour through the full parse → bridge pipeline.

fn build_flex_style(source: &str) -> ogham::widget::style::FlexStyle {
    let mut runtime = ogham::runtime::Runtime::from_source(source, None).unwrap();
    let module = runtime.get_module().unwrap().clone();
    let widget_value = runtime.execute_module(&module).unwrap();
    let runtime_ref = std::sync::Arc::new(std::sync::Mutex::new(runtime));
    let registry = ogham::widget::builder::WidgetRegistry::with_defaults();
    let widget_ref = ogham::widget::builder::widget_value_to_widget_ref(
        &registry,
        &runtime_ref,
        &widget_value,
    )
    .unwrap();
    let guard = widget_ref.lock().unwrap();
    guard
        .downcast_ref::<ogham::widget::flex_widget::FlexWidget>()
        .unwrap()
        .style
        .clone()
}

#[test]
fn corner_radius_shorthand_applies_round_to_all_four() {
    use ogham::widget::style::CornerShape;
    let style = build_flex_style(
        r#"let main = fn () { Flex { style: { corner_radius: 8 } } };"#,
    );
    assert_eq!(style.corners.top_left, CornerShape::Round(8.0));
    assert_eq!(style.corners.top_right, CornerShape::Round(8.0));
    assert_eq!(style.corners.bottom_left, CornerShape::Round(8.0));
    assert_eq!(style.corners.bottom_right, CornerShape::Round(8.0));
}

#[test]
fn corner_radius_zero_normalises_to_sharp() {
    use ogham::widget::style::CornerShape;
    let style = build_flex_style(
        r#"let main = fn () {
              Flex { style: {
                corner_radius: { top_left: 4, top_right: 0, bottom_left: 0, bottom_right: 4 }
              } }
           };"#,
    );
    assert_eq!(style.corners.top_left, CornerShape::Round(4.0));
    assert_eq!(style.corners.top_right, CornerShape::Sharp);
    assert_eq!(style.corners.bottom_left, CornerShape::Sharp);
    assert_eq!(style.corners.bottom_right, CornerShape::Round(4.0));
}

#[test]
fn corner_chamfer_shorthand_applies_chamfer_to_all_four() {
    use ogham::widget::style::CornerShape;
    let style = build_flex_style(
        r#"let main = fn () { Flex { style: { corner_chamfer: 6 } } };"#,
    );
    assert_eq!(style.corners.top_left, CornerShape::Chamfer(6.0));
    assert_eq!(style.corners.top_right, CornerShape::Chamfer(6.0));
    assert_eq!(style.corners.bottom_left, CornerShape::Chamfer(6.0));
    assert_eq!(style.corners.bottom_right, CornerShape::Chamfer(6.0));
}

#[test]
fn corner_radius_and_chamfer_merge_per_corner() {
    // The mix-and-match pattern: TL+BR rounded, TR+BL chamfered.
    // Source-order should not matter because the merge is
    // commutative — chamfer wins on conflict, sharp inputs are
    // no-ops.
    use ogham::widget::style::CornerShape;
    let style = build_flex_style(
        r#"let main = fn () {
              Flex { style: {
                corner_radius:  { top_left: 8, top_right: 0, bottom_left: 0, bottom_right: 8 },
                corner_chamfer: { top_left: 0, top_right: 6, bottom_left: 6, bottom_right: 0 }
              } }
           };"#,
    );
    assert_eq!(style.corners.top_left, CornerShape::Round(8.0));
    assert_eq!(style.corners.top_right, CornerShape::Chamfer(6.0));
    assert_eq!(style.corners.bottom_left, CornerShape::Chamfer(6.0));
    assert_eq!(style.corners.bottom_right, CornerShape::Round(8.0));
}

#[test]
fn corner_chamfer_wins_when_both_specify_the_same_corner() {
    // If both parsers target the same corner with non-zero sizes,
    // chamfer must win regardless of which key was iterated first.
    use ogham::widget::style::CornerShape;
    let style = build_flex_style(
        r#"let main = fn () {
              Flex { style: {
                corner_radius:  { top_left: 8, top_right: 8, bottom_left: 8, bottom_right: 8 },
                corner_chamfer: { top_left: 4, top_right: 0, bottom_left: 0, bottom_right: 0 }
              } }
           };"#,
    );
    assert_eq!(style.corners.top_left, CornerShape::Chamfer(4.0));
    assert_eq!(style.corners.top_right, CornerShape::Round(8.0));
    assert_eq!(style.corners.bottom_left, CornerShape::Round(8.0));
    assert_eq!(style.corners.bottom_right, CornerShape::Round(8.0));
}

#[test]
fn inner_glow_parses_color_blur_and_spread() {
    let style = build_flex_style(
        r#"let main = fn () {
              Flex { style: {
                inner_glow: {
                  color: { r: 110, g: 220, b: 240, a: 200 },
                  blur: 8,
                  spread: 1
                }
              } }
           };"#,
    );
    let glow = style.inner_glow.expect("inner_glow should be Some");
    assert_eq!(glow.color.r, 110);
    assert_eq!(glow.color.g, 220);
    assert_eq!(glow.color.b, 240);
    assert_eq!(glow.color.a, 200);
    assert_eq!(glow.blur, 8.0);
    assert_eq!(glow.spread, 1.0);
    assert!(glow.is_active());
}

#[test]
fn inner_glow_spread_defaults_to_zero_when_omitted() {
    let style = build_flex_style(
        r#"let main = fn () {
              Flex { style: {
                inner_glow: { color: { r: 0, g: 0, b: 0, a: 255 }, blur: 4 }
              } }
           };"#,
    );
    let glow = style.inner_glow.unwrap();
    assert_eq!(glow.blur, 4.0);
    assert_eq!(glow.spread, 0.0);
}
