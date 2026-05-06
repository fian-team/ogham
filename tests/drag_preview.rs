//! Phase 3 M2 — drag_preview tests.
//!
//! `dispatch_drag_start` reads the originator's
//! `drag_preview()` and stashes it on the UI; `drag_move`
//! updates the cursor; `drag_end` clears it. Renderers
//! consult `UI::active_drag_preview()` each frame and push
//! the preview into the `CursorAttached` portal layer.

use std::sync::{Arc, Mutex};

use ogham::runtime::value::Value;
use ogham::runtime::Runtime;
use ogham::widget::event::DragState;
use ogham::widget::flex_widget::FlexWidget;
use ogham::widget::point::Point;
use ogham::widget::rect::Rect;
use ogham::widget::{builder, Widget, WidgetRef, UI};

fn build_root(src: &str) -> WidgetRef {
    let runtime = Arc::new(Mutex::new(
        Runtime::from_source(src, None).expect("parse"),
    ));
    let module = {
        let rt = runtime.lock().unwrap();
        rt.get_module().expect("module").clone()
    };
    let widget_value = {
        let mut rt = runtime.lock().unwrap();
        rt.execute_module(&module).expect("execute")
    };
    let registry = builder::WidgetRegistry::with_defaults();
    builder::widget_value_to_widget_ref(&registry, &runtime, &widget_value)
        .expect("build widget tree")
}

#[test]
fn drag_preview_default_is_none() {
    let f = FlexWidget::new();
    assert!(f.drag_preview().is_none());
}

#[test]
fn drag_preview_property_round_trips_through_builder() {
    let src = r#"
let main = fn () {
  Flex {
    drag_preview: Flex { children: [] },
  }
};
"#;
    let root = build_root(src);
    let g = root.lock().unwrap();
    assert!(
        g.drag_preview().is_some(),
        "drag_preview should be exposed via the trait"
    );
}

#[test]
fn dispatch_drag_start_captures_preview() {
    // A widget with drag_preview, dispatched: UI exposes the
    // preview for rendering.
    let mut origin = FlexWidget::new();
    origin.layout = Some(Rect::new(0.0, 0.0, 100.0, 100.0));
    let preview: WidgetRef = Arc::new(Mutex::new({
        let mut p = FlexWidget::new();
        p.layout = Some(Rect::new(0.0, 0.0, 32.0, 32.0));
        p
    }));
    origin.drag_preview = Some(preview.clone());
    let origin_ref: WidgetRef = Arc::new(Mutex::new(origin));

    let mut ui = UI::new(origin_ref.clone());
    assert!(ui.active_drag_preview().is_none());

    ui.dispatch_drag_start(origin_ref.clone(), Value::Void, Point::new(50.0, 50.0));
    assert!(
        ui.active_drag_preview().is_some(),
        "drag_start should capture the preview onto UI"
    );
    let captured = ui.active_drag_preview().unwrap();
    assert_eq!(captured.cursor.x(), 50.0);
}

#[test]
fn dispatch_drag_move_updates_preview_cursor() {
    let mut origin = FlexWidget::new();
    origin.layout = Some(Rect::new(0.0, 0.0, 100.0, 100.0));
    origin.drag_preview = Some(Arc::new(Mutex::new(FlexWidget::new())));
    let origin_ref: WidgetRef = Arc::new(Mutex::new(origin));

    let mut ui = UI::new(origin_ref.clone());
    let mut state = ui.dispatch_drag_start(
        origin_ref.clone(),
        Value::Void,
        Point::new(10.0, 10.0),
    );
    ui.dispatch_drag_move(&mut state, Point::new(40.0, 70.0));
    let captured = ui.active_drag_preview().expect("preview present");
    assert_eq!(captured.cursor.x(), 40.0);
    assert_eq!(captured.cursor.y(), 70.0);
}

#[test]
fn dispatch_drag_end_clears_preview() {
    let mut origin = FlexWidget::new();
    origin.layout = Some(Rect::new(0.0, 0.0, 100.0, 100.0));
    origin.drag_preview = Some(Arc::new(Mutex::new(FlexWidget::new())));
    let origin_ref: WidgetRef = Arc::new(Mutex::new(origin));

    let mut ui = UI::new(origin_ref.clone());
    let mut state = ui.dispatch_drag_start(
        origin_ref.clone(),
        Value::Void,
        Point::new(0.0, 0.0),
    );
    assert!(ui.active_drag_preview().is_some());
    ui.dispatch_drag_end(&mut state, Point::new(50.0, 50.0));
    assert!(
        ui.active_drag_preview().is_none(),
        "drag_end should clear the preview"
    );
}

#[test]
fn widget_without_drag_preview_doesnt_capture() {
    let mut origin = FlexWidget::new();
    origin.layout = Some(Rect::new(0.0, 0.0, 100.0, 100.0));
    // No drag_preview set.
    let origin_ref: WidgetRef = Arc::new(Mutex::new(origin));

    let mut ui = UI::new(origin_ref.clone());
    ui.dispatch_drag_start(origin_ref.clone(), Value::Void, Point::new(0.0, 0.0));
    assert!(
        ui.active_drag_preview().is_none(),
        "no drag_preview → no UI preview state"
    );
}

#[test]
fn drag_preview_gets_laid_out_at_zero_origin_during_ui_layout() {
    // Phase 3 M2: UI::layout pass runs the drag_preview at
    // origin (0,0) sized at its declared dimensions so the
    // Skia paint path can translate the synthetic
    // CursorAttached entry by viewport_rect=cursor without
    // double-offset. This test verifies the layout pass
    // actually runs against the active preview.
    use ogham::widget::style::{FlexStyle, Size};

    let mut origin = FlexWidget::new();
    origin.layout = Some(Rect::new(0.0, 0.0, 100.0, 100.0));
    let mut preview = FlexWidget::new();
    let mut style = FlexStyle::default();
    style.width = Size::Fixed(64.0);
    style.height = Size::Fixed(48.0);
    preview.declared_style = style.clone();
    preview.style = style;
    let preview_ref: WidgetRef = Arc::new(Mutex::new(preview));
    origin.drag_preview = Some(preview_ref.clone());
    let origin_ref: WidgetRef = Arc::new(Mutex::new(origin));

    let mut ui = UI::new(origin_ref.clone());
    ui.dispatch_drag_start(
        origin_ref.clone(),
        Value::Void,
        Point::new(50.0, 50.0),
    );
    ui.layout(800.0, 600.0);

    let g = preview_ref.lock().unwrap();
    let rect = g.get_layout_rect().expect("preview should have layout rect");
    assert_eq!(rect.x, 0.0, "preview laid out at local origin x=0");
    assert_eq!(rect.y, 0.0, "preview laid out at local origin y=0");
    assert_eq!(rect.width, 64.0, "preview width from declared size");
    assert_eq!(rect.height, 48.0, "preview height from declared size");
}

// Suppress unused-import warning when the file changes shape.
const _: Option<DragState> = None;
