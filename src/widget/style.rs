use crate::widget::animation::TransitionSet;
use crate::widget::WidgetRef;

/// Represents either the horizontal or vertical axis, used to unify
/// row/column logic in layout calculations.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Axis {
    Horizontal,
    Vertical,
}

impl Axis {
    /// Returns the perpendicular axis.
    pub fn cross(&self) -> Axis {
        match self {
            Axis::Horizontal => Axis::Vertical,
            Axis::Vertical => Axis::Horizontal,
        }
    }
}

/// Controls how content that exceeds the container bounds is handled.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum Overflow {
    /// Content is not clipped and may render outside the container (default).
    #[default]
    Visible,
    /// Content is clipped to the container bounds.
    Hidden,
    /// Content is clipped and can be scrolled with the mouse wheel.
    Scroll,
}

#[derive(Debug, Clone)]
pub struct FlexStyle {
    pub position: Position,
    pub width: Size,
    pub height: Size,
    pub direction: Direction,
    pub main_alignment: Alignment,
    pub cross_alignment: Alignment,
    /// When `true`, row-direction children that exceed the container's
    /// main-axis size wrap onto a new line. Only meaningful for row
    /// directions; column wrap is not supported.
    pub flex_wrap: bool,
    pub gap: f32,
    pub padding: Padding,
    pub margin: Margin,
    pub border: Border,
    pub corners: Corners,
    pub background_color: Option<Color>,
    pub background_image: Option<String>,
    pub text_size: Option<f32>,
    pub text_color: Option<Color>,
    pub overflow: Overflow,
    /// Paint-time opacity applied to the widget and its descendants.
    /// 1.0 is fully opaque; values less than 1.0 trigger a Skia layer
    /// composite. Does not affect layout.
    pub opacity: Opacity,
    /// Paint-time affine transform applied to the widget and its
    /// descendants. Pivots around the widget's center. Does not affect
    /// layout or hit-testing (hit-tests ignore the transform for now).
    pub transform: Transform,
    /// Paint-time backdrop filter applied to the canvas content
    /// underneath this widget's border box (i.e. the panel "frosts"
    /// what's behind it). `None` is a no-op. The filter is captured
    /// when this widget paints its own background; descendants
    /// render normally on top of the blurred composite.
    pub backdrop_filter: Option<BackdropFilter>,
    /// Paint-time inset glow drawn inside the border box. `None` is a
    /// no-op. Renders after the background fill and before children so
    /// it sits beneath descendant content (matches CSS `box-shadow: inset`).
    pub inner_glow: Option<InnerGlow>,
    /// Spring-driven transitions declared for specific style properties.
    /// Empty by default — properties snap to new values unless opted in.
    pub transitions: TransitionSet,
}

/// A single-knob backdrop filter. Currently exposes Gaussian blur only;
/// future variants (saturate, brightness) can extend this type without
/// a breaking change to call sites that just read `.blur`.
///
/// Sigma is in unscaled pixels — Skia's symmetric kernel sigma. Values
/// of 8–16 read as "frosted glass" over typical UI imagery; values
/// above ~32 collapse to a near-uniform tint and aren't worth the cost.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct BackdropFilter {
    pub blur: f32,
}

impl BackdropFilter {
    /// True when the filter would do anything useful — call sites use
    /// this to skip the (expensive) save-layer for zero-sigma filters.
    pub fn is_active(&self) -> bool {
        self.blur > 0.0
    }
}

/// Single-scalar opacity wrapped so it has a non-zero `Default`. Values
/// are clamped to `[0, 1]` at paint time.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Opacity(pub f32);

impl Default for Opacity {
    fn default() -> Self {
        Self(1.0)
    }
}

impl Opacity {
    pub const OPAQUE: Self = Self(1.0);
    pub fn value(self) -> f32 {
        self.0.clamp(0.0, 1.0)
    }
    pub fn is_opaque(self) -> bool {
        self.value() >= 1.0
    }
}

