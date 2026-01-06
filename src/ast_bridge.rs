use super::ast_vm::{VMError, Value, VM};
use super::flex_widget::FlexWidget;
use super::style::*;
use super::svg_widget::SvgWidget;
use super::text_input_widget::TextInputWidget;
use super::text_widget::TextWidget;
use super::WidgetRef;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

#[derive(Debug)]
pub enum BridgeError {
    InvalidWidgetType(String),
    MissingProperty(String),
    InvalidPropertyType(String, String),
    VMError(VMError),
}

impl From<VMError> for BridgeError {
    fn from(err: VMError) -> Self {
        BridgeError::VMError(err)
    }
}

/// Converts a Value::Widget from the VM to a WidgetRef for use in the UI
pub fn widget_value_to_widget_ref(
    vm: &mut VM,
    widget_value: &Value,
) -> Result<WidgetRef, BridgeError> {
    if let Value::Widget(parser_widget) = widget_value {
        let identifier = parser_widget.identifier.get().to_lowercase();

        match identifier.as_str() {
            "flex" | "box" => create_flex_widget(vm, parser_widget),
            "text" => create_text_widget(vm, parser_widget),
            "textinput" | "text_input" => create_text_input_widget(vm, parser_widget),
            "svg" => create_svg_widget(vm, parser_widget),
            _ => Err(BridgeError::InvalidWidgetType(format!(
                "Unknown widget type: {}",
                identifier
            ))),
        }
    } else {
        Err(BridgeError::InvalidPropertyType(
            "widget".to_string(),
            format!("Expected Widget, got {:?}", widget_value),
        ))
    }
}

