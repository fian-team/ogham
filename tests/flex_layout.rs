use std::sync::{Arc, Mutex};

use ogham::tree::{
    flex_widget::FlexWidget,
    style::{Direction, FlexStyle},
    Widget, WidgetRef,
};

fn flex_ref(widget: FlexWidget) -> WidgetRef {
    Arc::new(Mutex::new(widget))
}

#[test]
fn grow_width_allocation_does_not_depend_on_child_direction() {
    // Parent lays out children in a row: second child should receive remaining width,
    // regardless of the child's own flex direction.
    let root_style = FlexStyle::builder()
        .row()
        .width_fixed(500.0)
        .height_fixed(100.0)
        .build();

    let fixed_child = flex_ref(FlexWidget::with_style(
        FlexStyle::builder()
            .width_fixed(100.0)
            .height_fixed(100.0)
            .build(),
    ));

    let grow_child_row = flex_ref(FlexWidget::with_style(
        FlexStyle::builder()
            .row()
            .width_grow(1.0)
            .height_fixed(100.0)
            .build(),
    ));

    let grow_child_col = flex_ref(FlexWidget::with_style(
        FlexStyle::builder()
            .column()
            .width_grow(1.0)
            .height_fixed(100.0)
            .build(),
    ));

    // Case A: grow child is row-direction
    {
        let mut root_a = FlexWidget::with_style(root_style.clone());
        root_a.children = vec![fixed_child.clone(), grow_child_row.clone()];
        root_a.layout(
            0.0,
            0.0,
            &Direction::Column,
            500.0,
            500.0,
            100.0,
            100.0,
            0.0,
        );

        let grow_guard = grow_child_row.lock().unwrap();
        let grow = grow_guard
            .downcast_ref::<FlexWidget>()
            .expect("expected FlexWidget");
        let rect = grow.layout.as_ref().expect("expected layout");
        assert!(
            (rect.width - 400.0).abs() < 0.001,
            "expected 400px, got {}",
            rect.width
        );
    }

    // Case B: grow child is column-direction (should be identical)
    {
        let mut root_b = FlexWidget::with_style(root_style);
        root_b.children = vec![fixed_child, grow_child_col.clone()];
        root_b.layout(
            0.0,
            0.0,
            &Direction::Column,
            500.0,
            500.0,
            100.0,
            100.0,
            0.0,
        );

        let grow_guard = grow_child_col.lock().unwrap();
        let grow = grow_guard
            .downcast_ref::<FlexWidget>()
            .expect("expected FlexWidget");
        let rect = grow.layout.as_ref().expect("expected layout");
        assert!(
            (rect.width - 400.0).abs() < 0.001,
            "expected 400px, got {}",
            rect.width
        );
    }
}
