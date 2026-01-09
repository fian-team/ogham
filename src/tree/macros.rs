#[macro_export]
macro_rules! add_child_to {
    ($widget:expr, $child:expr) => {{
        let mut widget_guard = $widget.lock().unwrap();
        let widget = widget_guard
            .downcast_mut::<$crate::flex_widget::FlexWidget>()
            .unwrap();
        widget.add_child($child);
    }};
}
