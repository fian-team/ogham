use std::sync::{Arc, Mutex};

use crate::event::Event;
use crate::style::{Direction, TextAlign};
use crate::svg_widget::SvgWidget;
use crate::text_widget::TextWidget;
use crate::{
    style::{
        Alignment, Border, BorderSide, BorderStyle, Color, CornerRadii, FlexStyle, Margin, Padding,
        Size,
    },
    FlexWidget,
};
use crate::{TextInputWidget, WidgetRef};

pub fn button(
    label: String,
    width: Option<Size>,
    height: Option<Size>,
    mouse_up_listener: Box<dyn Fn(&Event)>,
) -> WidgetRef {
    let width = width.unwrap_or(Size::Fixed(200.0));
    let height = height.unwrap_or(Size::Fixed(32.0));
    let mut widget = FlexWidget::with_style(FlexStyle {
        background_color: Some(Color::new(230, 230, 255, 255)),
        width,
        height,
        main_alignment: Alignment::Center,
        cross_alignment: Alignment::Center,
        padding: Padding {
            top: 4.0,
            right: 4.0,
            bottom: 4.0,
            left: 4.0,
        },
        border: Border {
            top: BorderSide {
                width: 2.0,
                color: Color::new(0, 0, 0, 255), // Red
                style: BorderStyle::Solid,
            },
            right: BorderSide {
                width: 2.0,
                color: Color::new(0, 0, 0, 255), // Green
                style: BorderStyle::Solid,
            },
            bottom: BorderSide {
                width: 2.0,
                color: Color::new(0, 0, 0, 255), // Blue
                style: BorderStyle::Solid,
            },
            left: BorderSide {
                width: 2.0,
                color: Color::new(0, 0, 0, 255), // Yellow
                style: BorderStyle::Solid,
            },
        },
        corner_radii: CornerRadii::all(8.0),
        ..Default::default()
    });
    widget
        .event_listeners
        .insert("mouse_up".to_string(), vec![mouse_up_listener]);
    let mut text = TextWidget::new(label);
    text.style.align = TextAlign::Center;
    widget.add_child(Arc::new(Mutex::new(text)));
    Arc::new(Mutex::new(widget))
}

pub fn icon_button(label: String, mouse_down_listener: Box<dyn Fn(&Event)>) -> WidgetRef {
    let mut widget = FlexWidget::with_style(FlexStyle {
        background_color: Some(Color::new(230, 230, 255, 255)),
        width: Size::Fixed(40.0),
        height: Size::Fixed(40.0),
        main_alignment: Alignment::Center,
        cross_alignment: Alignment::Center,
        padding: Padding {
            top: 4.0,
            right: 4.0,
            bottom: 4.0,
            left: 4.0,
        },
        margin: Margin {
            top: 4.0,
            right: 4.0,
            bottom: 4.0,
            left: 4.0,
        },
        border: Border {
            top: BorderSide {
                width: 2.0,
                color: Color::new(0, 0, 0, 255), // Red
                style: BorderStyle::Solid,
            },
            right: BorderSide {
                width: 2.0,
                color: Color::new(0, 0, 0, 255), // Green
                style: BorderStyle::Solid,
            },
            bottom: BorderSide {
                width: 2.0,
                color: Color::new(0, 0, 0, 255), // Blue
                style: BorderStyle::Solid,
            },
            left: BorderSide {
                width: 2.0,
                color: Color::new(0, 0, 0, 255), // Yellow
                style: BorderStyle::Solid,
            },
        },
        ..Default::default()
    });
    widget
        .event_listeners
        .insert("mouse_down".to_string(), vec![mouse_down_listener]);
    widget
        .event_listeners
        .insert("mouse_up".to_string(), vec![]);
    let mut text = TextWidget::new(label);
    text.style.align = TextAlign::Center;
    widget.add_child(Arc::new(Mutex::new(text)));
    Arc::new(Mutex::new(widget))
}

