use crate::runtime::{descriptor::WidgetDescriptor, error::VMError, value::Value, Runtime};
use crate::widget::animation::{TransitionConfig, TransitionSet};
use crate::widget::{
    flex_widget::FlexWidget, grid_widget::{GridPlacement, GridStyle, GridWidget},
    image_widget::ImageWidget, presence_widget::PresenceWidget, style::*,
    svg_widget::SvgWidget, text_input_widget::TextInputWidget, text_widget::TextWidget, WidgetRef,
};
use crate::widget::event::Event;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// A factory function that constructs a `WidgetRef` from a runtime descriptor.
pub type WidgetFactory = Arc<
    dyn Fn(&WidgetRegistry, &Arc<Mutex<Runtime>>, &WidgetDescriptor) -> Result<WidgetRef, BridgeError>
        + Send
        + Sync,
>;

/// Maps widget type names (lowercased) to their factory functions. Populated
/// with the built-in types by default; host applications can register
/// additional widgets via [`RuntimeConfig::with_widget`].
#[derive(Clone, Default)]
pub struct WidgetRegistry {
    pub(crate) factories: HashMap<String, WidgetFactory>,
}

impl WidgetRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a registry pre-populated with every built-in widget type.
    pub fn with_defaults() -> Self {
        let mut reg = Self::new();
        reg.register("flex", |reg, rt, desc| create_flex_widget(reg, rt, desc));
        reg.register("text", |_reg, rt, desc| create_text_widget(rt, desc));
        reg.register("textinput", |_reg, rt, desc| {
            create_text_input_widget(rt, desc)
        });
        reg.register("svg", |_reg, rt, desc| create_svg_widget(rt, desc));
        reg.register("image", |_reg, rt, desc| create_image_widget(rt, desc));
        reg.register("grid", |reg, rt, desc| create_grid_widget(reg, rt, desc));
        reg.register("presence", |reg, rt, desc| create_presence_widget(reg, rt, desc));
        reg
    }

    /// Register a widget factory under the given type name. Names are
    /// lowercased so that lookups are case-insensitive.
    pub fn register<S, F>(&mut self, name: S, factory: F)
    where
        S: Into<String>,
        F: Fn(&WidgetRegistry, &Arc<Mutex<Runtime>>, &WidgetDescriptor) -> Result<WidgetRef, BridgeError>
            + Send
            + Sync
            + 'static,
    {
        self.factories
            .insert(name.into().to_lowercase(), Arc::new(factory));
    }

    pub fn get(&self, name: &str) -> Option<&WidgetFactory> {
        self.factories.get(name)
    }
}

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

/// Converts a `Value::Widget` from the Runtime to a `WidgetRef` for use in the
/// UI. The `registry` is used to look up the factory for the widget's type name.
pub fn widget_value_to_widget_ref(
    registry: &WidgetRegistry,
    runtime: &Arc<Mutex<Runtime>>,
    widget_value: &Value,
) -> Result<WidgetRef, BridgeError> {
    if let Value::Widget(runtime_widget) = widget_value {
        let identifier = runtime_widget.identifier.get().to_lowercase();
        let factory = registry.get(&identifier).ok_or_else(|| {
            BridgeError::InvalidWidgetType(format!("Unknown widget type: {}", identifier))
        })?;
        factory(registry, runtime, runtime_widget)
    } else {
        Err(BridgeError::InvalidPropertyType(
            "widget".to_string(),
            format!("Expected Widget, got {:?}", widget_value),
        ))
    }
}

fn optional_style_map<'a>(descriptor: &'a WidgetDescriptor) -> Option<&'a HashMap<String, Value>> {
    match descriptor.properties.get("style") {
        Some(Value::Map(map)) => Some(map),
        _ => None,
    }
}

fn optional_hover_style_map<'a>(
    descriptor: &'a WidgetDescriptor,
) -> Option<&'a HashMap<String, Value>> {
    match descriptor.properties.get("hover_style") {
        Some(Value::Map(map)) => Some(map),
        _ => None,
    }
}

