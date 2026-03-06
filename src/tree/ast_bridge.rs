use crate::runtime::{error::VMError, value::Value, widget::RuntimeWidget, Runtime};
use crate::tree::{
    flex_widget::FlexWidget, style::*, svg_widget::SvgWidget, text_input_widget::TextInputWidget,
    text_widget::TextWidget, WidgetRef,
};
use crate::tree::event::Event;
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

/// Create a boxed event-listener closure from a `Value::BytecodeClosure`.
/// Returns `None` if the value is not a callable.
fn make_event_listener(
    value: &Value,
    runtime: &Arc<Mutex<Runtime>>,
    event_name: &str,
) -> Option<Box<dyn Fn(&Event)>> {
    match value {
        Value::BytecodeClosure(closure) => {
            let rt = runtime.clone();
            let closure = closure.clone();
            let ename = event_name.to_string();
            Some(Box::new(move |_event: &Event| {
                let result = rt.lock().expect("runtime lock poisoned").call_bytecode_closure(&closure, &[]);
                if let Err(err) = result {
                    eprintln!("[ogham] {} handler error: {:?}", ename, err);
                }
            }))
        }
        _ => None,
    }
}

/// Like `make_event_listener` but for handlers that receive a single `Value`
/// argument (e.g. `on_change`).
fn make_event_listener_with_arg(
    value: &Value,
    runtime: &Arc<Mutex<Runtime>>,
    event_name: &str,
) -> Option<Box<dyn Fn(&Event)>> {
    match value {
        Value::BytecodeClosure(closure) => {
            let rt = runtime.clone();
            let closure = closure.clone();
            let ename = event_name.to_string();
            Some(Box::new(move |event: &Event| {
                let arg = Value::String(event.value.clone().unwrap_or_default());
                let result = rt.lock().expect("runtime lock poisoned").call_bytecode_closure(&closure, &[arg]);
                if let Err(err) = result {
                    eprintln!("[ogham] {} handler error: {:?}", ename, err);
                }
            }))
        }
        _ => None,
    }
}

/// Register an event listener (no arguments) on a widget's listener map.
/// Does nothing if the property is absent; returns an error if it is present but not a closure.
fn register_event_listener(
    listeners: &mut HashMap<String, Vec<Box<dyn Fn(&Event)>>>,
    properties: &HashMap<String, Value>,
    runtime: &Arc<Mutex<Runtime>>,
    event_name: &str,
) -> Result<(), BridgeError> {
    if let Some(value) = properties.get(event_name) {
        if let Some(listener) = make_event_listener(value, runtime, event_name) {
            listeners
                .entry(event_name.to_string())
                .or_default()
                .push(listener);
        } else {
            return Err(BridgeError::InvalidPropertyType(
                event_name.to_string(),
                format!("Expected Closure, got {:?}", value),
            ));
        }
    }
    Ok(())
}

/// Register an event listener that receives a single `Value` argument (e.g. `on_change`).
/// Does nothing if the property is absent; returns an error if it is present but not a closure.
fn register_event_listener_with_arg(
    listeners: &mut HashMap<String, Vec<Box<dyn Fn(&Event)>>>,
    properties: &HashMap<String, Value>,
    runtime: &Arc<Mutex<Runtime>>,
    event_name: &str,
) -> Result<(), BridgeError> {
    if let Some(value) = properties.get(event_name) {
        if let Some(listener) = make_event_listener_with_arg(value, runtime, event_name) {
            listeners
                .entry(event_name.to_string())
                .or_default()
                .push(listener);
        } else {
            return Err(BridgeError::InvalidPropertyType(
                event_name.to_string(),
                format!("Expected Closure, got {:?}", value),
            ));
        }
    }
    Ok(())
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
            "textinput" => create_text_input_widget(runtime, runtime_widget),
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

fn optional_hover_style_map<'a>(
    parser_widget: &'a RuntimeWidget,
) -> Option<&'a HashMap<String, Value>> {
    match parser_widget.properties.get("hover_style") {
        Some(Value::Map(map)) => Some(map),
        _ => None,
    }
}

