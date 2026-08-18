//! The vocabulary the language actually recognises, and what to say when
//! a document misses it.
//!
//! ogham drops what it does not recognise — **keys and values alike** —
//! and the widget then parses, lays out, draws, and is wrong. Nothing
//! errors, nothing warns, nothing fails a test. Five instances were found
//! by hand in a single afternoon of one game's UI: `on_click` for
//! `mouse_down`, `font_size` for `size`, `wrap` for `flex_wrap`,
//! `wrap: "wrap"` for the same, and `cross_alignment: "stretch"` — which
//! is not a key at all but a *value*, silently read as `start`, and
//! therefore invisible for as long as the children happen to `"grow"`.
//!
//! This module is the vocabulary written down once, beside the code that
//! consumes it. Two things read it:
//!
//! - [`scan_source`] — a parse of a `.ogh` file, reporting file, line and
//!   column. Sees only what an author literally wrote, which is most of
//!   it, and is what a repo turns into a test over its own `.ogh`.
//! - The widget builder, when [`RuntimeConfig::with_strict_vocabulary`]
//!   is on. Sees the *materialized* map however it was assembled — a
//!   `let`-bound style map, a match arm, a host-injected value — so it
//!   cannot be fooled by indirection, at the cost of having no source
//!   location to name.
//!
//! Neither changes what is drawn. A key the runtime ignores today goes on
//! being ignored; this says so out loud instead of leaving it to an
//! afternoon of grepping.
//!
//! # The rule for adding to the tables
//!
//! Each table below mirrors exactly one `match` in [`super::builder`].
//! Adding a key there without adding it here turns a working property
//! into a reported violation, which is the one failure mode that would
//! get this turned off. The tables are `pub` so that a consumer can read
//! them; they are not extensible, because a host that could add to them
//! could also silence a real one.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::parser::{Expression, Literal, Span};
use crate::runtime::value::Value;

// ---------------------------------------------------------------------
// The tables.
// ---------------------------------------------------------------------

/// Placement a parent `Grid` reads off a *child's* properties
/// (`extract_grid_placement`), so it is legal on any widget that might be
/// one.
pub const GRID_PLACEMENT: &[&str] = &["grid_col", "grid_col_span", "grid_row", "grid_row_span"];

/// `Flex` — the properties `create_flex_widget` reads.
pub const FLEX_PROPERTIES: &[&str] = &[
    "accepts_drop",
    "block_interactions",
    "children",
    "contextmenu",
    "drag_dead_zone",
    "drag_end",
    "drag_move",
    "drag_payload",
    "drag_preview",
    "drag_start",
    "exit",
    "hover_style",
    "initial",
    "key",
    "mouse_down",
    "mouse_enter",
    "mouse_leave",
    "mouse_up",
    "style",
];

/// `Text`.
pub const TEXT_PROPERTIES: &[&str] = &["hover_style", "style", "text"];

/// `TextInput`.
pub const TEXT_INPUT_PROPERTIES: &[&str] = &[
    "focus_style",
    "hover_style",
    "mouse_down",
    "mouse_up",
    "on_change",
    "on_submit",
    "style",
    "value",
];

/// `Image`. No `style`: the widget takes bare `width` / `height` numbers
/// at the root and cannot participate in layout.
pub const IMAGE_PROPERTIES: &[&str] = &["height", "mouse_down", "path", "width"];

/// `Grid`.
pub const GRID_PROPERTIES: &[&str] = &[
    "children",
    "mouse_down",
    "mouse_enter",
    "mouse_leave",
    "mouse_up",
    "style",
];

/// `Presence`.
pub const PRESENCE_PROPERTIES: &[&str] = &["children", "key", "mode", "style"];

/// `Portal`. No `style` — the portal seats its subtree, it does not paint.
pub const PORTAL_PROPERTIES: &[&str] = &[
    "anchor",
    "anchor_offset",
    "anchor_policy",
    "children",
    "cursor",
    "focus_trap",
    "layer",
    "open",
];

/// `Canvas`.
pub const CANVAS_PROPERTIES: &[&str] = &[
    "contextmenu",
    "mouse_down",
    "mouse_enter",
    "mouse_leave",
    "mouse_up",
    "painter",
    "props",
    "style",
];

/// The keys `apply_flex_style_from_map` matches.
pub const FLEX_STYLE_KEYS: &[&str] = &[
    "backdrop_filter",
    "background_color",
    "background_image",
    "border",
    "corner_chamfer",
    "corner_radius",
    "cross_alignment",
    "cursor",
    "direction",
    "flex_wrap",
    "gap",
    "height",
    "inner_glow",
    "main_alignment",
    "margin",
    "opacity",
    "overflow",
    "padding",
    "position",
    "scroll_follow_end",
    "shadow",
    "stagger",
    "transform",
    "transition",
    "width",
];