fn optional_map_property<'a>(
    descriptor: &'a WidgetDescriptor,
    name: &str,
) -> Option<&'a HashMap<String, Value>> {
    match descriptor.properties.get(name) {
        Some(Value::Map(map)) => Some(map),
        _ => None,
    }
}

/// Apply flex style properties from a map onto a mutable `FlexStyle`.
fn apply_flex_style_from_map(style: &mut FlexStyle, map: &HashMap<String, Value>) {
    for (key, value) in map {
        match key.as_str() {
            "direction" => {
                if let Value::String(dir_str) = value {
                    match dir_str.to_lowercase().as_str() {
                        "row" => style.direction = Direction::Row,
                        "column" => style.direction = Direction::Column,
                        "row_reverse" => style.direction = Direction::RowReverse,
                        "column_reverse" => style.direction = Direction::ColumnReverse,
                        _ => {}
                    }
                }
            }
            "main_alignment" => {
                if let Value::String(v) = value {
                    if let Some(a) = parse_flex_alignment(v) {
                        style.main_alignment = a;
                    }
                }
            }
            "cross_alignment" => {
                if let Value::String(v) = value {
                    if let Some(a) = parse_flex_alignment(v) {
                        style.cross_alignment = a;
                    }
                }
            }
            "width" => {
                if let Some(sz) = parse_size_value(value) {
                    style.width = sz;
                }
            }
            "height" => {
                if let Some(sz) = parse_size_value(value) {
                    style.height = sz;
                }
            }
            "gap" => {
                if let Some(g) = value_to_f32(value) {
                    style.gap = g;
                }
            }
            "flex_wrap" => match value {
                Value::String(s) => style.flex_wrap = s == "wrap",
                Value::Boolean(b) => style.flex_wrap = *b,
                _ => {}
            },
            "padding" => {
                if let Some(p) = parse_spacing_value(value) {
                    style.padding = p;
                }
            }
            "margin" => {
                if let Some(m) = parse_spacing_value(value) {
                    style.margin = m;
                }
            }
            "background_color" => {
                if let Some(color) = parse_color_value(value) {
                    style.background_color = Some(color);
                }
            }
            "border" => {
                if let Some(border) = parse_border_value(value) {
                    style.border = border;
                }
            }
            "corner_radius" => {
                if let Some(cr) = parse_corner_radii_value(value) {
                    style.corner_radii = cr;
                }
            }
            "background_image" => {
                if let Value::String(path) = value {
                    style.background_image = Some(path.clone());
                }
            }
            "position" => {
                match value {
                    Value::String(s) if s == "static" => style.position = Position::Static,
                    Value::Map(map) => {
                        let pos_type = map.get("type").and_then(|v| {
                            if let Value::String(s) = v { Some(s.as_str()) } else { None }
                        }).unwrap_or("static");
                        let x = get_float_from_map(map, "x", 0.0);
                        let y = get_float_from_map(map, "y", 0.0);
                        match pos_type {
                            "absolute" => style.position = Position::Absolute(x, y),
                            "relative" => style.position = Position::Relative(x, y),
                            _ => {}
                        }
                    }
                    _ => {}
                }
            }
            "overflow" => {
                if let Value::String(s) = value {
                    match s.as_str() {
                        "hidden" => style.overflow = Overflow::Hidden,
                        "scroll" => style.overflow = Overflow::Scroll,
                        _ => style.overflow = Overflow::Visible,
                    }
                }
            }
            "opacity" => {
                if let Some(v) = value_to_f32(value) {
                    style.opacity = Opacity(v);
                }
            }
            "transform" => {
                if let Some(t) = parse_transform_value(value) {
                    style.transform = t;
                }
            }
            "transition" => {
                if let Some(set) = parse_transition_value(value) {
                    style.transitions = set;
                }
            }
            _ => {}
        }
    }
}