/// Affine transform with separate translation, scale, and rotation
/// components. Composed at paint time as
/// `translate(center) * rotate * scale * translate(-center) * translate(tx, ty)`
/// so scale and rotation pivot around the widget's center.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transform {
    pub translate_x: f32,
    pub translate_y: f32,
    pub scale_x: f32,
    pub scale_y: f32,
    /// Rotation in degrees. Positive rotates clockwise (Skia convention).
    pub rotate: f32,
}

impl Default for Transform {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl Transform {
    pub const IDENTITY: Self = Self {
        translate_x: 0.0,
        translate_y: 0.0,
        scale_x: 1.0,
        scale_y: 1.0,
        rotate: 0.0,
    };

    pub fn is_identity(&self) -> bool {
        self.translate_x == 0.0
            && self.translate_y == 0.0
            && self.scale_x == 1.0
            && self.scale_y == 1.0
            && self.rotate == 0.0
    }
}

impl Default for FlexStyle {
    fn default() -> Self {
        Self {
            position: Position::Static,
            width: Size::Shrink,
            height: Size::Shrink,
            direction: Direction::Row,
            main_alignment: Alignment::Start,
            cross_alignment: Alignment::Start,
            flex_wrap: false,
            gap: 0.0,
            padding: Padding::identity(),
            margin: Margin::identity(),
            border: Border::identity(),
            corners: Corners::identity(),
            background_color: None,
            background_image: None,
            text_size: None,
            text_color: None,
            overflow: Overflow::Visible,
            opacity: Opacity::OPAQUE,
            transform: Transform::IDENTITY,
            backdrop_filter: None,
            inner_glow: None,
            transitions: TransitionSet::default(),
        }
    }
}

impl FlexStyle {
    /// Creates a new builder for constructing a FlexStyle
    pub fn builder() -> FlexStyleBuilder {
        FlexStyleBuilder::default()
    }

    /// Total inset (padding + margin + border) on the left side.
    pub fn inset_left(&self) -> f32 {
        self.padding.get_left() + self.margin.get_left() + self.border.get_left()
    }

    /// Total inset (padding + margin + border) on the top side.
    pub fn inset_top(&self) -> f32 {
        self.padding.get_top() + self.margin.get_top() + self.border.get_top()
    }

    /// Total inset (padding + margin + border) on the right side.
    pub fn inset_right(&self) -> f32 {
        self.padding.get_right() + self.margin.get_right() + self.border.get_right()
    }

    /// Total inset (padding + margin + border) on the bottom side.
    pub fn inset_bottom(&self) -> f32 {
        self.padding.get_bottom() + self.margin.get_bottom() + self.border.get_bottom()
    }

    /// Total horizontal inset (left + right).
    pub fn horizontal_inset(&self) -> f32 {
        self.inset_left() + self.inset_right()
    }

    /// Total vertical inset (top + bottom).
    pub fn vertical_inset(&self) -> f32 {
        self.inset_top() + self.inset_bottom()
    }

    /// Compare only the fields that affect layout — skipping paint-only
    /// fields (`opacity`, `transform`, `*_color`, `background_image`,
    /// `inner_glow`), `corners` (visual corner shape doesn't change
    /// box dimensions), and `transitions` (which doesn't impl `PartialEq`
    /// and gates how animations interpolate, not what the layout pass
    /// produces).
    ///
    /// Used by [`FlexWidget::update`] to decide whether to bubble a
    /// `needs_layout` signal up the tree on reconcile.
    pub fn layout_equal(&self, other: &Self) -> bool {
        self.position == other.position
            && self.width == other.width
            && self.height == other.height
            && self.direction == other.direction
            && self.main_alignment == other.main_alignment
            && self.cross_alignment == other.cross_alignment
            && self.flex_wrap == other.flex_wrap
            && self.gap == other.gap
            && self.padding == other.padding
            && self.margin == other.margin
            && self.border == other.border
            && self.text_size == other.text_size
            && self.overflow == other.overflow
    }

    /// Returns the size along the given axis.
    pub fn size_on_axis(&self, axis: Axis) -> &Size {
        match axis {
            Axis::Horizontal => &self.width,
            Axis::Vertical => &self.height,
        }
    }