/// The keys `apply_text_style_from_map` matches.
pub const TEXT_STYLE_KEYS: &[&str] = &[
    "align",
    "color",
    "decoration",
    "font",
    "height",
    "letter_spacing",
    "outline",
    "size",
    "weight",
    "width",
];

/// The keys `apply_grid_style_from_map` matches.
pub const GRID_STYLE_KEYS: &[&str] = &[
    "background_color",
    "cell_color",
    "cell_height",
    "cell_width",
    "columns",
    "corner_chamfer",
    "corner_radius",
    "gap",
    "margin",
    "padding",
    "rows",
];

pub const DIRECTIONS: &[&str] = &["column", "column_reverse", "row", "row_reverse"];
pub const ALIGNMENTS: &[&str] = &["center", "end", "space_around", "space_between", "start"];
pub const OVERFLOWS: &[&str] = &["hidden", "scroll", "visible"];
pub const CURSOR_ROLES: &[&str] = &["default", "pointer"];
pub const WRAPS: &[&str] = &["nowrap", "wrap"];
pub const SIZES: &[&str] = &["grow", "shrink"];
pub const POSITION_KINDS: &[&str] = &["absolute", "relative", "static"];
pub const TEXT_ALIGNS: &[&str] = &["center", "left", "right"];
pub const FONT_WEIGHTS: &[&str] = &["bold", "light", "normal", "semi_bold"];
pub const DECORATIONS: &[&str] = &["none", "strikethrough", "underline"];
pub const BORDER_STYLES: &[&str] = &["dashed", "dotted", "solid"];
pub const SPRINGS: &[&str] = &["default", "spring"];
pub const STAGGER_ORDERS: &[&str] = &["forward", "reverse"];

pub const SPACING_KEYS: &[&str] = &["bottom", "left", "right", "top"];
pub const COLOR_KEYS: &[&str] = &["a", "b", "g", "r"];
pub const CORNER_KEYS: &[&str] = &["bottom_left", "bottom_right", "top_left", "top_right"];
pub const TRANSFORM_KEYS: &[&str] = &[
    "rotate",
    "scale",
    "scale_x",
    "scale_y",
    "translate_x",
    "translate_y",
];
pub const SHADOW_KEYS: &[&str] = &["blur", "color", "offset_x", "offset_y"];
pub const INNER_GLOW_KEYS: &[&str] = &["blur", "color", "spread"];
pub const BACKDROP_FILTER_KEYS: &[&str] = &["blur"];
pub const STAGGER_KEYS: &[&str] = &["exit_order", "exit_step", "step"];
pub const TRANSITION_KEYS: &[&str] = &[
    "background_color",
    "border",
    "corner_chamfer",
    "corner_radius",
    "corners",
    "gap",
    "inner_glow",
    "margin",
    "opacity",
    "padding",
    "text_color",
    "text_size",
    "transform",
];
pub const SPRING_KEYS: &[&str] = &["damping", "delay", "stiffness"];
pub const BORDER_KEYS: &[&str] = &[
    "bottom", "color", "left", "right", "style", "top", "width",
];
pub const BORDER_SIDE_KEYS: &[&str] = &["color", "style", "width"];
pub const POSITION_KEYS: &[&str] = &["type", "x", "y"];
pub const GROW_KEYS: &[&str] = &["grow"];
pub const OUTLINE_KEYS: &[&str] = &["color", "width"];
pub const ANCHOR_OFFSET_KEYS: &[&str] = &["x", "y"];

/// Which style vocabulary a widget's `style:` map is read against.
#[derive(Clone, Copy, PartialEq, Eq)]
enum StyleKind {
    Flex,
    Text,
    /// `TextInput` runs its one map through *both* appliers, so its
    /// vocabulary is the union — a fact that is easy to lose if the two
    /// checks are written separately.
    FlexAndText,
    Grid,
    None,
}

/// The properties and style vocabulary of one built-in widget, or `None`
/// for a name this crate does not own.
///
/// A host's own widget reads whatever properties it likes, so a name that
/// is not ours is not checked at all. That is the difference between a
/// lint that gets kept and one that gets turned off.
fn widget_vocabulary(name: &str) -> Option<(&'static [&'static str], StyleKind)> {
    match name {
        "flex" => Some((FLEX_PROPERTIES, StyleKind::Flex)),
        "text" => Some((TEXT_PROPERTIES, StyleKind::Text)),
        "textinput" => Some((TEXT_INPUT_PROPERTIES, StyleKind::FlexAndText)),
        "image" => Some((IMAGE_PROPERTIES, StyleKind::None)),
        "grid" => Some((GRID_PROPERTIES, StyleKind::Grid)),
        "presence" => Some((PRESENCE_PROPERTIES, StyleKind::Flex)),
        "portal" => Some((PORTAL_PROPERTIES, StyleKind::None)),
        "canvas" => Some((CANVAS_PROPERTIES, StyleKind::Flex)),
        _ => None,
    }
}