/// Parse a `transform:` value.
///
/// Accepted shapes:
/// - number — uniform translate_x/translate_y? No — we treat a bare
///   number as meaningless for transforms and ignore it.
/// - `{ translate_x, translate_y, scale, scale_x, scale_y, rotate }` —
///   missing fields default to identity values. `scale` sets both axes.
fn parse_transform_value(value: &Value) -> Option<Transform> {
    match value {
        Value::Map(map) => {
            let mut t = Transform::IDENTITY;
            if let Some(v) = map.get("translate_x").and_then(value_to_f32) {
                t.translate_x = v;
            }
            if let Some(v) = map.get("translate_y").and_then(value_to_f32) {
                t.translate_y = v;
            }
            if let Some(v) = map.get("scale").and_then(value_to_f32) {
                t.scale_x = v;
                t.scale_y = v;
            }
            if let Some(v) = map.get("scale_x").and_then(value_to_f32) {
                t.scale_x = v;
            }
            if let Some(v) = map.get("scale_y").and_then(value_to_f32) {
                t.scale_y = v;
            }
            if let Some(v) = map.get("rotate").and_then(value_to_f32) {
                t.rotate = v;
            }
            Some(t)
        }
        _ => None,
    }
}

/// Parse a `transition:` declaration.
///
/// Accepted shapes:
/// - `"spring"` — enable a default spring on every animatable property
/// - `{ background_color: "spring", border: { stiffness: 200, damping: 30 } }`
///   — enable named properties with either a default or custom config
/// - `{ background_color: true }` — equivalent to `"spring"`
fn parse_transition_value(value: &Value) -> Option<TransitionSet> {
    match value {
        Value::String(s) if s == "spring" || s == "default" => {
            let cfg = TransitionConfig::DEFAULT;
            Some(TransitionSet {
                background_color: Some(cfg),
                text_color: Some(cfg),
                border: Some(cfg),
                corner_radius: Some(cfg),
                padding: Some(cfg),
                margin: Some(cfg),
                gap: Some(cfg),
                text_size: Some(cfg),
                opacity: Some(cfg),
                transform: Some(cfg),
            })
        }
        Value::Map(map) => {
            let mut set = TransitionSet::default();
            for (key, entry_value) in map {
                let cfg = parse_transition_entry(entry_value)?;
                match key.as_str() {
                    "background_color" => set.background_color = Some(cfg),
                    "text_color" => set.text_color = Some(cfg),
                    "border" => set.border = Some(cfg),
                    "corner_radius" => set.corner_radius = Some(cfg),
                    "padding" => set.padding = Some(cfg),
                    "margin" => set.margin = Some(cfg),
                    "gap" => set.gap = Some(cfg),
                    "text_size" => set.text_size = Some(cfg),
                    "opacity" => set.opacity = Some(cfg),
                    "transform" => set.transform = Some(cfg),
                    _ => {}
                }
            }
            Some(set)
        }
        _ => None,
    }
}

/// Parse the value side of a single transition entry into a
/// `TransitionConfig`. `true` / `"spring"` yield the default; a map with
/// `stiffness` / `damping` yields a tuned spring.
fn parse_transition_entry(value: &Value) -> Option<TransitionConfig> {
    match value {
        Value::Boolean(true) => Some(TransitionConfig::DEFAULT),
        Value::String(s) if s == "spring" || s == "default" => Some(TransitionConfig::DEFAULT),
        Value::Map(map) => {
            let mut cfg = TransitionConfig::DEFAULT;
            if let Some(v) = map.get("stiffness").and_then(value_to_f32) {
                cfg.stiffness = v;
            }
            if let Some(v) = map.get("damping").and_then(value_to_f32) {
                cfg.damping = v;
            }
            Some(cfg)
        }
        _ => None,
    }
}