    /// Total inset (padding + margin + border) along the given axis.
    pub fn inset_on_axis(&self, axis: Axis) -> f32 {
        self.padding.total(axis) + self.margin.total(axis) + self.border.width_total(axis)
    }
}

/// Builder for constructing FlexStyle instances
#[derive(Debug, Clone)]
pub struct FlexStyleBuilder {
    style: FlexStyle,
}

impl FlexStyleBuilder {
    pub fn position(mut self, position: Position) -> Self {
        self.style.position = position;
        self
    }

    pub fn width(mut self, width: Size) -> Self {
        self.style.width = width;
        self
    }

    pub fn height(mut self, height: Size) -> Self {
        self.style.height = height;
        self
    }

    pub fn direction(mut self, direction: Direction) -> Self {
        self.style.direction = direction;
        self
    }

    pub fn main_alignment(mut self, alignment: Alignment) -> Self {
        self.style.main_alignment = alignment;
        self
    }

    pub fn cross_alignment(mut self, alignment: Alignment) -> Self {
        self.style.cross_alignment = alignment;
        self
    }

    /// Sets both main and cross alignment.
    pub fn align(mut self, main: Alignment, cross: Alignment) -> Self {
        self.style.main_alignment = main;
        self.style.cross_alignment = cross;
        self
    }

    pub fn gap(mut self, gap: f32) -> Self {
        self.style.gap = gap;
        self
    }

    pub fn padding(mut self, padding: Padding) -> Self {
        self.style.padding = padding;
        self
    }

    pub fn margin(mut self, margin: Margin) -> Self {
        self.style.margin = margin;
        self
    }

    pub fn border(mut self, border: Border) -> Self {
        self.style.border = border;
        self
    }

    pub fn corners(mut self, corners: Corners) -> Self {
        self.style.corners = corners;
        self
    }

    pub fn background_color(mut self, color: Color) -> Self {
        self.style.background_color = Some(color);
        self
    }

    pub fn background_color_opt(mut self, color: Option<Color>) -> Self {
        self.style.background_color = color;
        self
    }

    pub fn background_image(mut self, image_path: String) -> Self {
        self.style.background_image = Some(image_path);
        self
    }

    pub fn background_image_opt(mut self, image_path: Option<String>) -> Self {
        self.style.background_image = image_path;
        self
    }

    pub fn text_size(mut self, size: f32) -> Self {
        self.style.text_size = Some(size);
        self
    }

    pub fn text_size_opt(mut self, size: Option<f32>) -> Self {
        self.style.text_size = size;
        self
    }

    pub fn text_color(mut self, color: Color) -> Self {
        self.style.text_color = Some(color);
        self
    }

    pub fn text_color_opt(mut self, color: Option<Color>) -> Self {
        self.style.text_color = color;
        self
    }