pub fn panel(
    children: Vec<WidgetRef>,
    direction: Direction,
    width: Size,
    height: Size,
) -> WidgetRef {
    let mut widget = FlexWidget::with_style(FlexStyle {
        background_color: Some(Color::new(85, 85, 85, 255)),
        padding: Padding {
            top: 12.0,
            right: 12.0,
            bottom: 12.0,
            left: 12.0,
        },
        margin: Margin {
            top: 0.0,
            right: 0.0,
            bottom: 0.0,
            left: 0.0,
        },
        corner_radii: CornerRadii::all(8.0),
        border: Border {
            top: BorderSide {
                width: 0.0,
                color: Color::new(0, 0, 0, 255),
                style: BorderStyle::Solid,
            },
            right: BorderSide {
                width: 0.0,
                color: Color::new(0, 0, 0, 255),
                style: BorderStyle::Solid,
            },
            bottom: BorderSide {
                width: 0.0,
                color: Color::new(0, 0, 0, 255),
                style: BorderStyle::Solid,
            },
            left: BorderSide {
                width: 0.0,
                color: Color::new(0, 0, 0, 255),
                style: BorderStyle::Solid,
            },
        },
        direction,
        gap: 8.0,
        width,
        height,
        ..Default::default()
    });
    for child in children {
        widget.add_child(child);
    }
    Arc::new(Mutex::new(widget))
}

pub fn panel_label(label: String) -> WidgetRef {
    let mut widget = FlexWidget::with_style(FlexStyle {
        width: Size::Grow(1.0),
        height: Size::Fixed(24.0),
        ..Default::default()
    });
    let mut text = TextWidget::with_color(label, Color::new(224, 224, 224, 255));
    text.style.align = TextAlign::Left;
    widget.add_child(Arc::new(Mutex::new(text)));
    Arc::new(Mutex::new(widget))
}

pub fn panel_icon_button(
    icon: String,
    mouse_down_listener: Box<dyn Fn(&Event)>,
    active: bool,
) -> WidgetRef {
    let mut widget = FlexWidget::with_style(FlexStyle {
        width: Size::Fixed(34.0),
        height: Size::Fixed(34.0),
        main_alignment: Alignment::Center,
        cross_alignment: Alignment::Center,
        padding: Padding {
            top: 4.0,
            right: 4.0,
            bottom: 4.0,
            left: 4.0,
        },
        corner_radii: CornerRadii::all(8.0),
        background_color: if active {
            Some(Color::new(68, 68, 68, 255))
        } else {
            Some(Color::new(85, 85, 85, 255))
        },
        ..Default::default()
    });
    let svg_widget = SvgWidget::new(icon, 24.0, 24.0, Some(Color::new(224, 224, 224, 255)));
    widget.add_child(Arc::new(Mutex::new(svg_widget)));
    widget
        .event_listeners
        .insert("mouse_down".to_string(), vec![mouse_down_listener]);
    Arc::new(Mutex::new(widget))
}

pub fn text_input(
    label: Option<String>,
    width: Size,
    value: String,
    on_change_listener: Option<Box<dyn Fn(&Event)>>,
) -> WidgetRef {
    let mut container = FlexWidget::with_style(FlexStyle {
        width,
        height: Size::Fixed(40.0),
        direction: Direction::Column,
        corner_radii: CornerRadii::all(8.0),
        ..Default::default()
    });
    if let Some(label) = label {
        container.style.height = Size::Fixed(60.0);
        let mut text_container = FlexWidget::with_style(FlexStyle {
            width,
            height: Size::Fixed(20.0),
            ..Default::default()
        });
        let mut text = TextWidget::new(label);
        text.style.color = Color::new(224, 224, 224, 255);
        text_container.add_child(Arc::new(Mutex::new(text)));
        container.add_child(Arc::new(Mutex::new(text_container)));
    }

    let mut text_input = TextInputWidget::with_style(FlexStyle {
        background_color: Some(Color::new(68, 68, 68, 255)),
        text_color: Some(Color::new(224, 224, 224, 255)),
        ..Default::default()
    });
    text_input.set_value(value);
    text_input.style.width = Size::Fixed(150.0);
    text_input.style.height = Size::Fixed(40.0);
    text_input.style.padding = Padding {
        top: 10.0,
        right: 10.0,
        bottom: 10.0,
        left: 10.0,
    };
    text_input.style.border = Border {
        top: BorderSide {
            color: Color::new(224, 224, 224, 255),
            width: 1.0,
            style: BorderStyle::Solid,
        },
        right: BorderSide {
            color: Color::new(224, 224, 224, 255),
            width: 1.0,
            style: BorderStyle::Solid,
        },
        bottom: BorderSide {
            color: Color::new(224, 224, 224, 255),
            width: 1.0,
            style: BorderStyle::Solid,
        },
        left: BorderSide {
            color: Color::new(224, 224, 224, 255),
            width: 1.0,
            style: BorderStyle::Solid,
        },
        ..Default::default()
    };
    if let Some(on_change_listener) = on_change_listener {
        text_input
            .event_listeners
            .insert("on_change".to_string(), vec![on_change_listener]);
    }
    container.add_child(Arc::new(Mutex::new(text_input)));
    Arc::new(Mutex::new(container))
}