/// Apply text style properties from a map onto a mutable `TextStyle`.
fn apply_text_style_from_map(style: &mut TextStyle, map: &HashMap<String, Value>) {
    for (key, value) in map {
        match key.as_str() {
            "size" => {
                if let Some(sz) = value_to_f32(value) {
                    style.size = sz;
                }
            }
            "color" => {
                if let Some(c) = parse_color_value(value) {
                    style.color = c;
                }
            }
            "align" => {
                if let Value::String(v) = value {
                    if let Some(a) = parse_text_align(v) {
                        style.align = a;
                    }
                }
            }
            "weight" => {
                if let Value::String(v) = value {
                    match v.to_lowercase().as_str() {
                        "normal" => style.weight = FontWeight::Normal,
                        "bold" => style.weight = FontWeight::Bold,
                        "semi_bold" => style.weight = FontWeight::SemiBold,
                        "light" => style.weight = FontWeight::Light,
                        _ => {}
                    }
                }
            }
            "decoration" => {
                if let Value::String(v) = value {
                    match v.to_lowercase().as_str() {
                        "none" => style.decoration = TextDecoration::None,
                        "underline" => style.decoration = TextDecoration::Underline,
                        "strikethrough" => style.decoration = TextDecoration::Strikethrough,
                        _ => {}
                    }
                }
            }
            "width" => {
                if let Some(sz) = parse_size_value(value) {
                    style.width = sz;
                }
            }
            "height" => {
                if let Some(sz) = parse_size_value(value) {
                    style.height = sz;
                }
            }
            "font" => {
                if let Value::String(v) = value {
                    style.font = Some(v.clone());
                }
            }
            _ => {}
        }
    }
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

pub(crate) fn parse_spacing_value(value: &Value) -> Option<Spacing> {
    match value {
        Value::Float(f) => Some(Spacing::all(*f as f32)),
        Value::Integer(i) => Some(Spacing::all(*i as f32)),
        Value::Map(map) => {
            let top = get_float_from_map(map, "top", 0.0);
            let right = get_float_from_map(map, "right", 0.0);
            let bottom = get_float_from_map(map, "bottom", 0.0);
            let left = get_float_from_map(map, "left", 0.0);
            Some(Spacing::new(top, right, bottom, left))
        }
        _ => None,
    }
}

fn create_flex_widget(
    registry: &WidgetRegistry,
    runtime: &Arc<Mutex<Runtime>>,
    descriptor: &WidgetDescriptor,
) -> Result<WidgetRef, BridgeError> {
    let mut flex_widget = FlexWidget::new();

    // block_interactions: when false, clicks are only "handled" if a child or listener handled them
    if let Some(Value::Boolean(b)) = descriptor.properties.get("block_interactions") {
        flex_widget.block_interactions = *b;
    }

    let mut style = FlexStyle::default();

    let mut children = Vec::new();

    // Event handlers (e.g. `mouse_down: fn () { ... }`)
    register_event_listener(
        &mut flex_widget.event_listeners,
        &descriptor.properties,
        runtime,
        "mouse_down",
    )?;
    register_event_listener(
        &mut flex_widget.event_listeners,
        &descriptor.properties,
        runtime,
        "mouse_up",
    )?;
    register_event_listener(
        &mut flex_widget.event_listeners,
        &descriptor.properties,
        runtime,
        "mouse_enter",
    )?;
    register_event_listener(
        &mut flex_widget.event_listeners,
        &descriptor.properties,
        runtime,
        "mouse_leave",
    )?;

    if let Some(value) = descriptor.properties.get("children") {
        if let Value::Array(children_array) = value {
            for child_value in children_array {
                if let Value::Widget(child_widget) = child_value {
                    let child_ref = widget_value_to_widget_ref(
                        registry,
                        runtime,
                        &Value::Widget(child_widget.clone()),
                    )?;
                    children.push(child_ref);
                }
            }
        } else if let Value::Widget(child_widget) = value {
            let child_ref = widget_value_to_widget_ref(
                registry,
                runtime,
                &Value::Widget(child_widget.clone()),
            )?;
            children.push(child_ref);
        }
    }

    if let Some(style_map) = optional_style_map(descriptor) {
        apply_flex_style_from_map(&mut style, style_map);
    }

    // Declared style is the authored target; `style` is kept in sync and
    // is what layout/rendering observes. Only diverges mid-transition.
    flex_widget.declared_style = style.clone();
    flex_widget.style = style;

    if let Some(hover_map) = optional_hover_style_map(descriptor) {
        let mut hover_style = flex_widget.declared_style.clone();
        apply_flex_style_from_map(&mut hover_style, hover_map);
        flex_widget.hover_style = Some(hover_style);
    }

    // `initial:` — style the widget is born with. Defaults to the
    // declared style as a base so unspecified fields are sensible;
    // fields present in the initial map override.
    if let Some(initial_map) = optional_map_property(descriptor, "initial") {
        let mut initial_style = flex_widget.declared_style.clone();
        apply_flex_style_from_map(&mut initial_style, initial_map);
        flex_widget.initial_style = Some(initial_style);
    }

    // `exit:` — style the widget animates toward while leaving the tree.
    if let Some(exit_map) = optional_map_property(descriptor, "exit") {
        let mut exit_style = flex_widget.declared_style.clone();
        apply_flex_style_from_map(&mut exit_style, exit_map);
        flex_widget.exit_style = Some(exit_style);
    }

    // Optional stable identity. Accepts strings, or numbers stringified,
    // so templates can write `key: index` or `key: "header"`.
    if let Some(key_value) = descriptor.properties.get("key") {
        flex_widget.key = key_to_string(key_value);
    }

    for child in children {
        flex_widget.add_child(child);
    }

    // Apply any declared entry transition last so initial_style
    // overrides `style` on the first frame and springs start moving
    // toward `declared_style` on the next tick.
    flex_widget.apply_entry_transition();

    Ok(Arc::new(Mutex::new(flex_widget)))
}

fn key_to_string(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.clone()),
        Value::Integer(i) => Some(i.to_string()),
        Value::Float(f) => Some(f.to_string()),
        Value::Boolean(b) => Some(b.to_string()),
        _ => None,
    }
}

