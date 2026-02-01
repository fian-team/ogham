use crate::runtime::{error::VMError, value::Value, widget::RuntimeWidget, Runtime};
use crate::tree::{
    flex_widget::FlexWidget, style::*, svg_widget::SvgWidget, text_input_widget::TextInputWidget,
    text_widget::TextWidget, WidgetRef,
};
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

/// Converts a Value::Widget from the Runtime to a WidgetRef for use in the UI
pub fn widget_value_to_widget_ref(
    runtime: &Arc<Mutex<Runtime>>,
    widget_value: &Value,
) -> Result<WidgetRef, BridgeError> {
    if let Value::Widget(runtime_widget) = widget_value {
        let identifier = runtime_widget.identifier.get().to_lowercase();

        match identifier.as_str() {
            "flex" => create_flex_widget(runtime, runtime_widget),
            "text" => create_text_widget(runtime, runtime_widget),
            "text_input" => create_text_input_widget(runtime, runtime_widget),
            "svg" => create_svg_widget(runtime, runtime_widget),
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

fn optional_style_map<'a>(parser_widget: &'a RuntimeWidget) -> Option<&'a HashMap<String, Value>> {
    match parser_widget.properties.get("style") {
        Some(Value::Map(map)) => Some(map),
        _ => None,
    }
}

fn create_flex_widget(
    runtime: &Arc<Mutex<Runtime>>,
    parser_widget: &RuntimeWidget,
) -> Result<WidgetRef, BridgeError> {
    let mut flex_widget = FlexWidget::new();

    // block_interactions: when false, clicks are only "handled" if a child or listener handled them
    if let Some(Value::Boolean(b)) = parser_widget.properties.get("block_interactions") {
        flex_widget.block_interactions = *b;
    }

    // Build style from properties
    let mut style_builder = FlexStyle::builder();

    let mut children = Vec::new();

    // Event handlers (e.g. `mouse_down: fn () { ... }`)
    if let Some(value) = parser_widget.properties.get("mouse_down") {
        match value {
            Value::Closure(closure) => {
                let runtime_for_handler = runtime.clone();
                let closure = closure.clone();
                let event_name = "mouse_down".to_string();
                flex_widget
                    .event_listeners
                    .entry(event_name.clone())
                    .or_default()
                    .push(Box::new(move |_event| {
                        let result = runtime_for_handler.lock().unwrap().call_closure(
                            &closure,
                            &[],
                            &format!("event_handler_{}", event_name),
                        );
                        if let Err(err) = result {
                            eprintln!("[ogham] {} handler error: {:?}", event_name, err);
                        }
                    }));
            }
            other => {
                return Err(BridgeError::InvalidPropertyType(
                    "mouse_down".to_string(),
                    format!("Expected Closure, got {:?}", other),
                ));
            }
        }
    }

    if let Some(value) = parser_widget.properties.get("children") {
        // Children can be:
        // 1. An array of widgets
        // 2. A map containing widgets (with numeric or string keys)
        // 3. A single widget
        if let Value::Array(children_array) = value {
            // Array of widgets - iterate in order
            for child_value in children_array {
                if let Value::Widget(child_widget) = child_value {
                    let child_ref =
                        widget_value_to_widget_ref(runtime, &Value::Widget(child_widget.clone()))?;
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
                    let child_ref =
                        widget_value_to_widget_ref(runtime, &Value::Widget(child_widget.clone()))?;
                    children.push(child_ref);
                }
            }
        } else if let Value::Widget(child_widget) = value {
            // Single child widget
            let child_ref =
                widget_value_to_widget_ref(runtime, &Value::Widget(child_widget.clone()))?;
            children.push(child_ref);
        }
    }

    let style_props = optional_style_map(parser_widget);
    if let Some(style_map) = style_props {
        for (key, value) in style_map {
            // Properties have already been evaluated by the Runtime when the widget expression was evaluated.
            match key.as_str() {
                "direction" => {
                    if let Value::String(dir_str) = value {
                        match dir_str.to_lowercase().as_str() {
                            "row" => style_builder = style_builder.row(),
                            "column" => style_builder = style_builder.column(),
                            "row_reverse" => style_builder = style_builder.row_reverse(),
                            "column_reverse" => style_builder = style_builder.column_reverse(),
                            _ => {}
                        }
                    }
                }
                "main_alignment" => {
                    if let Value::String(s) = value {
                        if let Some(alignment) = parse_flex_alignment(&s) {
                            style_builder = style_builder.main_alignment(alignment);
                        }
                    }
                }
                "cross_alignment" => {
                    if let Value::String(s) = value {
                        if let Some(alignment) = parse_flex_alignment(&s) {
                            style_builder = style_builder.cross_alignment(alignment);
                        }
                    }
                }
                "width" => {
                    if let Value::Float(f) = value {
                        style_builder = style_builder.width_fixed(*f as f32);
                    } else if let Value::Integer(i) = value {
                        style_builder = style_builder.width_fixed(*i as f32);
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
                        style_builder = style_builder.height_fixed(*f as f32);
                    } else if let Value::Integer(i) = value {
                        style_builder = style_builder.height_fixed(*i as f32);
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
                        style_builder = style_builder.gap(*f as f32);
                    } else if let Value::Integer(i) = value {
                        style_builder = style_builder.gap(*i as f32);
                    }
                }
                "padding" => {
                    if let Value::Float(f) = value {
                        style_builder = style_builder.padding(Padding::all(*f as f32));
                    } else if let Value::Integer(i) = value {
                        style_builder = style_builder.padding(Padding::all(*i as f32));
                    } else if let Value::Map(padding_map) = value {
                        let top = get_float_from_map(&padding_map, "top", 0.0);
                        let right = get_float_from_map(&padding_map, "right", 0.0);
                        let bottom = get_float_from_map(&padding_map, "bottom", 0.0);
                        let left = get_float_from_map(&padding_map, "left", 0.0);
                        style_builder =
                            style_builder.padding(Padding::new(top, right, bottom, left));
                    }
                }
                "margin" => {
                    if let Value::Float(f) = value {
                        style_builder = style_builder.margin(Margin::all(*f as f32));
                    } else if let Value::Integer(i) = value {
                        style_builder = style_builder.margin(Margin::all(*i as f32));
                    } else if let Value::Map(margin_map) = value {
                        let top = get_float_from_map(&margin_map, "top", 0.0);
                        let right = get_float_from_map(&margin_map, "right", 0.0);
                        let bottom = get_float_from_map(&margin_map, "bottom", 0.0);
                        let left = get_float_from_map(&margin_map, "left", 0.0);
                        style_builder = style_builder.margin(Margin::new(top, right, bottom, left));
                    }
                }
                "background_color" => {
                    if let Value::Map(color_map) = value {
                        let r = get_integer_from_map(color_map, "r", 0) as u8;
                        let g = get_integer_from_map(color_map, "g", 0) as u8;
                        let b = get_integer_from_map(color_map, "b", 0) as u8;
                        let a = get_integer_from_map(color_map, "a", 255) as u8;
                        style_builder = style_builder.background(r, g, b, a);
                    }
                }
                "border" => {
                    if let Some(border) = parse_border_value(value) {
                        style_builder = style_builder.border(border);
                    }
                }
                "corner_radius" => {
                    if let Some(corner_radii) = parse_corner_radii_value(value) {
                        style_builder = style_builder.corner_radii(corner_radii);
                    }
                }
                _ => {
                    // If a value is a widget, treat it as a child (named-slot style).
                    if let Value::Widget(child_widget) = value {
                        let child_ref = widget_value_to_widget_ref(
                            runtime,
                            &Value::Widget(child_widget.clone()),
                        )?;
                        children.push(child_ref);
                    }
                }
            }
        }
    }

    flex_widget.style = style_builder.build();

    // println!("Flex widget style: {:?}", flex_widget.style);

    // Add children
    for child in children {
        flex_widget.add_child(child);
    }

    Ok(Arc::new(Mutex::new(flex_widget)))
}

fn create_text_widget(
    _runtime: &Arc<Mutex<Runtime>>,
    parser_widget: &RuntimeWidget,
) -> Result<WidgetRef, BridgeError> {
    let style_props = optional_style_map(parser_widget);

    // Get text property (required)
    let text_value = parser_widget
        .properties
        .get("text")
        .ok_or_else(|| BridgeError::MissingProperty("text".to_string()))?;
    let text = match text_value {
        Value::String(s) => s.clone(),
        Value::Integer(i) => i.to_string(),
        Value::Float(f) => f.to_string(),
        Value::Boolean(b) => b.to_string(),
        _ => {
            return Err(BridgeError::InvalidPropertyType(
                "text".to_string(),
                "Expected String, Integer, Float, or Boolean".to_string(),
            ));
        }
    };

    // Build text style (also carries minimal width/height sizing for Text widgets).
    // Default to shrink along both axes (and thus along the parent's main axis).
    let mut style_builder = TextStyle::builder();
    let mut width_override: Option<Size> = Some(Size::Shrink);
    let mut height_override: Option<Size> = Some(Size::Shrink);

    if let Some(style_map) = style_props {
        for (key, value) in style_map {
            match key.as_str() {
                "size" => {
                    if let Value::Float(f) = value {
                        style_builder = style_builder.size(*f as f32);
                    } else if let Value::Integer(i) = value {
                        style_builder = style_builder.size(*i as f32);
                    }
                }
                "color" => {
                    if let Value::Map(color_map) = value {
                        let r = get_integer_from_map(color_map, "r", 0) as u8;
                        let g = get_integer_from_map(color_map, "g", 0) as u8;
                        let b = get_integer_from_map(color_map, "b", 0) as u8;
                        let a = get_integer_from_map(color_map, "a", 255) as u8;
                        style_builder = style_builder.color_rgba(r, g, b, a);
                    }
                }
                "align" => {
                    if let Value::String(s) = value {
                        if let Some(align) = parse_text_align(&s) {
                            style_builder = style_builder.align(align);
                        }
                    }
                }
                "width" => {
                    if let Value::Float(f) = value {
                        width_override = Some(Size::Fixed(*f as f32));
                    } else if let Value::Integer(i) = value {
                        width_override = Some(Size::Fixed(*i as f32));
                    } else if let Value::String(s) = value {
                        match s.to_lowercase().as_str() {
                            "shrink" => width_override = Some(Size::Shrink),
                            "grow" => width_override = Some(Size::Grow(1.0)),
                            _ => {}
                        }
                    }
                }
                "height" => {
                    if let Value::Float(f) = value {
                        height_override = Some(Size::Fixed(*f as f32));
                    } else if let Value::Integer(i) = value {
                        height_override = Some(Size::Fixed(*i as f32));
                    } else if let Value::String(s) = value {
                        match s.to_lowercase().as_str() {
                            "shrink" => height_override = Some(Size::Shrink),
                            "grow" => height_override = Some(Size::Grow(1.0)),
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
        }
    }

    // Build full style; preserve all fields (including alignment), while keeping existing
    // behavior of defaulting via TextStyle.
    let mut text_widget = TextWidget::new(text);
    let mut style = style_builder.build();
    if let Some(w) = width_override {
        style.width = w;
    }
    if let Some(h) = height_override {
        style.height = h;
    }
    text_widget.style = style;
    Ok(Arc::new(Mutex::new(text_widget)))
}

fn create_text_input_widget(
    _runtime: &Arc<Mutex<Runtime>>,
    parser_widget: &RuntimeWidget,
) -> Result<WidgetRef, BridgeError> {
    let mut text_input = TextInputWidget::new();

    // Build style
    let mut style_builder = FlexStyle::builder();

    // `value` is required and lives at the root (not in `style`).
    let value_value = parser_widget
        .properties
        .get("value")
        .ok_or_else(|| BridgeError::MissingProperty("value".to_string()))?;
    match value_value {
        Value::String(s) => text_input.set_value(s.clone()),
        _ => {
            return Err(BridgeError::InvalidPropertyType(
                "value".to_string(),
                "Expected String".to_string(),
            ))
        }
    }

    let style_props = optional_style_map(parser_widget);
    if let Some(style_map) = style_props {
        for (key, value) in style_map {
            match key.as_str() {
                "width" => {
                    if let Value::Float(f) = value {
                        style_builder = style_builder.width_fixed(*f as f32);
                    } else if let Value::Integer(i) = value {
                        style_builder = style_builder.width_fixed(*i as f32);
                    }
                }
                "height" => {
                    if let Value::Float(f) = value {
                        style_builder = style_builder.height_fixed(*f as f32);
                    } else if let Value::Integer(i) = value {
                        style_builder = style_builder.height_fixed(*i as f32);
                    }
                }
                _ => {}
            }
        }
    }

    text_input.style = style_builder.build();

    Ok(Arc::new(Mutex::new(text_input)))
}

fn create_svg_widget(
    _runtime: &Arc<Mutex<Runtime>>,
    parser_widget: &RuntimeWidget,
) -> Result<WidgetRef, BridgeError> {
    let style_props = optional_style_map(parser_widget);

    // Get required properties
    let path_value = parser_widget
        .properties
        .get("path")
        .ok_or_else(|| BridgeError::MissingProperty("path".to_string()))?;
    let path = match path_value {
        Value::String(s) => s.clone(),
        _ => {
            return Err(BridgeError::InvalidPropertyType(
                "path".to_string(),
                "Expected String".to_string(),
            ))
        }
    };

    let width_value = parser_widget
        .properties
        .get("width")
        .ok_or_else(|| BridgeError::MissingProperty("width".to_string()))?;
    let width = match width_value {
        Value::Float(f) => *f as f32,
        Value::Integer(i) => *i as f32,
        _ => {
            return Err(BridgeError::InvalidPropertyType(
                "width".to_string(),
                "Expected Float or Integer".to_string(),
            ))
        }
    };

    let height_value = parser_widget
        .properties
        .get("height")
        .ok_or_else(|| BridgeError::MissingProperty("height".to_string()))?;
    let height = match height_value {
        Value::Float(f) => *f as f32,
        Value::Integer(i) => *i as f32,
        _ => {
            return Err(BridgeError::InvalidPropertyType(
                "height".to_string(),
                "Expected Float or Integer".to_string(),
            ))
        }
    };

    // Optional color
    let mut color = None;
    if let Some(style_map) = style_props {
        if let Some(color_value) = style_map.get("color") {
            if let Value::Map(color_map) = color_value {
                let r = get_integer_from_map(color_map, "r", 0) as u8;
                let g = get_integer_from_map(color_map, "g", 0) as u8;
                let b = get_integer_from_map(color_map, "b", 0) as u8;
                let a = get_integer_from_map(color_map, "a", 255) as u8;
                color = Some(Color::new(r, g, b, a));
            }
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

fn value_to_f32(value: &Value) -> Option<f32> {
    match value {
        Value::Float(f) => Some(*f as f32),
        Value::Integer(i) => Some(*i as f32),
        _ => None,
    }
}

fn parse_color_value(value: &Value) -> Option<Color> {
    match value {
        Value::Map(map) => {
            let r = get_integer_from_map(map, "r", 0).clamp(0, 255) as u8;
            let g = get_integer_from_map(map, "g", 0).clamp(0, 255) as u8;
            let b = get_integer_from_map(map, "b", 0).clamp(0, 255) as u8;
            let a = get_integer_from_map(map, "a", 255).clamp(0, 255) as u8;
            Some(Color::new(r, g, b, a))
        }
        _ => None,
    }
}

fn parse_border_style_value(value: &Value) -> Option<BorderStyle> {
    match value {
        Value::String(s) => match s.to_lowercase().as_str() {
            "solid" => Some(BorderStyle::Solid),
            "dashed" => Some(BorderStyle::Dashed),
            "dotted" => Some(BorderStyle::Dotted),
            _ => None,
        },
        _ => None,
    }
}

fn parse_border_side_value(value: &Value) -> Option<BorderSide> {
    // Supported shapes:
    // - number => width, default black, solid
    // - { width, color, style }
    match value {
        Value::Float(_) | Value::Integer(_) => {
            let width = value_to_f32(value)?;
            Some(BorderSide::new(
                width,
                Color::new(0, 0, 0, 255),
                BorderStyle::Solid,
            ))
        }
        Value::Map(map) => {
            let width = map.get("width").and_then(value_to_f32).unwrap_or(0.0);

            let color = map
                .get("color")
                .and_then(parse_color_value)
                .unwrap_or(Color::new(0, 0, 0, 255));

            let style = map
                .get("style")
                .and_then(parse_border_style_value)
                .unwrap_or(BorderStyle::Solid);

            Some(BorderSide::new(width, color, style))
        }
        _ => None,
    }
}

fn parse_border_value(value: &Value) -> Option<Border> {
    // Supported shapes:
    // - number => uniform border width on all sides (black, solid)
    // - { width, color, style } => uniform side definition
    // - { top, right, bottom, left } => per-side definitions (each can be number or {width,color,style})
    match value {
        Value::Float(_) | Value::Integer(_) => {
            let side = parse_border_side_value(value)?;
            Some(Border::new(side.clone(), side.clone(), side.clone(), side))
        }
        Value::Map(map) => {
            // If it looks like a side definition (has width/color/style), treat as uniform
            let looks_like_uniform =
                map.contains_key("width") || map.contains_key("color") || map.contains_key("style");

            if looks_like_uniform {
                let side = parse_border_side_value(value)?;
                return Some(Border::new(side.clone(), side.clone(), side.clone(), side));
            }

            let mut top = None;
            let mut right = None;
            let mut bottom = None;
            let mut left = None;

            if let Some(v) = map.get("top") {
                top = parse_border_side_value(v);
            }
            if let Some(v) = map.get("right") {
                right = parse_border_side_value(v);
            }
            if let Some(v) = map.get("bottom") {
                bottom = parse_border_side_value(v);
            }
            if let Some(v) = map.get("left") {
                left = parse_border_side_value(v);
            }

            let top_side = top.unwrap_or(BorderSide::identity());
            let bottom_side = bottom.unwrap_or(BorderSide::identity());
            let left_side = left.unwrap_or(BorderSide::identity());
            let right_side = right.unwrap_or(BorderSide::identity());

            Some(Border::new(top_side, right_side, bottom_side, left_side))
        }
        _ => None,
    }
}

fn parse_corner_radii_value(value: &Value) -> Option<CornerRadii> {
    // Supported shapes:
    // - number => all corners
    // - { all } => all corners
    // - { top_left, top_right, bottom_left, bottom_right }
    match value {
        Value::Float(_) | Value::Integer(_) => Some(CornerRadii::all(value_to_f32(value)?)),
        Value::Map(map) => {
            if let Some(all) = map.get("all").and_then(value_to_f32) {
                return Some(CornerRadii::all(all));
            }
            let tl = map.get("top_left").and_then(value_to_f32)?;
            let tr = map.get("top_right").and_then(value_to_f32)?;
            let bl = map.get("bottom_left").and_then(value_to_f32)?;
            let br = map.get("bottom_right").and_then(value_to_f32)?;
            Some(CornerRadii::new(tl, tr, bl, br))
        }
        _ => None,
    }
}

fn parse_text_align(value: &str) -> Option<TextAlign> {
    match value.to_lowercase().as_str() {
        "left" => Some(TextAlign::Left),
        "center" => Some(TextAlign::Center),
        "right" => Some(TextAlign::Right),
        _ => None,
    }
}

fn parse_flex_alignment(value: &str) -> Option<Alignment> {
    match value.to_lowercase().as_str() {
        "start" => Some(Alignment::Start),
        "center" => Some(Alignment::Center),
        "end" => Some(Alignment::End),
        "space_between" => Some(Alignment::SpaceBetween),
        "space_around" => Some(Alignment::SpaceAround),
        _ => None,
    }
}