/// The property names that carry a style map on this widget.
fn style_slots(name: &str) -> &'static [&'static str] {
    match name {
        "flex" | "presence" => &["style", "hover_style", "initial", "exit"],
        "text" => &["style", "hover_style"],
        "textinput" => &["style", "hover_style", "focus_style"],
        "grid" | "canvas" => &["style"],
        _ => &[],
    }
}

// ---------------------------------------------------------------------
// What a violation is.
// ---------------------------------------------------------------------

/// Whether the document wrote a name the language does not have, or a
/// value it does not have.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A key: `on_click`, `wrap`, `font_size`.
    Key,
    /// A value in a closed set: `cross_alignment: "stretch"`.
    Value,
}

/// Where a violation was found.
///
/// Two, because the two feeders genuinely know different things and
/// pretending otherwise would mean one of them lying. The source scan has
/// a file and a line; the builder has the VM call path that produced the
/// widget, which names the helper rather than the line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Site {
    Source {
        file: String,
        line: usize,
        column: usize,
    },
    Built {
        /// The call-stack path the widget was constructed under, e.g.
        /// `main/menu_row/label`. Empty for a widget built at module top
        /// level.
        path: String,
    },
}

impl std::fmt::Display for Site {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Site::Source { file, line, column } => write!(f, "{file}:{line}:{column}"),
            Site::Built { path } if path.is_empty() => write!(f, "<module>"),
            Site::Built { path } => write!(f, "{path}"),
        }
    }
}

/// One thing the document said that the language does not recognise.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Violation {
    pub site: Site,
    /// The widget type as written, lowercased.
    pub widget: String,
    /// Dotted path from the widget: `on_click`, `style.cross_alignment`,
    /// `hover_style.border.colour`.
    pub path: String,
    pub kind: Kind,
    /// The key or value as written.
    pub found: String,
    /// The vocabulary it was checked against.
    pub known: &'static [&'static str],
    /// The nearest known name, when one is near enough to name.
    pub suggestion: Option<&'static str>,
}

impl std::fmt::Display for Violation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let what = match self.kind {
            Kind::Key => format!("`{}` is not a property of {}", self.path, self.widget),
            Kind::Value => format!(
                "`{}` on {} is not {:?}",
                self.path, self.widget, self.found
            ),
        };
        write!(f, "{}: {what}", self.site)?;
        match self.suggestion {
            Some(near) => write!(f, " — did you mean `{near}`?"),
            None => write!(f, " (known: {})", self.known.join(", ")),
        }
    }
}

impl Violation {
    /// A stable identity for de-duplication: the same mistake seen on
    /// sixty frames is one mistake.
    fn signature(&self) -> String {
        format!("{}|{}|{}|{}", self.site, self.widget, self.path, self.found)
    }
}

/// The nearest known name to `found`, if one is near enough that naming
/// it helps rather than misleads.
///
/// Ranked: a shared ending first (`wrap` → `flex_wrap`, `font_size` →
/// `size`, `radius` → `corner_radius`), then any containment, then one or
/// two edits. **A tie yields nothing** — `translate` is as close to
/// `translate_x` as to `translate_y`, and a confident wrong answer is
/// worse than the list of what exists.
fn nearest(found: &str, known: &'static [&'static str]) -> Option<&'static str> {
    let lower = found.to_lowercase();
    let mut best: Option<(usize, &'static str)> = None;
    let mut tied = false;
    for candidate in known {
        let score = if *candidate == lower {
            0
        } else if candidate.ends_with(&lower) || lower.ends_with(*candidate) {
            1
        } else if candidate.contains(&lower) || lower.contains(*candidate) {
            2
        } else {
            let d = edit_distance(&lower, candidate);
            if d <= 2 && lower.len() > 3 {
                2 + d
            } else {
                continue;
            }
        };
        match best {
            Some((s, _)) if score > s => {}
            Some((s, _)) if score == s => tied = true,
            _ => {
                best = Some((score, candidate));
                tied = false;
            }
        }
    }
    match tied {
        true => None,
        false => best.map(|(_, c)| c),
    }
}

fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0usize; b.len() + 1];
    for i in 1..=a.len() {
        cur[0] = i;
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            cur[j] = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

// ---------------------------------------------------------------------
// The thing being checked.
// ---------------------------------------------------------------------

/// A value flattened to what the vocabulary needs to know about it.
///
/// One shape, so one checker serves both a parsed `Expression` (which has
/// spans and may be dynamic) and a runtime `Value` (which has neither
/// spans nor anything dynamic left). [`Node::Opaque`] is the honest
/// answer for anything the checker cannot see into — a closure, a widget,
/// a computed expression — and it is checked against nothing.
enum Node<'a> {
    Str(&'a str),
    Map(Vec<(&'a str, Span, Node<'a>)>),
    Opaque,
}

impl<'a> Node<'a> {
    fn as_str(&self) -> Option<&'a str> {
        match self {
            Node::Str(s) => Some(s),
            _ => None,
        }
    }

    fn as_map(&self) -> Option<&[(&'a str, Span, Node<'a>)]> {
        match self {
            Node::Map(entries) => Some(entries),
            _ => None,
        }
    }
}