/// Build a `Presence` widget. Recognized properties:
/// - `key`: the generation discriminator. When this changes across
///   reconciles, the current content begins exiting and the new content
///   is held as pending until exits settle.
/// - `children`: the content to mount. Accepts a single Widget or an
///   Array of Widgets.
fn create_presence_widget(
    registry: &WidgetRegistry,
    runtime: &Arc<Mutex<Runtime>>,
    descriptor: &WidgetDescriptor,
) -> Result<WidgetRef, BridgeError> {
    let mut presence = PresenceWidget::new();

    if let Some(value) = descriptor.properties.get("key") {
        presence.generation_key = key_to_string(value);
    }

    let mut children: Vec<WidgetRef> = Vec::new();
    if let Some(value) = descriptor.properties.get("children") {
        match value {
            Value::Array(children_array) => {
                for child_value in children_array {
                    if let Value::Widget(child_widget) = child_value {
                        let child_ref = widget_value_to_widget_ref(
                            registry,
                            runtime,
                            &Value::Widget(child_widget.clone()),
                        )?;
                        children.push(child_ref);
                    }
                }
            }
            Value::Widget(child_widget) => {
                let child_ref = widget_value_to_widget_ref(
                    registry,
                    runtime,
                    &Value::Widget(child_widget.clone()),
                )?;
                children.push(child_ref);
            }
            _ => {}
        }
    }
    presence.inner.children = children;

    Ok(Arc::new(Mutex::new(presence)))
}

fn create_text_widget(
    _runtime: &Arc<Mutex<Runtime>>,
    descriptor: &WidgetDescriptor,
) -> Result<WidgetRef, BridgeError> {
    let style_props = optional_style_map(descriptor);

    // Get text property (required)
    let text_value = descriptor
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

    let mut text_widget = TextWidget::new(text);
    // Default to shrink along both axes for text widgets.
    let mut style = TextStyle::default();
    style.width = Size::Shrink;
    style.height = Size::Shrink;

    if let Some(style_map) = style_props {
        apply_text_style_from_map(&mut style, style_map);
    }
    text_widget.style = style;

    if let Some(hover_map) = optional_hover_style_map(descriptor) {
        let mut hover_style = text_widget.style.clone();
        apply_text_style_from_map(&mut hover_style, hover_map);
        text_widget.hover_style = Some(hover_style);
    }

    Ok(Arc::new(Mutex::new(text_widget)))
}