/// Clone the base `FlexStyle` and apply hover overrides from the map on top.
fn apply_flex_hover_style(base: &FlexStyle, style_map: &HashMap<String, Value>) -> FlexStyle {
    let mut s = base.clone();
    for (key, value) in style_map {
        match key.as_str() {
            "direction" => {
                if let Value::String(dir_str) = value {
                    s.direction = match dir_str.to_lowercase().as_str() {
                        "row" => Direction::Row,
                        "column" => Direction::Column,
                        "row_reverse" => Direction::RowReverse,
                        "column_reverse" => Direction::ColumnReverse,
                        _ => continue,
                    };
                }
            }
            "main_alignment" => {
                if let Value::String(v) = value {
                    if let Some(a) = parse_flex_alignment(v) {
                        s.main_alignment = a;
                    }
                }
            }
            "cross_alignment" => {
                if let Value::String(v) = value {
                    if let Some(a) = parse_flex_alignment(v) {
                        s.cross_alignment = a;
                    }
                }
            }
            "width" => {
                if let Some(sz) = parse_size_value(value) {
                    s.width = sz;
                }
            }
            "height" => {
                if let Some(sz) = parse_size_value(value) {
                    s.height = sz;
                }
            }
            "gap" => {
                if let Some(g) = value_to_f32(value) {
                    s.gap = g;
                }
            }
            "padding" => {
                if let Some(p) = parse_padding_value(value) {
                    s.padding = p;
                }
            }
            "margin" => {
                if let Some(m) = parse_margin_value(value) {
                    s.margin = m;
                }
            }
            "background_color" => {
                if let Some(color) = parse_color_value(value) {
                    s.background_color = Some(color);
                }
            }
            "border" => {
                if let Some(border) = parse_border_value(value) {
                    s.border = border;
                }
            }
            "corner_radius" => {
                if let Some(cr) = parse_corner_radii_value(value) {
                    s.corner_radii = cr;
                }
            }
            _ => {}
        }
    }
    s
}

/// Clone the base `TextStyle` and apply hover overrides from the map on top.
fn apply_text_hover_style(base: &TextStyle, style_map: &HashMap<String, Value>) -> TextStyle {
    let mut s = base.clone();
    for (key, value) in style_map {
        match key.as_str() {
            "size" => {
                if let Some(sz) = value_to_f32(value) {
                    s.size = sz;
                }
            }
            "color" => {
                if let Some(c) = parse_color_value(value) {
                    s.color = c;
                }
            }
            "align" => {
                if let Value::String(v) = value {
                    if let Some(a) = parse_text_align(v) {
                        s.align = a;
                    }
                }
            }
            "weight" => {
                if let Value::String(v) = value {
                    s.weight = match v.to_lowercase().as_str() {
                        "normal" => FontWeight::Normal,
                        "bold" => FontWeight::Bold,
                        "semi_bold" => FontWeight::SemiBold,
                        "light" => FontWeight::Light,
                        _ => continue,
                    };
                }
            }
            "decoration" => {
                if let Value::String(v) = value {
                    s.decoration = match v.to_lowercase().as_str() {
                        "none" => TextDecoration::None,
                        "underline" => TextDecoration::Underline,
                        "strikethrough" => TextDecoration::Strikethrough,
                        _ => continue,
                    };
                }
            }
            "width" => {
                if let Some(sz) = parse_size_value(value) {
                    s.width = sz;
                }
            }
            "height" => {
                if let Some(sz) = parse_size_value(value) {
                    s.height = sz;
                }
            }
            "font" => {
                if let Value::String(v) = value {
                    s.font = Some(v.clone());
                }
            }
            _ => {}
        }
    }
    s
}