/// Creates a widget with a background image.
///
/// This is a convenience function for creating a FlexWidget with a background image.
/// The image will be loaded and cached automatically on first render.
///
/// # Arguments
///
/// * `image_path` - Path to the image file (supports PNG format)
/// * `width` - Optional width for the container (defaults to 400px)
/// * `height` - Optional height for the container (defaults to 300px)
///
/// # Example
///
/// ```rust
/// use ui::components::background_image_container;
/// use ui::style::Size;
///
/// let container = background_image_container(
///     "data/assets/background_homestead.jpg".to_string(),
///     Some(Size::Fixed(800.0)),
///     Some(Size::Fixed(600.0))
/// );
/// ```
pub fn background_image_container(
    image_path: String,
    width: Option<Size>,
    height: Option<Size>,
) -> WidgetRef {
    let width = width.unwrap_or(Size::Fixed(400.0));
    let height = height.unwrap_or(Size::Fixed(300.0));

    let widget = FlexWidget::with_style(FlexStyle {
        width,
        height,
        background_image: Some(image_path),
        main_alignment: Alignment::Center,
        cross_alignment: Alignment::Center,
        padding: Padding::all(16.0),
        ..Default::default()
    });
    Arc::new(Mutex::new(widget))
}

pub fn row(
    children: Vec<WidgetRef>,
    main_alignment: Alignment,
    cross_alignment: Alignment,
    gap: f32,
) -> WidgetRef {
    let mut widget = FlexWidget::with_style(FlexStyle {
        direction: Direction::Row,
        main_alignment,
        cross_alignment,
        gap,
        width: Size::Grow(1.0),
        height: Size::Shrink,
        ..Default::default()
    });
    for child in children {
        widget.add_child(child);
    }
    Arc::new(Mutex::new(widget))
}

pub fn column(
    children: Vec<WidgetRef>,
    main_alignment: Alignment,
    cross_alignment: Alignment,
    gap: f32,
) -> WidgetRef {
    let mut widget = FlexWidget::with_style(FlexStyle {
        direction: Direction::Column,
        main_alignment,
        cross_alignment,
        gap,
        width: Size::Shrink,
        height: Size::Grow(1.0),
        ..Default::default()
    });
    for child in children {
        widget.add_child(child);
    }
    Arc::new(Mutex::new(widget))
}

pub fn checkbox(
    label: String,
    checked: bool,
    on_change_listener: Option<Box<dyn Fn(&Event)>>,
) -> WidgetRef {
    let container = row(vec![], Alignment::Start, Alignment::Center, 4.0);
    let mut widget = FlexWidget::with_style(FlexStyle {
        width: Size::Fixed(32.0),
        height: Size::Fixed(32.0),
        background_color: if checked {
            Some(Color::new(230, 230, 255, 255))
        } else {
            Some(Color::new(120, 120, 120, 255))
        },
        corner_radii: CornerRadii::all(8.0),
        ..Default::default()
    });
    if let Some(on_change_listener) = on_change_listener {
        widget
            .event_listeners
            .insert("mouse_down".to_string(), vec![on_change_listener]);
    }
    {
        let mut container = container.lock().unwrap();
        let container = container.downcast_mut::<FlexWidget>().unwrap();
        container.add_child(Arc::new(Mutex::new(widget)));
        let mut text_container = FlexWidget::with_style(FlexStyle {
            width: Size::Grow(1.0),
            height: Size::Fixed(20.0),
            ..Default::default()
        });
        let mut text = TextWidget::new(label);
        text.style.color = Color::new(224, 224, 224, 255);
        text_container.add_child(Arc::new(Mutex::new(text)));
        container.add_child(Arc::new(Mutex::new(text_container)));
    }
    container
}