    pub fn build(self) -> FlexStyle {
        self.style
    }
}

impl Default for FlexStyleBuilder {
    fn default() -> Self {
        Self {
            style: FlexStyle::default(),
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub enum Position {
    #[default]
    Static,
    Relative(f32, f32),
    Absolute(f32, f32),
}

#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub enum Size {
    Grow(f32),
    #[default]
    Shrink,
    Fixed(f32),
    Percent(f32),
}

impl Size {
    /// Returns the fixed value if this is `Size::Fixed(v)`, otherwise `None`.
    pub fn as_fixed(&self) -> Option<f32> {
        match self {
            Size::Fixed(v) => Some(*v),
            _ => None,
        }
    }

    /// Returns the grow basis if this is `Size::Grow(basis)`, otherwise `0.0`.
    pub fn grow_basis(&self) -> f32 {
        match self {
            Size::Grow(basis) => *basis,
            _ => 0.0,
        }
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub enum TextAlign {
    #[default]
    Left,
    Center,
    Right,
}

/// An outline stroked around glyphs, painted *under* the fill so the fill
/// color stays crisp. `width` is the logical (unscaled) stroke width in
/// pixels; it is DPI-scaled at paint time like any other stroke.
#[derive(Debug, Clone, Copy)]
pub struct TextOutline {
    pub color: Color,
    pub width: f32,
}

#[derive(Debug, Clone)]
pub struct TextStyle {
    pub size: f32,
    pub color: Color,
    pub weight: FontWeight,
    pub decoration: TextDecoration,
    pub align: TextAlign,
    /// Controls layout width behavior for text widgets.
    pub width: Size,
    /// Controls layout height behavior for text widgets.
    pub height: Size,
    /// Optional registered font family name (e.g. `"heading"`).
    /// When `None`, the system default font is used.
    pub font: Option<String>,
    /// Optional outline stroked around glyphs. `None` draws no outline.
    pub outline: Option<TextOutline>,
}

impl TextStyle {
    pub fn get_size(&self) -> f32 {
        self.size
    }

    pub fn get_color(&self) -> Color {
        self.color
    }

    pub fn get_weight(&self) -> FontWeight {
        self.weight
    }

    pub fn get_decoration(&self) -> TextDecoration {
        self.decoration
    }

    pub fn get_align(&self) -> TextAlign {
        self.align
    }

    pub fn get_font(&self) -> Option<&str> {
        self.font.as_deref()
    }

    pub fn get_outline(&self) -> Option<TextOutline> {
        self.outline
    }

    pub fn with_color(mut self, color: Color) -> Self {
        self.color = color;
        self
    }

    /// Creates a new builder for constructing a TextStyle
    pub fn builder() -> TextStyleBuilder {
        TextStyleBuilder::default()
    }
}

/// Builder for constructing TextStyle instances
#[derive(Debug, Clone)]
pub struct TextStyleBuilder {
    style: TextStyle,
}

impl TextStyleBuilder {
    pub fn size(mut self, size: f32) -> Self {
        self.style.size = size;
        self
    }

    pub fn color(mut self, color: Color) -> Self {
        self.style.color = color;
        self
    }

    pub fn weight(mut self, weight: FontWeight) -> Self {
        self.style.weight = weight;
        self
    }

    pub fn decoration(mut self, decoration: TextDecoration) -> Self {
        self.style.decoration = decoration;
        self
    }

    pub fn align(mut self, align: TextAlign) -> Self {
        self.style.align = align;
        self
    }

    pub fn font(mut self, name: String) -> Self {
        self.style.font = Some(name);
        self
    }

    pub fn outline(mut self, outline: TextOutline) -> Self {
        self.style.outline = Some(outline);
        self
    }

    pub fn build(self) -> TextStyle {
        self.style
    }
}

impl Default for TextStyleBuilder {
    fn default() -> Self {
        Self {
            style: TextStyle::default(),
        }
    }
}

impl Default for TextStyle {
    fn default() -> Self {
        Self {
            size: 16.0,                      // Default font size
            color: Color::new(0, 0, 0, 255), // Default black color
            weight: FontWeight::Normal,
            decoration: TextDecoration::None,
            align: TextAlign::Left,
            // Width grows to fill its column allocation (so left/center/right
            // alignment has room to work); height shrinks to the glyph extent.
            // A Grow height let a default Text claim its parent's full height,
            // ballooning any Shrink row that held it — the height analogue of
            // the flex shrink-measurement bug.
            width: Size::Grow(1.0),
            height: Size::Shrink,
            font: None,
            outline: None,
        }
    }
}

#[derive(Debug, Default, PartialEq, Clone)]
pub enum Direction {
    #[default]
    Row,
    Column,
    RowReverse,
    ColumnReverse,
}

impl Direction {
    pub fn is_row(&self) -> bool {
        matches!(self, Direction::Row | Direction::RowReverse)
    }

    pub fn is_reverse(&self) -> bool {
        matches!(self, Direction::RowReverse | Direction::ColumnReverse)
    }

    /// The primary axis along which children are laid out.
    pub fn main_axis(&self) -> Axis {
        if self.is_row() {
            Axis::Horizontal
        } else {
            Axis::Vertical
        }
    }

    /// The axis perpendicular to the main axis.
    pub fn cross_axis(&self) -> Axis {
        self.main_axis().cross()
    }

    pub fn update_main_axis_position(&self, x: &mut f32, y: &mut f32, delta: f32) {
        match self {
            Direction::Row | Direction::RowReverse => *x += delta,
            Direction::Column | Direction::ColumnReverse => *y += delta,
        }
    }

    pub fn get_grow_size(&self, basis: f32, sibling_basis: f32, available_size: f32) -> f32 {
        // If there's no available space (parent is constrained), return basis as minimum size
        if available_size <= 0.0 {
            return basis;
        }
        if sibling_basis == 0.0 {
            available_size
        } else {
            (basis / sibling_basis) * available_size
        }
    }

    pub fn get_shrink_size(
        &self,
        children: &[WidgetRef],
        get_dimensions: impl Fn(&WidgetRef) -> (f32, f32),
    ) -> f32 {
        if self.is_row() {
            children.iter().map(|child| get_dimensions(child).0).sum()
        } else {
            children.iter().map(|child| get_dimensions(child).1).sum()
        }
    }

    pub fn get_shrink_max_size(
        &self,
        children: &[WidgetRef],
        get_dimensions: impl Fn(&WidgetRef) -> (f32, f32),
    ) -> f32 {
        if self.is_row() {
            children
                .iter()
                .map(|child| get_dimensions(child).1)
                .max_by(|a, b| a.partial_cmp(b).unwrap())
                .unwrap_or(0.0)
        } else {
            children
                .iter()
                .map(|child| get_dimensions(child).0)
                .max_by(|a, b| a.partial_cmp(b).unwrap())
                .unwrap_or(0.0)
        }
    }
}

#[derive(Debug, Default, Clone, PartialEq)]
pub enum Alignment {
    #[default]
    Start,
    Center,
    End,
    SpaceBetween,
    SpaceAround,
}

impl Alignment {
    /// Calculates the position offset for a single item based on the alignment and available space
    pub fn get_offset(&self, item_size: f32, available_space: f32) -> f32 {
        match self {
            Alignment::Start => 0.0,
            Alignment::Center => (available_space - item_size) / 2.0,
            Alignment::End => available_space - item_size,
            Alignment::SpaceBetween | Alignment::SpaceAround => 0.0, // These are handled differently
        }
    }

    /// Calculates the spacing between items for SpaceBetween and SpaceAround alignments
    pub fn get_spacing(
        &self,
        total_items: usize,
        available_space: f32,
        total_item_size: f32,
    ) -> f32 {
        if total_items <= 1 {
            return 0.0;
        }

        match self {
            Alignment::SpaceBetween => {
                (available_space - total_item_size) / (total_items - 1) as f32
            }
            Alignment::SpaceAround => (available_space - total_item_size) / total_items as f32,
            _ => 0.0,
        }
    }

    /// Gets the initial offset for SpaceAround alignment
    pub fn get_space_around_offset(&self, spacing: f32) -> f32 {
        match self {
            Alignment::SpaceAround => spacing / 2.0,
            _ => 0.0,
        }
    }
}

/// Generic four-sided spacing (used for both padding and margin).
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Spacing {
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    pub left: f32,
}

impl Spacing {
    pub fn identity() -> Self {
        Self {
            top: 0.0,
            right: 0.0,
            bottom: 0.0,
            left: 0.0,
        }
    }

    pub fn new(top: f32, right: f32, bottom: f32, left: f32) -> Self {
        Self {
            top,
            right,
            bottom,
            left,
        }
    }

    pub fn all(value: f32) -> Self {
        Self {
            top: value,
            right: value,
            bottom: value,
            left: value,
        }
    }

    pub fn symmetric(horizontal: f32, vertical: f32) -> Self {
        Self {
            top: vertical,
            right: horizontal,
            bottom: vertical,
            left: horizontal,
        }
    }

    pub fn get_top(&self) -> f32 {
        self.top
    }

    pub fn get_right(&self) -> f32 {
        self.right
    }

    pub fn get_bottom(&self) -> f32 {
        self.bottom
    }

    pub fn get_left(&self) -> f32 {
        self.left
    }

    /// Returns the start-side value for the given axis (left for horizontal, top for vertical).
    pub fn start(&self, axis: Axis) -> f32 {
        match axis {
            Axis::Horizontal => self.left,
            Axis::Vertical => self.top,
        }
    }

    /// Returns the end-side value for the given axis (right for horizontal, bottom for vertical).
    pub fn end(&self, axis: Axis) -> f32 {
        match axis {
            Axis::Horizontal => self.right,
            Axis::Vertical => self.bottom,
        }
    }

    /// Returns the total (start + end) for the given axis.
    pub fn total(&self, axis: Axis) -> f32 {
        self.start(axis) + self.end(axis)
    }
}

pub type Padding = Spacing;
pub type Margin = Spacing;

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Border {
    pub top: BorderSide,
    pub right: BorderSide,
    pub bottom: BorderSide,
    pub left: BorderSide,
}

impl Border {
    pub fn identity() -> Self {
        Self {
            top: BorderSide::identity(),
            right: BorderSide::identity(),
            bottom: BorderSide::identity(),
            left: BorderSide::identity(),
        }
    }

    pub fn new(top: BorderSide, right: BorderSide, bottom: BorderSide, left: BorderSide) -> Self {
        Self {
            top,
            right,
            bottom,
            left,
        }
    }

    pub fn get_top(&self) -> f32 {
        self.top.width
    }

    pub fn get_right(&self) -> f32 {
        self.right.width
    }

    pub fn get_bottom(&self) -> f32 {
        self.bottom.width
    }

    pub fn get_left(&self) -> f32 {
        self.left.width
    }

    /// Returns the start-side width for the given axis.
    pub fn width_start(&self, axis: Axis) -> f32 {
        match axis {
            Axis::Horizontal => self.left.width,
            Axis::Vertical => self.top.width,
        }
    }

    /// Returns the end-side width for the given axis.
    pub fn width_end(&self, axis: Axis) -> f32 {
        match axis {
            Axis::Horizontal => self.right.width,
            Axis::Vertical => self.bottom.width,
        }
    }

    /// Returns the total border width for the given axis.
    pub fn width_total(&self, axis: Axis) -> f32 {
        self.width_start(axis) + self.width_end(axis)
    }
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct BorderSide {
    pub width: f32,
    pub color: Color,
    pub style: BorderStyle,
}

impl BorderSide {
    pub fn identity() -> Self {
        Self {
            width: 0.0,
            color: Color::new(0, 0, 0, 255),
            style: BorderStyle::Solid,
        }
    }

    pub fn new(width: f32, color: Color, style: BorderStyle) -> Self {
        Self {
            width,
            color,
            style,
        }
    }
}

/// Shape of a single corner of a box. Each corner is independently
/// `Sharp` (90° vertex), `Round(r)` (quarter-circle of radius `r`), or
/// `Chamfer(c)` (diagonal cut taking `c` px off each adjacent side).
///
/// `Sharp` and `Round(0.0)` / `Chamfer(0.0)` are semantically equivalent;
/// constructors normalise zero-sized rounds/chamfers to `Sharp` so
/// downstream fast-path checks (e.g. `Corners::is_pure_round`) don't
/// have to special-case them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CornerShape {
    Sharp,
    Round(f32),
    Chamfer(f32),
}

impl Default for CornerShape {
    fn default() -> Self {
        Self::Sharp
    }
}

impl CornerShape {
    /// Build a `Round(n)`, normalising `n <= 0` to `Sharp`.
    pub fn round(n: f32) -> Self {
        if n > 0.0 {
            Self::Round(n)
        } else {
            Self::Sharp
        }
    }

    /// Build a `Chamfer(n)`, normalising `n <= 0` to `Sharp`.
    pub fn chamfer(n: f32) -> Self {
        if n > 0.0 {
            Self::Chamfer(n)
        } else {
            Self::Sharp
        }
    }

    /// Scalar size of this corner (radius for `Round`, chamfer depth for
    /// `Chamfer`, 0 for `Sharp`).
    pub fn size(self) -> f32 {
        match self {
            Self::Sharp => 0.0,
            Self::Round(r) => r,
            Self::Chamfer(c) => c,
        }
    }

    pub fn is_sharp(self) -> bool {
        matches!(self, Self::Sharp)
    }

    pub fn is_round(self) -> bool {
        matches!(self, Self::Round(_))
    }

    pub fn is_chamfer(self) -> bool {
        matches!(self, Self::Chamfer(_))
    }
}

/// Per-corner shape for a box. Each corner can independently be sharp,
/// rounded, or chamfered. The `corner_radius:` and `corner_chamfer:`
/// Ogham style keys both write into this field — when both target the
/// same corner the later property in the style map wins (matches the
/// generic style-map duplicate-key rule).
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct Corners {
    pub top_left: CornerShape,
    pub top_right: CornerShape,
    pub bottom_left: CornerShape,
    pub bottom_right: CornerShape,
}

impl Corners {
    pub fn identity() -> Self {
        Self {
            top_left: CornerShape::Sharp,
            top_right: CornerShape::Sharp,
            bottom_left: CornerShape::Sharp,
            bottom_right: CornerShape::Sharp,
        }
    }

    pub fn new(
        top_left: CornerShape,
        top_right: CornerShape,
        bottom_left: CornerShape,
        bottom_right: CornerShape,
    ) -> Self {
        Self {
            top_left,
            top_right,
            bottom_left,
            bottom_right,
        }
    }

    /// All four corners rounded with the same radius (normalises to
    /// `Sharp` when `radius <= 0`).
    pub fn all_round(radius: f32) -> Self {
        let s = CornerShape::round(radius);
        Self {
            top_left: s,
            top_right: s,
            bottom_left: s,
            bottom_right: s,
        }
    }

    /// All four corners chamfered with the same size.
    pub fn all_chamfer(size: f32) -> Self {
        let s = CornerShape::chamfer(size);
        Self {
            top_left: s,
            top_right: s,
            bottom_left: s,
            bottom_right: s,
        }
    }

    /// True when every corner is `Sharp`. Lets render backends skip
    /// path construction and just `fill_rect` / `draw_border` straight.
    pub fn is_all_sharp(&self) -> bool {
        self.top_left.is_sharp()
            && self.top_right.is_sharp()
            && self.bottom_left.is_sharp()
            && self.bottom_right.is_sharp()
    }

    /// True when every non-sharp corner is `Round`. Backends use this
    /// to stay on the existing rounded-rect fast path; mixed shapes
    /// or any `Chamfer` fall through to the general path builder.
    pub fn is_pure_round(&self) -> bool {
        let ok = |c: CornerShape| c.is_sharp() || c.is_round();
        ok(self.top_left) && ok(self.top_right) && ok(self.bottom_left) && ok(self.bottom_right)
    }
}

/// Paint-time inner glow drawn inside the border box. Modeled on
/// CSS `box-shadow: inset` — `blur` is the Gaussian sigma in unscaled
/// pixels; `spread` insets the stroke that many pixels from the border
/// edge so the glow appears to sit just inside the border. Skia
/// renders this as a stroked path with a blur mask filter, clipped to
/// the border-box interior so the glow can only spread inward.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct InnerGlow {
    pub color: Color,
    pub blur: f32,
    pub spread: f32,
}

impl InnerGlow {
    pub fn is_active(&self) -> bool {
        self.blur > 0.0 && self.color.a > 0
    }
}

#[derive(Debug, Default, Clone, PartialEq)]
pub enum BorderStyle {
    #[default]
    Solid,
    Dashed,
    Dotted,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub fn new(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub enum FontWeight {
    #[default]
    Normal,
    SemiBold,
    Bold,
    Light,
}

#[derive(Debug, Default, Clone, Copy)]
pub enum TextDecoration {
    #[default]
    None,
    Underline,
    Strikethrough,
}
