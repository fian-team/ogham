//! Phase 2.5 M3 — hot-reload lifecycle reset tests.
//!
//! `Ogham::reload_file` and `Ogham::recompile_from_source`
//! call `ui.clear_lifecycle_state()` before swapping in the
//! new UI. Prevents stale focus restoration pointing at
//! widgets that no longer exist in the reloaded tree.
//!
//! Note: directly poking the old UI's state to verify the
//! clear isn't easy from outside the Ogham facade — once
//! reload swaps `self.ui`, the old UI is dropped. Instead,
//! these tests invoke clear_lifecycle_state on a UI directly
//! (the load-bearing piece) and verify reload paths run
//! cleanly without leaving stale focus.

use std::sync::{Arc, Mutex};

use ogham::widget::flex_widget::FlexWidget;
use ogham::widget::portal_layer::PortalLayer;
use ogham::widget::portal_widget::PortalWidget;
use ogham::widget::{PortalEntry, WidgetRef, UI};

fn make_flex() -> WidgetRef {
    Arc::new(Mutex::new(FlexWidget::new()))
}

fn make_portal_with_focus_trap() -> WidgetRef {
    let mut p = PortalWidget::new();
    p.open = true;
    p.focus_trap = true;
    p.layer = PortalLayer::OverlayModal;
    Arc::new(Mutex::new(p))
}

#[test]
fn clear_lifecycle_state_clears_all_three_pieces() {
    // Sanity that the load-bearing call zeroes everything
    // the hot-reload path needs zeroed.
    let root = make_flex();
    let mut ui = UI::new(root);
    let modal = make_portal_with_focus_trap();
    ui.portal_layers.push(PortalEntry {
        widget: modal.clone(),
        viewport_rect: ogham::widget::rect::Rect::zero(),
        layer: PortalLayer::OverlayModal,
        focus_trap: true,
        cursor: ogham::widget::portal_layer::CursorPreference::Free,
    });
    ui.sync_focus_stack();
    ui.try_set_focus(modal.clone());

    assert!(!ui.portal_layers.is_empty());
    assert!(!ui.focus_stack.is_empty());
    assert!(ui.get_focused().is_some());

    ui.clear_lifecycle_state();

    assert!(ui.portal_layers.is_empty());
    assert!(ui.focus_stack.is_empty());
    assert!(ui.get_focused().is_none());
}

#[test]
fn clear_lifecycle_state_idempotent() {
    // Calling clear repeatedly on an already-clean UI is fine.
    let root = make_flex();
    let mut ui = UI::new(root);
    ui.clear_lifecycle_state();
    ui.clear_lifecycle_state();
    assert!(ui.portal_layers.is_empty());
    assert!(ui.focus_stack.is_empty());
    assert!(ui.get_focused().is_none());
}

#[test]
fn ogham_recompile_from_source_runs_cleanly() {
    // The integration: recompile_from_source should run
    // without panicking even when the old UI had focus
    // state. Hard to assert the OLD UI's clear directly
    // (it's dropped); we assert the new UI is in clean state
    // afterward.
    let src1 = "let main = fn () { Flex { children: [] } };";
    let src2 = "let main = fn () { Flex { children: [Flex { children: [] }] } };";
    let mut ogham = ogham::Ogham::from_source(
        src1,
        ogham::runtime::config::RuntimeConfig::default(),
    )
    .expect("create");

    // Force the OLD UI to have some focus state.
    let f = make_flex();
    ogham.get_ui_mut().try_set_focus(f);
    assert!(ogham.get_ui().get_focused().is_some());

    // Recompile.
    ogham
        .recompile_from_source(src2)
        .expect("recompile");

    // New UI is clean. Note: the old UI's focus was on a
    // widget from the OLD tree that's no longer reachable —
    // clear_lifecycle_state on the old UI ensures we don't
    // try to restore that focus into the new tree.
    assert!(
        ogham.get_ui().get_focused().is_none(),
        "new UI should start with no focused widget"
    );
    assert!(ogham.get_ui().focus_stack.is_empty());
    assert!(ogham.get_ui().portal_layers.is_empty());
}