/// Lower a parsed expression into what the checker reads. Anything that
/// is not a literal string or a literal map is [`Node::Opaque`] — the
/// static scan sees only what the author wrote down, and the builder scan
/// is what covers the rest.
fn node_from_expression(expr: &Expression) -> Node<'_> {
    match expr {
        Expression::Literal(Literal::String(s, _)) => Node::Str(s),
        Expression::Literal(Literal::Map(map)) => Node::Map(
            map.properties
                .iter()
                .map(|(key, value)| {
                    (
                        key.as_str(),
                        key.span,
                        node_from_expression(value),
                    )
                })
                .collect(),
        ),
        Expression::Grouping(g) => node_from_expression(&g.value),
        _ => Node::Opaque,
    }
}

fn node_from_value(value: &Value) -> Node<'_> {
    match value {
        Value::String(s) => Node::Str(s),
        Value::Map(map) => Node::Map(
            map.iter()
                .map(|(key, value)| (key.as_str(), Span::zero(), node_from_value(value)))
                .collect(),
        ),
        _ => Node::Opaque,
    }
}

// ---------------------------------------------------------------------
// The checker.
// ---------------------------------------------------------------------

struct Check<'a> {
    widget: String,
    origin: Origin<'a>,
    out: Vec<Violation>,
}

