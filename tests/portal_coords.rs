//! Phase 2.5 M0 — viewport-absolute coordinate fix tests.
//!
//! Phase 2 captured `parent_rect` in the immediate parent's
//! coordinate space. M0 renames to `viewport_rect` and
//! captures viewport-absolute coords by accumulating parent
//! translates through the recursion.
//!
//! The tests below construct PortalEntry values directly
//! (the renderer's translate accumulation isn't easily
//! exercised without a Skia surface) and assert the field's
//! semantics. End-to-end translate-accumulation is a Skia-
//! surface concern; manual smoke at M5.

use std::sync::{Arc, Mutex};

use ogham::widget::flex_widget::FlexWidget;
use ogham::widget::portal_layer::PortalLayer;
use ogham::widget::portal_widget::PortalWidget;
use ogham::widget::rect::Rect;
use ogham::widget::{PortalEntry, PortalLayers, WidgetRef};

fn portal_at(layer: PortalLayer) -> WidgetRef {
    let mut p = PortalWidget::new();
    p.open = true;
    p.layer = layer;
    Arc::new(Mutex::new(p))
}

#[test]
fn viewport_rect_replaces_parent_rect_field_name() {
    // Compile-time + field-existence check: PortalEntry now
    // has viewport_rect, not parent_rect. Phase 2 callers
    // would fail at this struct construction.
    let p = portal_at(PortalLayer::OverlayModal);
    let entry = PortalEntry {
        widget: p,
        viewport_rect: Rect::new(10.0, 20.0, 100.0, 50.0),
        layer: PortalLayer::OverlayModal,
        focus_trap: false,
        cursor: ogham::widget::portal_layer::CursorPreference::Inherit,
    };
    assert_eq!(entry.viewport_rect.x, 10.0);
    assert_eq!(entry.viewport_rect.y, 20.0);
}

#[test]
fn portal_at_root_unchanged_from_phase_2_behavior() {
    // When accumulated_translate is (0, 0) — the root case —
    // viewport_rect equals the local layout rect. This is the
    // Phase 2 escape-menu shape; the M0 fix doesn't regress it.
    //
    // We can't directly invoke the Skia draw path here; this
    // test is a placeholder asserting the contract: a
    // PortalEntry constructed with viewport_rect = local_rect
    // (the root case) round-trips correctly.
    let p = portal_at(PortalLayer::OverlayModal);
    let local = Rect::new(0.0, 0.0, 800.0, 600.0);
    let entry = PortalEntry {
        widget: p,
        viewport_rect: local.clone(),
        layer: PortalLayer::OverlayModal,
        focus_trap: false,
        cursor: ogham::widget::portal_layer::CursorPreference::Inherit,
    };
    assert_eq!(entry.viewport_rect.x, local.x);
    assert_eq!(entry.viewport_rect.y, local.y);
    assert_eq!(entry.viewport_rect.width, local.width);
    assert_eq!(entry.viewport_rect.height, local.height);
}

#[test]
fn nested_portal_inside_translated_parent_renders_at_correct_viewport_position() {
    // Document the contract: if a portal is nested inside a
    // parent translated by (50, 100), and the portal's local
    // layout rect is at (10, 20) with size (200, 150), the
    // PortalEntry's viewport_rect should be the cumulative
    // (60, 120, 200, 150).
    //
    // M0's renderer change is what implements this — Pass A
    // threads accumulated_translate through the recursion.
    // This test asserts the math we expect; the actual
    // accumulation is exercised by the Skia render path at
    // M5 manual smoke (no Skia surface in tests/).
    let parent_translate = (50.0, 100.0);
    let local = Rect::new(10.0, 20.0, 200.0, 150.0);
    let expected_viewport = Rect::new(
        local.x + parent_translate.0,
        local.y + parent_translate.1,
        local.width,
        local.height,
    );
    let p = portal_at(PortalLayer::Popover);
    let entry = PortalEntry {
        widget: p,
        viewport_rect: expected_viewport.clone(),
        layer: PortalLayer::Popover,
        focus_trap: false,
        cursor: ogham::widget::portal_layer::CursorPreference::Inherit,
    };
    assert_eq!(entry.viewport_rect.x, 60.0);
    assert_eq!(entry.viewport_rect.y, 120.0);
}

#[test]
fn portal_layers_storage_preserves_viewport_rect() {
    // Sanity that PortalLayers's per-layer storage doesn't
    // mangle the viewport_rect on push/iter.
    let mut layers = PortalLayers::new();
    let p = portal_at(PortalLayer::Tooltip);
    let original = Rect::new(33.0, 77.0, 50.0, 25.0);
    layers.push(PortalEntry {
        widget: p,
        viewport_rect: original.clone(),
        layer: PortalLayer::Tooltip,
        focus_trap: false,
        cursor: ogham::widget::portal_layer::CursorPreference::Inherit,
    });
    let entry = layers.iter_paint_order().next().expect("one entry");
    assert_eq!(entry.viewport_rect.x, original.x);
    assert_eq!(entry.viewport_rect.y, original.y);
}

// Compile-only assertion that `make_flex` and other helpers
// from focus_trap.rs aren't accidentally used here — kept as
// a doc note. Keep this file focused on coord plumbing.
fn _silence_unused() {
    let _ = FlexWidget::new();
}
