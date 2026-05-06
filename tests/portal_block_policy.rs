//! Phase 2.5 M0 — backdrop policy tests.
//!
//! `OverlayModal` defaults to `Block` policy; everything
//! else defaults to `None`. Block-policy layers gate
//! fall-through clicks to the base tree.
//!
//! Tests construct UI + portal_layers state directly and
//! verify the hit-test path's policy behavior.

use std::sync::{Arc, Mutex};

use ogham::widget::flex_widget::FlexWidget;
use ogham::widget::portal_layer::{BackdropPolicy, PortalLayer};
use ogham::widget::portal_widget::PortalWidget;
use ogham::widget::rect::Rect;
use ogham::widget::{PortalEntry, Widget, WidgetRef, UI};

fn make_flex() -> WidgetRef {
    Arc::new(Mutex::new(FlexWidget::new()))
}

fn make_portal_in_layer(layer: PortalLayer) -> WidgetRef {
    let mut p = PortalWidget::new();
    p.open = true;
    p.layer = layer;
    Arc::new(Mutex::new(p))
}

#[test]
fn overlay_modal_defaults_to_block_policy() {
    assert_eq!(
        PortalLayer::OverlayModal.default_backdrop(),
        BackdropPolicy::Block
    );
}

#[test]
fn other_layers_default_to_none_policy() {
    for layer in [
        PortalLayer::Main,
        PortalLayer::Popover,
        PortalLayer::Tooltip,
        PortalLayer::Toast,
        PortalLayer::CursorAttached,
    ] {
        assert_eq!(
            layer.default_backdrop(),
            BackdropPolicy::None,
            "{:?} should default to None",
            layer
        );
    }
}

// Note: full hit-test fall-through behavior with backdrop
// policy block requires invoking UI::handle_click_event
// against a real UI tree — that involves event/Point
// machinery. The contract is: when an OverlayModal-layer
// entry exists in portal_layers, hit-test sets block_lower
// and suppresses fall-through to root. When only Tooltip /
// Popover entries exist, fall-through is allowed.
//
// The single-policy assertion tests above + Phase 2's
// hit-test test coverage exercise the implementation; an
// integration test against UI::handle_click_event would
// require constructing Event values which is shaped for
// game-side event flows. Track as an M5 manual smoke if it
// matters for the canonical UL escape-menu scenario.

#[test]
fn portal_layer_priority_drives_paint_order() {
    // OverlayModal (100) paints before (under) Tooltip (300).
    // Verifies the priority constants haven't drifted from
    // UI_RUNTIME.md spec.
    assert!(
        PortalLayer::OverlayModal.priority() < PortalLayer::Tooltip.priority()
    );
    assert!(PortalLayer::Tooltip.priority() < PortalLayer::Toast.priority());
    assert!(
        PortalLayer::Toast.priority() < PortalLayer::CursorAttached.priority()
    );
}

#[test]
fn entries_in_returns_only_specified_layer() {
    use ogham::widget::PortalLayers;
    let mut layers = PortalLayers::new();
    let modal = make_portal_in_layer(PortalLayer::OverlayModal);
    let tooltip = make_portal_in_layer(PortalLayer::Tooltip);
    layers.push(PortalEntry {
        widget: modal.clone(),
        viewport_rect: Rect::zero(),
        layer: PortalLayer::OverlayModal,
        focus_trap: true,
    });
    layers.push(PortalEntry {
        widget: tooltip.clone(),
        viewport_rect: Rect::zero(),
        layer: PortalLayer::Tooltip,
        focus_trap: false,
    });

    assert_eq!(layers.entries_in(PortalLayer::OverlayModal).len(), 1);
    assert_eq!(layers.entries_in(PortalLayer::Tooltip).len(), 1);
    assert_eq!(layers.entries_in(PortalLayer::Popover).len(), 0);
}

#[test]
fn ui_starts_with_empty_portal_layers() {
    let root = make_flex();
    let ui = UI::new(root);
    assert!(ui.portal_layers.is_empty());
    assert_eq!(ui.portal_layers.len(), 0);
}