enum Origin<'a> {
    /// A parsed file: spans mean something.
    Source(&'a str),
    /// A built widget: the VM call path is all there is.
    Built(&'a str),
}

impl<'a> Check<'a> {
    fn site(&self, span: Span) -> Site {
        match self.origin {
            Origin::Source(file) => Site::Source {
                file: file.to_string(),
                line: span.start_line,
                column: span.start_column,
            },
            Origin::Built(path) => Site::Built {
                path: path.to_string(),
            },
        }
    }

    fn key(&mut self, span: Span, path: String, found: &str, known: &'static [&'static str]) {
        let site = self.site(span);
        self.out.push(Violation {
            site,
            widget: self.widget.clone(),
            path,
            kind: Kind::Key,
            found: found.to_string(),
            known,
            suggestion: nearest(found, known),
        });
    }

    fn value(&mut self, span: Span, path: String, found: &str, known: &'static [&'static str]) {
        let site = self.site(span);
        self.out.push(Violation {
            site,
            widget: self.widget.clone(),
            path,
            kind: Kind::Value,
            found: found.to_string(),
            known,
            suggestion: nearest(found, known),
        });
    }

    /// A string value in a closed set. `fold` mirrors whether the builder
    /// lowercases before matching — where it does not, `"Hidden"` really
    /// is silently wrong and saying so is a true positive.
    fn enum_value(
        &mut self,
        span: Span,
        path: &str,
        node: &Node,
        known: &'static [&'static str],
        fold: bool,
    ) {
        let Some(found) = node.as_str() else {
            return;
        };
        let probe = if fold {
            found.to_lowercase()
        } else {
            found.to_string()
        };
        if !known.contains(&probe.as_str()) {
            self.value(span, path.to_string(), found, known);
        }
    }

    /// Every key of a map against a flat set, with no descent.
    fn map_keys(&mut self, path: &str, node: &Node, known: &'static [&'static str]) {
        let Some(entries) = node.as_map() else {
            return;
        };
        for (key, span, _) in entries {
            if !known.contains(key) {
                self.key(*span, format!("{path}.{key}"), key, known);
            }
        }
    }

    fn size(&mut self, span: Span, path: &str, node: &Node) {
        match node {
            Node::Str(_) => self.enum_value(span, path, node, SIZES, true),
            Node::Map(_) => self.map_keys(path, node, GROW_KEYS),
            Node::Opaque => {}
        }
    }

    fn border(&mut self, path: &str, node: &Node) {
        let Some(entries) = node.as_map() else {
            return;
        };
        let uniform = entries
            .iter()
            .any(|(k, _, _)| matches!(*k, "width" | "color" | "style"));
        for (key, span, value) in entries {
            let child = format!("{path}.{key}");
            match (*key, uniform) {
                ("width", _) => {}
                ("color", _) => self.map_keys(&child, value, COLOR_KEYS),
                ("style", _) => self.enum_value(*span, &child, value, BORDER_STYLES, true),
                ("top" | "right" | "bottom" | "left", false) => self.border_side(&child, value),
                _ => self.key(*span, child, key, BORDER_KEYS),
            }
        }
    }

    fn border_side(&mut self, path: &str, node: &Node) {
        let Some(entries) = node.as_map() else {
            return;
        };
        for (key, span, value) in entries {
            let child = format!("{path}.{key}");
            match *key {
                "width" => {}
                "color" => self.map_keys(&child, value, COLOR_KEYS),
                "style" => self.enum_value(*span, &child, value, BORDER_STYLES, true),
                _ => self.key(*span, child, key, BORDER_SIDE_KEYS),
            }
        }
    }

    fn transition(&mut self, span: Span, path: &str, node: &Node) {
        match node {
            Node::Str(_) => self.enum_value(span, path, node, SPRINGS, false),
            Node::Map(entries) => {
                for (key, span, value) in entries {
                    let child = format!("{path}.{key}");
                    if !TRANSITION_KEYS.contains(key) {
                        self.key(*span, child, key, TRANSITION_KEYS);
                        continue;
                    }
                    match value {
                        Node::Str(_) => self.enum_value(*span, &child, value, SPRINGS, false),
                        Node::Map(_) => self.map_keys(&child, value, SPRING_KEYS),
                        Node::Opaque => {}
                    }
                }
            }
            Node::Opaque => {}
        }
    }

    fn position(&mut self, span: Span, path: &str, node: &Node) {
        match node {
            Node::Str(_) => self.enum_value(span, path, node, &["static"], false),
            Node::Map(entries) => {
                for (key, span, value) in entries {
                    let child = format!("{path}.{key}");
                    match *key {
                        "type" => self.enum_value(*span, &child, value, POSITION_KINDS, false),
                        "x" | "y" => {}
                        _ => self.key(*span, child, key, POSITION_KEYS),
                    }
                }
            }
            Node::Opaque => {}
        }
    }

    /// One entry of a flex style map. Mirrors
    /// `apply_flex_style_from_map` arm for arm.
    fn flex_style_entry(&mut self, path: &str, key: &str, span: Span, value: &Node) {
        match key {
            "direction" => self.enum_value(span, path, value, DIRECTIONS, true),
            "main_alignment" | "cross_alignment" => {
                self.enum_value(span, path, value, ALIGNMENTS, true)
            }
            "width" | "height" => self.size(span, path, value),
            "flex_wrap" => self.enum_value(span, path, value, WRAPS, false),
            "padding" | "margin" => self.map_keys(path, value, SPACING_KEYS),
            "background_color" => self.map_keys(path, value, COLOR_KEYS),
            "border" => self.border(path, value),
            "corner_radius" | "corner_chamfer" => self.map_keys(path, value, CORNER_KEYS),
            "inner_glow" => {
                self.map_keys(path, value, INNER_GLOW_KEYS);
                self.descend_color(path, value);
            }
            "shadow" => {
                self.map_keys(path, value, SHADOW_KEYS);
                self.descend_color(path, value);
            }
            "position" => self.position(span, path, value),
            "overflow" => self.enum_value(span, path, value, OVERFLOWS, false),
            "cursor" => self.enum_value(span, path, value, CURSOR_ROLES, false),
            "transform" => self.map_keys(path, value, TRANSFORM_KEYS),
            "backdrop_filter" => self.map_keys(path, value, BACKDROP_FILTER_KEYS),
            "transition" => self.transition(span, path, value),
            "stagger" => {
                self.map_keys(path, value, STAGGER_KEYS);
                if let Some(entries) = value.as_map() {
                    for (k, s, v) in entries {
                        if *k == "exit_order" {
                            self.enum_value(*s, &format!("{path}.{k}"), v, STAGGER_ORDERS, false);
                        }
                    }
                }
            }
            // gap, background_image, scroll_follow_end, opacity: no closed
            // vocabulary below them.
            _ => {}
        }
    }

    /// One entry of a text style map. Mirrors
    /// `apply_text_style_from_map`.
    fn text_style_entry(&mut self, path: &str, key: &str, span: Span, value: &Node) {
        match key {
            "color" => self.map_keys(path, value, COLOR_KEYS),
            "align" => self.enum_value(span, path, value, TEXT_ALIGNS, true),
            "weight" => self.enum_value(span, path, value, FONT_WEIGHTS, true),
            "decoration" => self.enum_value(span, path, value, DECORATIONS, true),
            "width" | "height" => self.size(span, path, value),
            "outline" => {
                self.map_keys(path, value, OUTLINE_KEYS);
                self.descend_color(path, value);
            }
            _ => {}
        }
    }

    fn descend_color(&mut self, path: &str, node: &Node) {
        let Some(entries) = node.as_map() else {
            return;
        };
        for (key, _, value) in entries {
            if *key == "color" {
                self.map_keys(&format!("{path}.{key}"), value, COLOR_KEYS);
            }
        }
    }

    fn style_map(&mut self, slot: &str, kind: StyleKind, node: &Node) {
        let Some(entries) = node.as_map() else {
            return;
        };
        let known: &'static [&'static str] = match kind {
            StyleKind::Flex => FLEX_STYLE_KEYS,
            StyleKind::Text => TEXT_STYLE_KEYS,
            StyleKind::Grid => GRID_STYLE_KEYS,
            // The union is checked by hand below; this is only what a
            // violation is reported against.
            StyleKind::FlexAndText => FLEX_STYLE_KEYS,
            StyleKind::None => return,
        };
        for (key, span, value) in entries {
            let path = format!("{slot}.{key}");
            let in_flex = FLEX_STYLE_KEYS.contains(key);
            let in_text = TEXT_STYLE_KEYS.contains(key);
            let in_grid = GRID_STYLE_KEYS.contains(key);
            let recognised = match kind {
                StyleKind::Flex => in_flex,
                StyleKind::Text => in_text,
                StyleKind::FlexAndText => in_flex || in_text,
                StyleKind::Grid => in_grid,
                StyleKind::None => false,
            };
            if !recognised {
                self.key(*span, path, key, known);
                continue;
            }
            match kind {
                StyleKind::Flex => self.flex_style_entry(&path, key, *span, value),
                StyleKind::Text => self.text_style_entry(&path, key, *span, value),
                StyleKind::FlexAndText => {
                    // A key both appliers claim (`width`, `height`) means
                    // the same thing to both, so checking it once is
                    // enough and checking it twice would double-report.
                    if in_flex {
                        self.flex_style_entry(&path, key, *span, value);
                    } else {
                        self.text_style_entry(&path, key, *span, value);
                    }
                }
                StyleKind::Grid => {
                    match *key {
                        "padding" | "margin" => self.map_keys(&path, value, SPACING_KEYS),
                        "background_color" | "cell_color" => {
                            self.map_keys(&path, value, COLOR_KEYS)
                        }
                        "corner_radius" | "corner_chamfer" => {
                            self.map_keys(&path, value, CORNER_KEYS)
                        }
                        _ => {}
                    };
                }
                StyleKind::None => {}
            }
        }
    }

    /// The whole of one widget: its property names, its style maps, and
    /// the closed-set values in its root properties.
    fn widget(&mut self, properties: &[(&str, Span, Node)]) {
        let Some((known, style_kind)) = widget_vocabulary(&self.widget) else {
            return;
        };
        let slots = style_slots(&self.widget);
        for (key, span, value) in properties {
            if !known.contains(key) && !GRID_PLACEMENT.contains(key) {
                self.key(*span, key.to_string(), key, known);
                continue;
            }
            if slots.contains(key) {
                self.style_map(key, style_kind, value);
                continue;
            }
            // Root properties with a closed vocabulary of their own.
            // `Presence { mode }` and every `Portal` string are already
            // hard errors in the builder, so they are not repeated here.
            if self.widget == "portal" && *key == "anchor_offset" {
                self.map_keys(key, value, ANCHOR_OFFSET_KEYS);
            }
        }
    }
}