fn create_flex_widget(
    vm: &mut VM,
    parser_widget: &super::parser::Widget,
) -> Result<WidgetRef, BridgeError> {
    let mut flex_widget = FlexWidget::new();

    // Build style from properties
    let mut style_builder = FlexStyle::builder();

    // Evaluate all properties
    let mut children = Vec::new();

    for (key, expr) in &parser_widget.properties {
        // Check if this property is a widget expression (potential child)
        // Evaluate the expression to see what it is
        let value = vm.evaluate_expression(expr)?;

        match key.as_str() {
            "children" => {
                // Children can be:
                // 1. An array of widgets
                // 2. A map containing widgets (with numeric or string keys)
                // 3. A single widget
                if let Value::Array(children_array) = value {
                    // Array of widgets - iterate in order
                    for child_value in children_array {
                        if let Value::Widget(child_widget) = child_value {
                            let child_ref =
                                widget_value_to_widget_ref(vm, &Value::Widget(child_widget))?;
                            children.push(child_ref);
                        }
                    }
                } else if let Value::Map(children_map) = value {
                    // Try to extract widgets from the map
                    // If keys are numeric strings, sort them; otherwise just iterate
                    let mut sorted_entries: Vec<_> = children_map.iter().collect();
                    sorted_entries.sort_by(|a, b| {
                        // Try to parse keys as numbers for ordering
                        if let (Ok(a_num), Ok(b_num)) = (a.0.parse::<i32>(), b.0.parse::<i32>()) {
                            a_num.cmp(&b_num)
                        } else {
                            a.0.cmp(b.0)
                        }
                    });

                    for (_, child_value) in sorted_entries {
                        if let Value::Widget(child_widget) = child_value {
                            let child_ref = widget_value_to_widget_ref(
                                vm,
                                &Value::Widget(child_widget.clone()),
                            )?;
                            children.push(child_ref);
                        }
                    }
                } else if let Value::Widget(child_widget) = value {
                    // Single child widget
                    let child_ref = widget_value_to_widget_ref(vm, &Value::Widget(child_widget))?;
                    children.push(child_ref);
                }
            }
            "direction" => {
                if let Value::String(dir_str) = value {
                    match dir_str.to_lowercase().as_str() {
                        "row" => style_builder = style_builder.row(),
                        "column" => style_builder = style_builder.column(),
                        "rowreverse" | "row_reverse" => style_builder = style_builder.row_reverse(),
                        "columnreverse" | "column_reverse" => {
                            style_builder = style_builder.column_reverse()
                        }
                        _ => {}
                    }
                }
            }
            "width" => {
                if let Value::Float(f) = value {
                    style_builder = style_builder.width_fixed(f as f32);
                } else if let Value::Integer(i) = value {
                    style_builder = style_builder.width_fixed(i as f32);
                } else if let Value::String(s) = value {
                    match s.to_lowercase().as_str() {
                        "shrink" => style_builder = style_builder.width_shrink(),
                        "grow" => style_builder = style_builder.width_grow(1.0),
                        _ => {}
                    }
                }
            }
            "height" => {
                if let Value::Float(f) = value {
                    style_builder = style_builder.height_fixed(f as f32);
                } else if let Value::Integer(i) = value {
                    style_builder = style_builder.height_fixed(i as f32);
                } else if let Value::String(s) = value {
                    match s.to_lowercase().as_str() {
                        "shrink" => style_builder = style_builder.height_shrink(),
                        "grow" => style_builder = style_builder.height_grow(1.0),
                        _ => {}
                    }
                }
            }
            "gap" => {
                if let Value::Float(f) = value {
                    style_builder = style_builder.gap(f as f32);
                } else if let Value::Integer(i) = value {
                    style_builder = style_builder.gap(i as f32);
                }
            }
            "padding" => {
                if let Value::Float(f) = value {
                    style_builder = style_builder.padding(Padding::all(f as f32));
                } else if let Value::Integer(i) = value {
                    style_builder = style_builder.padding(Padding::all(i as f32));
                } else if let Value::Map(padding_map) = value {
                    let top = get_float_from_map(&padding_map, "top", 0.0);
                    let right = get_float_from_map(&padding_map, "right", 0.0);
                    let bottom = get_float_from_map(&padding_map, "bottom", 0.0);
                    let left = get_float_from_map(&padding_map, "left", 0.0);
                    style_builder = style_builder.padding(Padding::new(top, right, bottom, left));
                }
            }
            "margin" => {
                if let Value::Float(f) = value {
                    style_builder = style_builder.margin(Margin::all(f as f32));
                } else if let Value::Integer(i) = value {
                    style_builder = style_builder.margin(Margin::all(i as f32));
                } else if let Value::Map(margin_map) = value {
                    let top = get_float_from_map(&margin_map, "top", 0.0);
                    let right = get_float_from_map(&margin_map, "right", 0.0);
                    let bottom = get_float_from_map(&margin_map, "bottom", 0.0);
                    let left = get_float_from_map(&margin_map, "left", 0.0);
                    style_builder = style_builder.margin(Margin::new(top, right, bottom, left));
                }
            }
            "background_color" | "backgroundcolor" => {
                if let Value::Map(color_map) = value {
                    let r = get_integer_from_map(&color_map, "r", 0) as u8;
                    let g = get_integer_from_map(&color_map, "g", 0) as u8;
                    let b = get_integer_from_map(&color_map, "b", 0) as u8;
                    let a = get_integer_from_map(&color_map, "a", 255) as u8;
                    style_builder = style_builder.background(r, g, b, a);
                }
            }
            _ => {
                // Check if this property value is a widget (could be a child passed as a property)
                // Only do this if the key doesn't match a known style property
                if !matches!(
                    key.as_str(),
                    "direction"
                        | "width"
                        | "height"
                        | "gap"
                        | "padding"
                        | "margin"
                        | "background_color"
                        | "backgroundcolor"
                        | "style"
                ) {
                    if let Value::Widget(child_widget) = value {
                        // This property is a widget, treat it as a child
                        let child_ref =
                            widget_value_to_widget_ref(vm, &Value::Widget(child_widget))?;
                        children.push(child_ref);
                    }
                }
            }
        }
    }

    flex_widget.style = style_builder.build();

    println!("Flex widget style: {:?}", flex_widget.style);

    // Add children
    for child in children {
        flex_widget.add_child(child);
    }

    Ok(Arc::new(Mutex::new(flex_widget)))
}

fn create_text_widget(
    vm: &mut VM,
    parser_widget: &super::parser::Widget,
) -> Result<WidgetRef, BridgeError> {
    // Get text property (required)
    let text_expr = parser_widget
        .properties
        .get("text")
        .ok_or_else(|| BridgeError::MissingProperty("text".to_string()))?;
    let text_value = vm.evaluate_expression(text_expr)?;
    let text = match text_value {
        Value::String(s) => s,
        Value::Integer(i) => i.to_string(),
        Value::Float(f) => f.to_string(),
        Value::Boolean(b) => b.to_string(),
        _ => {
            return Err(BridgeError::InvalidPropertyType(
                "text".to_string(),
                "Expected String, Integer, Float, or Boolean".to_string(),
            ))
        }
    };

    // Build text style
    let mut style_builder = TextStyle::builder();

    for (key, expr) in &parser_widget.properties {
        if key == "text" {
            continue; // Already handled
        }

        let value = vm.evaluate_expression(expr)?;

        match key.as_str() {
            "size" | "font_size" | "fontsize" => {
                if let Value::Float(f) = value {
                    style_builder = style_builder.size(f as f32);
                } else if let Value::Integer(i) = value {
                    style_builder = style_builder.size(i as f32);
                }
            }
            "color" | "text_color" | "textcolor" => {
                if let Value::Map(color_map) = value {
                    let r = get_integer_from_map(&color_map, "r", 0) as u8;
                    let g = get_integer_from_map(&color_map, "g", 0) as u8;
                    let b = get_integer_from_map(&color_map, "b", 0) as u8;
                    let a = get_integer_from_map(&color_map, "a", 255) as u8;
                    style_builder = style_builder.color_rgba(r, g, b, a);
                }
            }
            _ => {}
        }
    }

    let text_widget = TextWidget::with_color(text, style_builder.build().color);
    Ok(Arc::new(Mutex::new(text_widget)))
}