pub(crate) fn parse_size_value(value: &Value) -> Option<Size> {
    match value {
        Value::Float(f) => Some(Size::Fixed(*f as f32)),
        Value::Integer(i) => Some(Size::Fixed(*i as f32)),
        Value::String(s) => match s.to_lowercase().as_str() {
            "shrink" => Some(Size::Shrink),
            "grow" => Some(Size::Grow(1.0)),
            _ => None,
        },
        _ => None,
    }
}

pub(crate) fn parse_padding_value(value: &Value) -> Option<Padding> {
    match value {
        Value::Float(f) => Some(Padding::all(*f as f32)),
        Value::Integer(i) => Some(Padding::all(*i as f32)),
        Value::Map(map) => {
            let top = get_float_from_map(map, "top", 0.0);
            let right = get_float_from_map(map, "right", 0.0);
            let bottom = get_float_from_map(map, "bottom", 0.0);
            let left = get_float_from_map(map, "left", 0.0);
            Some(Padding::new(top, right, bottom, left))
        }
        _ => None,
    }
}

pub(crate) fn parse_margin_value(value: &Value) -> Option<Margin> {
    match value {
        Value::Float(f) => Some(Margin::all(*f as f32)),
        Value::Integer(i) => Some(Margin::all(*i as f32)),
        Value::Map(map) => {
            let top = get_float_from_map(map, "top", 0.0);
            let right = get_float_from_map(map, "right", 0.0);
            let bottom = get_float_from_map(map, "bottom", 0.0);
            let left = get_float_from_map(map, "left", 0.0);
            Some(Margin::new(top, right, bottom, left))
        }
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
    register_event_listener(
        &mut flex_widget.event_listeners,
        &parser_widget.properties,
        runtime,
        "mouse_down",
    )?;

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
                        let dir = match dir_str.to_lowercase().as_str() {
                            "row" => Some(Direction::Row),
                            "column" => Some(Direction::Column),
                            "row_reverse" => Some(Direction::RowReverse),
                            "column_reverse" => Some(Direction::ColumnReverse),
                            _ => None,
                        };
                        if let Some(d) = dir {
                            style_builder = style_builder.direction(d);
                        }
                    }
                }
                "main_alignment" => {
                    if let Value::String(s) = value {
                        if let Some(alignment) = parse_flex_alignment(s) {
                            style_builder = style_builder.main_alignment(alignment);
                        }
                    }
                }
                "cross_alignment" => {
                    if let Value::String(s) = value {
                        if let Some(alignment) = parse_flex_alignment(s) {
                            style_builder = style_builder.cross_alignment(alignment);
                        }
                    }
                }
                "width" => {
                    if let Some(sz) = parse_size_value(value) {
                        style_builder = style_builder.width(sz);
                    }
                }
                "height" => {
                    if let Some(sz) = parse_size_value(value) {
                        style_builder = style_builder.height(sz);
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
                    if let Some(p) = parse_padding_value(value) {
                        style_builder = style_builder.padding(p);
                    }
                }
                "margin" => {
                    if let Some(m) = parse_margin_value(value) {
                        style_builder = style_builder.margin(m);
                    }
                }
                "background_color" => {
                    if let Some(color) = parse_color_value(value) {
                        style_builder = style_builder.background_color(color);
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
                _ => {}
            }
        }
    }

    flex_widget.style = style_builder.build();

    if let Some(hover_map) = optional_hover_style_map(parser_widget) {
        flex_widget.hover_style = Some(apply_flex_hover_style(&flex_widget.style, hover_map));
    }

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
                    if let Some(c) = parse_color_value(value) {
                        style_builder = style_builder.color(c);
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
                    if let Some(sz) = parse_size_value(value) {
                        width_override = Some(sz);
                    }
                }
                "height" => {
                    if let Some(sz) = parse_size_value(value) {
                        height_override = Some(sz);
                    }
                }
                "font" => {
                    if let Value::String(s) = value {
                        style_builder = style_builder.font(s.clone());
                    }
                }
                _ => {}
            }
        }
    }

    let mut text_widget = TextWidget::new(text);
    let mut style = style_builder.build();
    if let Some(w) = width_override {
        style.width = w;
    }
    if let Some(h) = height_override {
        style.height = h;
    }
    text_widget.style = style;

    if let Some(hover_map) = optional_hover_style_map(parser_widget) {
        text_widget.hover_style = Some(apply_text_hover_style(&text_widget.style, hover_map));
    }

    Ok(Arc::new(Mutex::new(text_widget)))
}