// ---------------------------------------------------------------------
// Feeder 1: a parsed source file.
// ---------------------------------------------------------------------

/// Every violation in one `.ogh` source, with file, line and column.
///
/// Sees what the author literally wrote: a widget's properties, and a
/// style map given as a literal. A style map reached through a `let` is
/// [`Node::Opaque`] here and is the builder feeder's to catch — the two
/// are complements, not alternatives.
///
/// Returns an empty vector for a source that does not parse; a parse
/// error has its own diagnostics and burying it under a second set of
/// them helps nobody.
pub fn scan_source(file: &str, source: &str) -> Vec<Violation> {
    let tokens = crate::scanner::Scanner::new(source.to_string()).scan();
    let Ok(module) = crate::parser::Parser::new(tokens).parse() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    walk_block(&module.body, &mut |widget| {
        let name = widget.identifier.get().to_lowercase();
        if widget_vocabulary(&name).is_none() {
            return;
        }
        let properties: Vec<(&str, Span, Node)> = widget
            .properties
            .iter()
            .map(|(key, value)| (key.as_str(), key.span, node_from_expression(value)))
            .collect();
        let mut check = Check {
            widget: name,
            origin: Origin::Source(file),
            out: Vec::new(),
        };
        check.widget(&properties);
        out.extend(check.out);
    });
    out.sort_by(|a, b| a.site.to_string().cmp(&b.site.to_string()));
    out
}

fn walk_block(block: &crate::parser::Block, f: &mut impl FnMut(&crate::parser::Widget)) {
    for statement in &block.statement_list {
        walk_statement(statement, f);
    }
}

