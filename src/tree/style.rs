use crate::tree::WidgetRef;

#[derive(Debug, Default, Clone)]
pub struct FlexStyle {
    pub position: Position,
    pub width: Size,
    pub height: Size,
    pub direction: Direction,
    pub main_alignment: Alignment,
    pub cross_alignment: Alignment,
    pub gap: f32,
    pub padding: Padding,
    pub margin: Margin,
    pub border: Border,
    pub corner_radii: CornerRadii,
    pub background_color: Option<Color>,
    pub background_image: Option<String>,
    pub text_size: Option<f32>,
    pub text_color: Option<Color>,
}

impl FlexStyle {
    pub fn default() -> Self {
        Self {
            position: Position::Static,
            width: Size::Shrink,
            height: Size::Shrink,
            direction: Direction::Row,
            main_alignment: Alignment::Start,
            cross_alignment: Alignment::Start,
            gap: 0.0,
            padding: Padding::identity(),
            margin: Margin::identity(),
            border: Border::identity(),
            corner_radii: CornerRadii::identity(),
            background_color: None,
            background_image: None,
            text_size: None,
            text_color: None,
        }
    }

    /// Creates a new builder for constructing a FlexStyle
    pub fn builder() -> FlexStyleBuilder {
        FlexStyleBuilder::default()
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

    pub fn corner_radii(mut self, corner_radii: CornerRadii) -> Self {
        self.style.corner_radii = corner_radii;
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

#[derive(Debug, Default, Clone, Copy)]
pub enum Position {
    #[default]
    Static,
    Relative(f32, f32),
    Absolute(f32, f32),
}

#[derive(Debug, Default, Clone, Copy)]
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
            // Preserve historical behavior: text expands to fill its allocation by default.
            width: Size::Grow(1.0),
            height: Size::Grow(1.0),
            font: None,
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

#[derive(Debug, Default, Clone)]
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
#[derive(Debug, Default, Clone)]
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
}

pub type Padding = Spacing;
pub type Margin = Spacing;

#[derive(Debug, Default, Clone)]
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
}

#[derive(Debug, Default, Clone)]
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

#[derive(Debug, Default, Clone)]
pub struct CornerRadii {
    pub top_left: f32,
    pub top_right: f32,
    pub bottom_left: f32,
    pub bottom_right: f32,
}

impl CornerRadii {
    pub fn identity() -> Self {
        Self {
            top_left: 0.0,
            top_right: 0.0,
            bottom_left: 0.0,
            bottom_right: 0.0,
        }
    }

    pub fn all(radius: f32) -> Self {
        Self {
            top_left: radius,
            top_right: radius,
            bottom_left: radius,
            bottom_right: radius,
        }
    }

    pub fn new(top_left: f32, top_right: f32, bottom_left: f32, bottom_right: f32) -> Self {
        Self {
            top_left,
            top_right,
            bottom_left,
            bottom_right,
        }
    }

    pub fn symmetric(horizontal: f32, vertical: f32) -> Self {
        Self {
            top_left: vertical,
            top_right: horizontal,
            bottom_left: horizontal,
            bottom_right: vertical,
        }
    }
}

#[derive(Debug, Default, Clone)]
pub enum BorderStyle {
    #[default]
    Solid,
    Dashed,
    Dotted,
}

#[derive(Debug, Default, Clone, Copy)]
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