fn create_text_input_widget(
    runtime: &Arc<Mutex<Runtime>>,
    descriptor: &WidgetDescriptor,
) -> Result<WidgetRef, BridgeError> {
    let mut text_input = TextInputWidget::new();

    // Build styles
    let mut style = FlexStyle::default();
    let mut text_style = TextStyle::default();

    // `value` is required and lives at the root (not in `style`).
    let value_value = descriptor
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
        &descriptor.properties,
        runtime,
        "on_change",
    )?;
    register_event_listener(
        &mut text_input.event_listeners,
        &descriptor.properties,
        runtime,
        "mouse_down",
    )?;
    register_event_listener(
        &mut text_input.event_listeners,
        &descriptor.properties,
        runtime,
        "mouse_up",
    )?;

    if let Some(style_map) = optional_style_map(descriptor) {
        apply_flex_style_from_map(&mut style, style_map);
        apply_text_style_from_map(&mut text_style, style_map);
    }

    text_input.style = style;
    text_input.text_style = text_style;

    if let Some(hover_map) = optional_hover_style_map(descriptor) {
        let mut hover_style = text_input.style.clone();
        apply_flex_style_from_map(&mut hover_style, hover_map);
        text_input.hover_style = Some(hover_style);
        let mut hover_text_style = text_input.text_style.clone();
        apply_text_style_from_map(&mut hover_text_style, hover_map);
        text_input.hover_text_style = Some(hover_text_style);
    }

    // `focus_style:` — applied while this input is the active focus
    // target. Built off the base style so the author only has to spell
    // the fields that change (typically the border).
    if let Some(focus_map) = optional_map_property(descriptor, "focus_style") {
        let mut focus_style = text_input.style.clone();
        apply_flex_style_from_map(&mut focus_style, focus_map);
        text_input.focus_style = Some(focus_style);
        let mut focus_text_style = text_input.text_style.clone();
        apply_text_style_from_map(&mut focus_text_style, focus_map);
        text_input.focus_text_style = Some(focus_text_style);
    }

    Ok(Arc::new(Mutex::new(text_input)))
}