fn walk_statement(statement: &crate::parser::Statement, f: &mut impl FnMut(&crate::parser::Widget)) {
    use crate::parser::Statement as S;
    match statement {
        S::Expression(s) => walk_expression(&s.value, f),
        S::Declare(s) => walk_expression(&s.value, f),
        S::DeclareState(s) => walk_expression(&s.value, f),
        S::Assign(s) => walk_expression(&s.value, f),
        S::Log(s) => walk_expression(&s.value, f),
        S::Return(s) => {
            if let Some(value) = &s.value {
                walk_expression(value, f);
            }
        }
        S::Conditional(s) => {
            for (condition, block) in &s.branches {
                walk_expression(condition, f);
                walk_block(block, f);
            }
            if let Some(block) = &s.else_block {
                walk_block(block, f);
            }
        }
        S::ForLoop(s) => {
            walk_expression(&s.range_start, f);
            walk_expression(&s.range_end, f);
            walk_block(&s.body, f);
        }
        S::ScreenDeclaration(s) => walk_expression(&s.view, f),
        S::Import(_) | S::RecordDeclaration(_) | S::HostStateDeclaration(_)
        | S::EventsDeclaration(_) => {}
    }
}

fn walk_expression(expr: &Expression, f: &mut impl FnMut(&crate::parser::Widget)) {
    use Expression as E;
    match expr {
        E::Widget(widget) => {
            f(widget);
            for (_, value) in &widget.properties {
                walk_expression(value, f);
            }
        }
        E::Literal(literal) => walk_literal(literal, f),
        E::Unary(e) => walk_expression(&e.value, f),
        E::Binary(e) => {
            walk_expression(&e.left, f);
            walk_expression(&e.right, f);
        }
        E::Grouping(e) => walk_expression(&e.value, f),
        E::MemberAccess(e) => walk_expression(&e.object, f),
        E::Call(e) => {
            walk_expression(&e.callee, f);
            for argument in &e.arguments {
                walk_expression(argument, f);
            }
        }
        E::IndexAccess(e) => {
            walk_expression(&e.object, f);
            walk_expression(&e.index, f);
        }
        E::Range(e) => {
            walk_expression(&e.start, f);
            walk_expression(&e.end, f);
        }
        E::ForLoop(e) | E::SpreadForLoop(e) => {
            walk_expression(&e.range_start, f);
            walk_expression(&e.range_end, f);
            walk_block(&e.body, f);
        }
        E::Spread(e) => walk_expression(&e.inner, f),
        E::Match(e) => {
            walk_expression(&e.scrutinee, f);
            for (pattern, block) in &e.arms {
                walk_expression(pattern, f);
                walk_block(block, f);
            }
        }
        E::PrefixIncrement(_) | E::PostfixIncrement(_) => {}
    }
}

fn walk_literal(literal: &Literal, f: &mut impl FnMut(&crate::parser::Widget)) {
    match literal {
        Literal::Map(map) => {
            for (_, value) in &map.properties {
                walk_expression(value, f);
            }
        }
        Literal::Array(array) => {
            for element in &array.elements {
                walk_expression(element, f);
            }
        }
        Literal::Function(function) => walk_block(&function.body, f),
        _ => {}
    }
}

// ---------------------------------------------------------------------
// Feeder 2: the widget builder.
// ---------------------------------------------------------------------

/// Where the builder's findings go.
///
/// Shared by clone with the [`WidgetRegistry`](super::builder::WidgetRegistry)
/// the runtime hands out each frame, so a hot reload's fresh registry
/// still reports into the same place.
///
/// De-duplicated: a widget rebuilt sixty times a second is one mistake,
/// not sixty, and a list that grows without bound is a leak dressed as a
/// diagnostic.
#[derive(Default)]
pub struct Sink {
    seen: std::collections::HashSet<String>,
    violations: Vec<Violation>,
}

impl Sink {
    /// Record a violation the first time it is seen, printing it once.
    fn record(&mut self, violation: Violation) {
        if !self.seen.insert(violation.signature()) {
            return;
        }
        eprintln!("[ogham] {violation}");
        self.violations.push(violation);
    }

    pub fn violations(&self) -> &[Violation] {
        &self.violations
    }

    pub fn is_empty(&self) -> bool {
        self.violations.is_empty()
    }
}

/// The handle a registry carries when strict vocabulary is on.
pub type SinkHandle = Arc<Mutex<Sink>>;