fn create_text_input_widget(
    runtime: &Arc<Mutex<Runtime>>,
    parser_widget: &RuntimeWidget,
) -> Result<WidgetRef, BridgeError> {
    let mut text_input = TextInputWidget::new();

    // Build styles
    let mut style_builder = FlexStyle::builder();
    let mut text_style_builder = TextStyle::builder();

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

    // Event handlers (e.g. `on_change: fn (value) { ... }`, `mouse_down: fn () { ... }`)
    register_event_listener_with_arg(
        &mut text_input.event_listeners,
        &parser_widget.properties,
        runtime,
        "on_change",
    )?;
    register_event_listener(
        &mut text_input.event_listeners,
        &parser_widget.properties,
        runtime,
        "mouse_down",
    )?;
    register_event_listener(
        &mut text_input.event_listeners,
        &parser_widget.properties,
        runtime,
        "mouse_up",
    )?;

    let style_props = optional_style_map(parser_widget);
    if let Some(style_map) = style_props {
        for (key, value) in style_map {
            match key.as_str() {
                // FlexStyle properties
                "width" => {
                    if let Some(sz) = parse_size_value(value) {
                        style_builder = style_builder.width(sz);
                    }
                }
                "height" => {
                    if let Some(sz) = parse_size_value(value) {
                        style_builder = style_builder.height(sz);
                    }
                }
                "padding" => {
                    if let Some(p) = parse_padding_value(value) {
                        style_builder = style_builder.padding(p);
                    }
                }
                "margin" => {
                    if let Some(m) = parse_margin_value(value) {
                        style_builder = style_builder.margin(m);
                    }
                }
                "background_color" => {
                    if let Some(color) = parse_color_value(value) {
                        style_builder = style_builder.background_color(color);
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
                "gap" => {
                    if let Value::Float(f) = value {
                        style_builder = style_builder.gap(*f as f32);
                    } else if let Value::Integer(i) = value {
                        style_builder = style_builder.gap(*i as f32);
                    }
                }
                "direction" => {
                    if let Value::String(dir_str) = value {
                        let dir = match dir_str.to_lowercase().as_str() {
                            "row" => Some(Direction::Row),
                            "column" => Some(Direction::Column),
                            "row_reverse" => Some(Direction::RowReverse),
                            "column_reverse" => Some(Direction::ColumnReverse),
                            _ => None,
                        };
                        if let Some(d) = dir {
                            style_builder = style_builder.direction(d);
                        }
                    }
                }
                // TextStyle properties
                "size" => {
                    if let Value::Float(f) = value {
                        text_style_builder = text_style_builder.size(*f as f32);
                    } else if let Value::Integer(i) = value {
                        text_style_builder = text_style_builder.size(*i as f32);
                    }
                }
                "color" => {
                    if let Some(c) = parse_color_value(value) {
                        text_style_builder = text_style_builder.color(c);
                    }
                }
                "weight" => {
                    if let Value::String(s) = value {
                        let w = match s.to_lowercase().as_str() {
                            "normal" => Some(FontWeight::Normal),
                            "bold" => Some(FontWeight::Bold),
                            "semi_bold" => Some(FontWeight::SemiBold),
                            "light" => Some(FontWeight::Light),
                            _ => None,
                        };
                        if let Some(w) = w {
                            text_style_builder = text_style_builder.weight(w);
                        }
                    }
                }
                "align" => {
                    if let Value::String(s) = value {
                        if let Some(align) = parse_text_align(s) {
                            text_style_builder = text_style_builder.align(align);
                        }
                    }
                }
                "decoration" => {
                    if let Value::String(s) = value {
                        let d = match s.to_lowercase().as_str() {
                            "none" => Some(TextDecoration::None),
                            "underline" => Some(TextDecoration::Underline),
                            "strikethrough" => Some(TextDecoration::Strikethrough),
                            _ => None,
                        };
                        if let Some(d) = d {
                            text_style_builder = text_style_builder.decoration(d);
                        }
                    }
                }
                "font" => {
                    if let Value::String(s) = value {
                        text_style_builder = text_style_builder.font(s.clone());
                    }
                }
                _ => {}
            }
        }
    }

    text_input.style = style_builder.build();
    text_input.text_style = text_style_builder.build();

    if let Some(hover_map) = optional_hover_style_map(parser_widget) {
        text_input.hover_style =
            Some(apply_flex_hover_style(&text_input.style, hover_map));
        text_input.hover_text_style =
            Some(apply_text_hover_style(&text_input.text_style, hover_map));
    }

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
    let color = style_props
        .and_then(|m| m.get("color"))
        .and_then(parse_color_value);

    let svg_widget = SvgWidget::new(path, width, height, color);
    Ok(Arc::new(Mutex::new(svg_widget)))
}

// Helper functions
pub(crate) fn get_float_from_map(map: &HashMap<String, Value>, key: &str, default: f32) -> f32 {
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

pub(crate) fn get_integer_from_map(map: &HashMap<String, Value>, key: &str, default: i32) -> i32 {
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

pub(crate) fn value_to_f32(value: &Value) -> Option<f32> {
    match value {
        Value::Float(f) => Some(*f as f32),
        Value::Integer(i) => Some(*i as f32),
        _ => None,
    }
}

pub(crate) fn parse_color_value(value: &Value) -> Option<Color> {
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

pub(crate) fn parse_border_style_value(value: &Value) -> Option<BorderStyle> {
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

pub(crate) fn parse_border_side_value(value: &Value) -> Option<BorderSide> {
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

pub(crate) fn parse_border_value(value: &Value) -> Option<Border> {
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

pub(crate) fn parse_corner_radii_value(value: &Value) -> Option<CornerRadii> {
    // Supported shapes:
    // - number => all corners
    // - { top_left, top_right, bottom_left, bottom_right }
    match value {
        Value::Float(_) | Value::Integer(_) => Some(CornerRadii::all(value_to_f32(value)?)),
        Value::Map(map) => {
            let tl = map.get("top_left").and_then(value_to_f32)?;
            let tr = map.get("top_right").and_then(value_to_f32)?;
            let bl = map.get("bottom_left").and_then(value_to_f32)?;
            let br = map.get("bottom_right").and_then(value_to_f32)?;
            Some(CornerRadii::new(tl, tr, bl, br))
        }
        _ => None,
    }
}

pub(crate) fn parse_text_align(value: &str) -> Option<TextAlign> {
    match value.to_lowercase().as_str() {
        "left" => Some(TextAlign::Left),
        "center" => Some(TextAlign::Center),
        "right" => Some(TextAlign::Right),
        _ => None,
    }
}

pub(crate) fn parse_flex_alignment(value: &str) -> Option<Alignment> {
    match value.to_lowercase().as_str() {
        "start" => Some(Alignment::Start),
        "center" => Some(Alignment::Center),
        "end" => Some(Alignment::End),
        "space_between" => Some(Alignment::SpaceBetween),
        "space_around" => Some(Alignment::SpaceAround),
        _ => None,
    }
}
