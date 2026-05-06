//! Phase 2.5 M1 — cursor coordination tests.
//!
//! Verify `Runtime::wants_cursor_free` returns the expected
//! signal for each canonical scenario: nothing active, modal
//! open, tooltip-only open, focused TextInput, explicit
//! cursor: "inherit" override on a modal.

use std::sync::{Arc, Mutex};

use ogham::widget::flex_widget::FlexWidget;
use ogham::widget::portal_layer::{CursorPreference, PortalLayer};
use ogham::widget::portal_widget::PortalWidget;
use ogham::widget::{PortalEntry, Widget, WidgetRef, UI};

fn make_flex() -> WidgetRef {
    Arc::new(Mutex::new(FlexWidget::new()))
}

fn entry_for(
    portal: &WidgetRef,
    layer: PortalLayer,
    cursor: CursorPreference,
) -> PortalEntry {
    PortalEntry {
        widget: portal.clone(),
        viewport_rect: ogham::widget::rect::Rect::zero(),
        layer,
        focus_trap: false,
        cursor,
    }
}

fn make_portal_in(layer: PortalLayer) -> WidgetRef {
    let mut p = PortalWidget::new();
    p.open = true;
    p.layer = layer;
    Arc::new(Mutex::new(p))
}

#[test]
fn wants_cursor_free_false_when_nothing_active() {
    let root = make_flex();
    let ui = UI::new(root);
    assert!(!ui.wants_cursor_free());
}

#[test]
fn wants_cursor_free_true_when_overlay_modal_open() {
    // OverlayModal layer's default cursor preference is Free,
    // so an open modal trips the signal.
    let root = make_flex();
    let mut ui = UI::new(root);
    let modal = make_portal_in(PortalLayer::OverlayModal);
    let info = modal.lock().unwrap().as_portal().unwrap();
    ui.portal_layers
        .push(entry_for(&modal, PortalLayer::OverlayModal, info.cursor));
    assert!(ui.wants_cursor_free(),
        "open OverlayModal portal should declare cursor-free");
}

#[test]
fn wants_cursor_free_false_when_only_tooltip_open() {
    // Tooltip layer defaults to Inherit; no contribution.
    let root = make_flex();
    let mut ui = UI::new(root);
    let tooltip = make_portal_in(PortalLayer::Tooltip);
    let info = tooltip.lock().unwrap().as_portal().unwrap();
    ui.portal_layers
        .push(entry_for(&tooltip, PortalLayer::Tooltip, info.cursor));
    assert!(!ui.wants_cursor_free(),
        "tooltip should not influence cursor state");
}

#[test]
fn wants_cursor_free_true_when_popover_open() {
    // Popover layer defaults to Free (it's interactive).
    let root = make_flex();
    let mut ui = UI::new(root);
    let popover = make_portal_in(PortalLayer::Popover);
    let info = popover.lock().unwrap().as_portal().unwrap();
    ui.portal_layers
        .push(entry_for(&popover, PortalLayer::Popover, info.cursor));
    assert!(ui.wants_cursor_free(),
        "open Popover should declare cursor-free");
}

#[test]
fn wants_cursor_free_combines_portal_and_focus_signals() {
    // Multiple sources can independently declare Free; any
    // one is sufficient.
    let root = make_flex();
    let mut ui = UI::new(root);
    let modal = make_portal_in(PortalLayer::OverlayModal);
    let info = modal.lock().unwrap().as_portal().unwrap();
    ui.portal_layers
        .push(entry_for(&modal, PortalLayer::OverlayModal, info.cursor));
    // Even with the focused widget being a regular Flex (no
    // cursor preference), the modal alone should trip it.
    ui.try_set_focus(make_flex());
    assert!(ui.wants_cursor_free());
}

#[test]
fn wants_cursor_free_false_when_modal_explicitly_inherits() {
    // Explicit cursor: "inherit" overrides the layer default.
    // A modal that opts out doesn't influence cursor.
    let root = make_flex();
    let mut ui = UI::new(root);
    let mut p = PortalWidget::new();
    p.open = true;
    p.layer = PortalLayer::OverlayModal;
    p.cursor = Some(CursorPreference::Inherit);
    let portal_ref: WidgetRef = Arc::new(Mutex::new(p));
    ui.portal_layers.push(entry_for(
        &portal_ref,
        PortalLayer::OverlayModal,
        CursorPreference::Inherit,
    ));
    assert!(
        !ui.wants_cursor_free(),
        "explicit cursor: inherit overrides layer default"
    );
}

#[test]
fn portal_effective_cursor_uses_layer_default_when_unset() {
    let mut p = PortalWidget::new();
    p.layer = PortalLayer::OverlayModal;
    assert_eq!(p.effective_cursor(), CursorPreference::Free);
    p.layer = PortalLayer::Tooltip;
    assert_eq!(p.effective_cursor(), CursorPreference::Inherit);
}

#[test]
fn portal_effective_cursor_uses_explicit_override_when_set() {
    let mut p = PortalWidget::new();
    p.layer = PortalLayer::Tooltip;
    p.cursor = Some(CursorPreference::Free);
    assert_eq!(p.effective_cursor(), CursorPreference::Free);
}