/// Check one built widget's descriptor. Called by every built-in factory
/// when — and only when — the host asked for it.
pub(crate) fn check_descriptor(
    sink: &SinkHandle,
    identifier: &str,
    owned_path: &str,
    properties: &HashMap<String, Value>,
) {
    let name = identifier.to_lowercase();
    if widget_vocabulary(&name).is_none() {
        return;
    }
    let nodes: Vec<(&str, Span, Node)> = properties
        .iter()
        .map(|(key, value)| (key.as_str(), Span::zero(), node_from_value(value)))
        .collect();
    let mut check = Check {
        widget: name,
        origin: Origin::Built(owned_path),
        out: Vec::new(),
    };
    check.widget(&nodes);
    if check.out.is_empty() {
        return;
    }
    let mut sink = sink.lock().expect("vocabulary sink poisoned");
    for violation in check.out {
        sink.record(violation);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scan(source: &str) -> Vec<String> {
        scan_source("t.ogh", source)
            .iter()
            .map(|v| format!("{}|{:?}|{}", v.path, v.kind, v.found))
            .collect()
    }

    #[test]
    fn a_click_listener_is_mouse_down() {
        let found = scan(r#"let main = fn () { Flex { on_click: fn () { 1 } } };"#);
        assert_eq!(found, vec!["on_click|Key|on_click"]);
    }

    #[test]
    fn a_text_size_is_size() {
        let found = scan(r#"let main = fn () { Text { text: "x", style: { font_size: 42 } } };"#);
        assert_eq!(found, vec!["style.font_size|Key|font_size"]);
    }

    #[test]
    fn a_wrapping_row_says_flex_wrap() {
        let found = scan(r#"let main = fn () { Flex { style: { wrap: true } } };"#);
        assert_eq!(found, vec!["style.wrap|Key|wrap"]);
        assert_eq!(
            scan_source("t.ogh", r#"let main = fn () { Flex { style: { wrap: "wrap" } } };"#)[0]
                .suggestion,
            Some("flex_wrap")
        );
    }

    /// The subtle one: a *value*, in a closed set, that reads as `start`
    /// and lays out plausibly for as long as the children `"grow"`.
    #[test]
    fn stretch_is_not_a_cross_alignment() {
        let found = scan(
            r#"let main = fn () { Flex { style: { cross_alignment: "stretch" } } };"#,
        );
        assert_eq!(found, vec![r#"style.cross_alignment|Value|stretch"#]);
    }

    #[test]
    fn the_alignments_that_exist_are_not_reported() {
        for value in ALIGNMENTS {
            let source =
                format!(r#"let main = fn () {{ Flex {{ style: {{ cross_alignment: "{value}" }} }} }};"#);
            assert!(scan(&source).is_empty(), "{value} should be accepted");
        }
    }

    #[test]
    fn a_style_key_on_the_wrong_widget_is_reported() {
        // `gap` is a Flex key; a Text has no gap and never had one.
        let found = scan(r#"let main = fn () { Text { text: "x", style: { gap: 4 } } };"#);
        assert_eq!(found, vec!["style.gap|Key|gap"]);
    }

    #[test]
    fn a_text_input_takes_both_vocabularies() {
        let source = r#"let main = fn () {
            TextInput { value: "x", style: { gap: 4, size: 12, letter_spacing: 1 } }
        };"#;
        assert!(scan(source).is_empty());
    }

    #[test]
    fn a_widget_this_crate_does_not_own_is_not_checked() {
        // A host-registered widget reads whatever properties it likes.
        assert!(scan(r#"let main = fn () { Dial { needle: 3, wobble: "yes" } };"#).is_empty());
    }

    #[test]
    fn grid_placement_is_legal_on_any_child() {
        let source = r#"let main = fn () {
            Grid { children: [ Flex { grid_col: 1, grid_row_span: 2 } ] }
        };"#;
        assert!(scan(source).is_empty());
    }

    #[test]
    fn a_nested_map_key_is_checked_too() {
        let found = scan(
            r#"let main = fn () { Flex { style: { padding: { top: 4, botom: 4 } } } };"#,
        );
        assert_eq!(found, vec!["style.padding.botom|Key|botom"]);
    }

    #[test]
    fn a_corner_map_that_would_be_dropped_whole_is_reported() {
        // All four keys are required; one typo drops the entire radius.
        let found = scan(
            r#"let main = fn () { Flex { style: { corner_radius: { tl: 4, top_right: 4, bottom_left: 4, bottom_right: 4 } } } };"#,
        );
        assert_eq!(found, vec!["style.corner_radius.tl|Key|tl"]);
    }

    #[test]
    fn a_dynamic_value_is_checked_against_nothing() {
        // The static scan sees a `let`, not a map. The builder feeder is
        // what covers this, and reporting a guess here would be the false
        // positive that gets the whole thing turned off.
        let source = r#"
            let s = { anything_at_all: 1 };
            let main = fn () { Flex { style: s } };"#;
        assert!(scan(source).is_empty());
    }

    #[test]
    fn hover_and_exit_styles_are_style_maps() {
        let source = r#"let main = fn () {
            Flex { hover_style: { wrap: true }, exit: { opacty: 0 } }
        };"#;
        let mut found = scan(source);
        found.sort();
        assert_eq!(found, vec!["exit.opacty|Key|opacty", "hover_style.wrap|Key|wrap"]);
    }

    #[test]
    fn an_edit_away_is_suggested_and_a_guess_is_not() {
        assert_eq!(nearest("opacty", FLEX_STYLE_KEYS), Some("opacity"));
        assert_eq!(nearest("font_size", TEXT_STYLE_KEYS), Some("size"));
        assert_eq!(nearest("radius", FLEX_STYLE_KEYS), Some("corner_radius"));
        assert_eq!(nearest("on_click", FLEX_PROPERTIES), None);
        // As close to one as to the other: say nothing, list both.
        assert_eq!(nearest("translate", TRANSFORM_KEYS), None);
    }
}