fn create_text_input_widget(
    vm: &mut VM,
    parser_widget: &super::parser::Widget,
) -> Result<WidgetRef, BridgeError> {
    let mut text_input = TextInputWidget::new();

    // Build style
    let mut style_builder = FlexStyle::builder();

    for (key, expr) in &parser_widget.properties {
        let value = vm.evaluate_expression(expr)?;

        match key.as_str() {
            "value" => {
                if let Value::String(s) = value {
                    text_input.set_value(s);
                }
            }
            "width" => {
                if let Value::Float(f) = value {
                    style_builder = style_builder.width_fixed(f as f32);
                } else if let Value::Integer(i) = value {
                    style_builder = style_builder.width_fixed(i as f32);
                }
            }
            "height" => {
                if let Value::Float(f) = value {
                    style_builder = style_builder.height_fixed(f as f32);
                } else if let Value::Integer(i) = value {
                    style_builder = style_builder.height_fixed(i as f32);
                }
            }
            _ => {}
        }
    }

    text_input.style = style_builder.build();

    Ok(Arc::new(Mutex::new(text_input)))
}

fn create_svg_widget(
    vm: &mut VM,
    parser_widget: &super::parser::Widget,
) -> Result<WidgetRef, BridgeError> {
    // Get required properties
    let path_expr = parser_widget
        .properties
        .get("path")
        .ok_or_else(|| BridgeError::MissingProperty("path".to_string()))?;
    let path_value = vm.evaluate_expression(path_expr)?;
    let path = match path_value {
        Value::String(s) => s,
        _ => {
            return Err(BridgeError::InvalidPropertyType(
                "path".to_string(),
                "Expected String".to_string(),
            ))
        }
    };

    let width_expr = parser_widget
        .properties
        .get("width")
        .ok_or_else(|| BridgeError::MissingProperty("width".to_string()))?;
    let width_value = vm.evaluate_expression(width_expr)?;
    let width = match width_value {
        Value::Float(f) => f as f32,
        Value::Integer(i) => i as f32,
        _ => {
            return Err(BridgeError::InvalidPropertyType(
                "width".to_string(),
                "Expected Float or Integer".to_string(),
            ))
        }
    };

    let height_expr = parser_widget
        .properties
        .get("height")
        .ok_or_else(|| BridgeError::MissingProperty("height".to_string()))?;
    let height_value = vm.evaluate_expression(height_expr)?;
    let height = match height_value {
        Value::Float(f) => f as f32,
        Value::Integer(i) => i as f32,
        _ => {
            return Err(BridgeError::InvalidPropertyType(
                "height".to_string(),
                "Expected Float or Integer".to_string(),
            ))
        }
    };

    // Optional color
    let mut color = None;
    if let Some(color_expr) = parser_widget.properties.get("color") {
        let color_value = vm.evaluate_expression(color_expr)?;
        if let Value::Map(color_map) = color_value {
            let r = get_integer_from_map(&color_map, "r", 0) as u8;
            let g = get_integer_from_map(&color_map, "g", 0) as u8;
            let b = get_integer_from_map(&color_map, "b", 0) as u8;
            let a = get_integer_from_map(&color_map, "a", 255) as u8;
            color = Some(Color::new(r, g, b, a));
        }
    }

    let svg_widget = SvgWidget::new(path, width, height, color);
    Ok(Arc::new(Mutex::new(svg_widget)))
}

// Helper functions
fn get_float_from_map(map: &HashMap<String, Value>, key: &str, default: f32) -> f32 {
    if let Some(value) = map.get(key) {
        match value {
            Value::Float(f) => *f as f32,
            Value::Integer(i) => *i as f32,
            _ => default,
        }
    } else {
        default
    }
}

fn get_integer_from_map(map: &HashMap<String, Value>, key: &str, default: i32) -> i32 {
    if let Some(value) = map.get(key) {
        match value {
            Value::Integer(i) => *i,
            Value::Float(f) => *f as i32,
            _ => default,
        }
    } else {
        default
    }
}