fn create_svg_widget(
    _runtime: &Arc<Mutex<Runtime>>,
    descriptor: &WidgetDescriptor,
) -> Result<WidgetRef, BridgeError> {
    let style_props = optional_style_map(descriptor);

    // Get required properties
    let path_value = descriptor
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

    let width_value = descriptor
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

    let height_value = descriptor
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

fn create_image_widget(
    runtime: &Arc<Mutex<Runtime>>,
    descriptor: &WidgetDescriptor,
) -> Result<WidgetRef, BridgeError> {
    // Required: path, width, height
    let path = match descriptor.properties.get("path") {
        Some(Value::String(s)) => s.clone(),
        _ => return Err(BridgeError::MissingProperty("path".to_string())),
    };

    let width = descriptor
        .properties
        .get("width")
        .and_then(value_to_f32)
        .ok_or_else(|| BridgeError::MissingProperty("width".to_string()))?;
    let height = descriptor
        .properties
        .get("height")
        .and_then(value_to_f32)
        .ok_or_else(|| BridgeError::MissingProperty("height".to_string()))?;

    let mut image_widget = ImageWidget::new(path, width, height);

    register_event_listener(
        &mut image_widget.event_listeners,
        &descriptor.properties,
        runtime,
        "mouse_down",
    )?;

    Ok(Arc::new(Mutex::new(image_widget)))
}

fn create_grid_widget(
    registry: &WidgetRegistry,
    runtime: &Arc<Mutex<Runtime>>,
    descriptor: &WidgetDescriptor,
) -> Result<WidgetRef, BridgeError> {
    let mut grid = GridWidget::new();

    if let Some(style_map) = optional_style_map(descriptor) {
        apply_grid_style_from_map(&mut grid.style, style_map);
    }

    if let Some(Value::Array(children_array)) = descriptor.properties.get("children") {
        for child_value in children_array {
            if let Value::Widget(child_widget) = child_value {
                let placement = extract_grid_placement(&child_widget.properties);
                let child_ref = widget_value_to_widget_ref(
                    registry,
                    runtime,
                    &Value::Widget(child_widget.clone()),
                )?;
                grid.children.push((placement, child_ref));
            }
        }
    }

    register_event_listener(
        &mut grid.event_listeners,
        &descriptor.properties,
        runtime,
        "mouse_down",
    )?;
    register_event_listener(
        &mut grid.event_listeners,
        &descriptor.properties,
        runtime,
        "mouse_up",
    )?;
    register_event_listener(
        &mut grid.event_listeners,
        &descriptor.properties,
        runtime,
        "mouse_enter",
    )?;
    register_event_listener(
        &mut grid.event_listeners,
        &descriptor.properties,
        runtime,
        "mouse_leave",
    )?;

    Ok(Arc::new(Mutex::new(grid)))
}

/// Apply grid-specific style properties from a map.
fn apply_grid_style_from_map(style: &mut GridStyle, map: &HashMap<String, Value>) {
    for (key, value) in map {
        match key.as_str() {
            "columns" => {
                if let Some(v) = value_to_f32(value) {
                    style.columns = v as u32;
                }
            }
            "rows" => {
                if let Some(v) = value_to_f32(value) {
                    style.rows = v as u32;
                }
            }
            "cell_width" => {
                if let Some(v) = value_to_f32(value) {
                    style.cell_width = v;
                }
            }
            "cell_height" => {
                if let Some(v) = value_to_f32(value) {
                    style.cell_height = v;
                }
            }
            "gap" => {
                if let Some(v) = value_to_f32(value) {
                    style.gap = v;
                }
            }
            "padding" => {
                if let Some(p) = parse_spacing_value(value) {
                    style.padding = p;
                }
            }
            "margin" => {
                if let Some(m) = parse_spacing_value(value) {
                    style.margin = m;
                }
            }
            "background_color" => {
                if let Some(c) = parse_color_value(value) {
                    style.background_color = Some(c);
                }
            }
            "cell_color" => {
                if let Some(c) = parse_color_value(value) {
                    style.cell_color = Some(c);
                }
            }
            "corner_radius" => {
                if let Some(cr) = parse_corner_radii_value(value) {
                    style.corner_radii = cr;
                }
            }
            _ => {}
        }
    }
}

/// Extract grid placement properties from a widget's property map.
fn extract_grid_placement(properties: &HashMap<String, Value>) -> GridPlacement {
    GridPlacement {
        col: properties
            .get("grid_col")
            .and_then(value_to_f32)
            .unwrap_or(0.0) as u32,
        row: properties
            .get("grid_row")
            .and_then(value_to_f32)
            .unwrap_or(0.0) as u32,
        col_span: properties
            .get("grid_col_span")
            .and_then(value_to_f32)
            .unwrap_or(1.0) as u32,
        row_span: properties
            .get("grid_row_span")
            .and_then(value_to_f32)
            .unwrap_or(1.0) as u32,
    }
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
        Value::String(s) if s.starts_with('#') => {
            let hex = s.trim_start_matches('#');
            match hex.len() {
                6 => {
                    let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
                    let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
                    let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
                    Some(Color::new(r, g, b, 255))
                }
                8 => {
                    let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
                    let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
                    let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
                    let a = u8::from_str_radix(&hex[6..8], 16).ok()?;
                    Some(Color::new(r, g, b, a))
                }
                _ => None,
            }
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
