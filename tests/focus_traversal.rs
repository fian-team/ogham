//! Stage 3 — Tab / Shift-Tab focus traversal (`UI::focus_next`).
//!
//! Builds a UI tree directly (no Skia surface) and exercises the tab-order
//! ring: ordering, wrap-around, reverse, and confinement to a focus_trap
//! portal's subtree.

use std::sync::{Arc, Mutex};

use ogham::widget::flex_widget::FlexWidget;
use ogham::widget::portal_widget::PortalWidget;
use ogham::widget::text_input_widget::TextInputWidget;
use ogham::widget::{PortalEntry, WidgetRef, UI};

fn input() -> WidgetRef {
    Arc::new(Mutex::new(TextInputWidget::new()))
}

fn flex_with(children: Vec<WidgetRef>) -> WidgetRef {
    let mut f = FlexWidget::new();
    f.children = children;
    Arc::new(Mutex::new(f))
}

fn same(a: &WidgetRef, b: &WidgetRef) -> bool {
    Arc::ptr_eq(a, b)
}

#[test]
fn tab_cycles_forward_in_tree_order_and_wraps() {
    let a = input();
    let b = input();
    let c = input();
    let root = flex_with(vec![a.clone(), b.clone(), c.clone()]);
    let mut ui = UI::new(root);

    // No focus yet → Tab lands on the first focusable.
    assert!(ui.focus_next(false));
    assert!(same(ui.get_focused().unwrap(), &a));

    assert!(ui.focus_next(false));
    assert!(same(ui.get_focused().unwrap(), &b));

    assert!(ui.focus_next(false));
    assert!(same(ui.get_focused().unwrap(), &c));

    // Wrap back to the first.
    assert!(ui.focus_next(false));
    assert!(same(ui.get_focused().unwrap(), &a));
}

#[test]
fn shift_tab_cycles_backward_and_wraps() {
    let a = input();
    let b = input();
    let root = flex_with(vec![a.clone(), b.clone()]);
    let mut ui = UI::new(root);

    // No focus yet → Shift-Tab lands on the last focusable.
    assert!(ui.focus_next(true));
    assert!(same(ui.get_focused().unwrap(), &b));

    assert!(ui.focus_next(true));
    assert!(same(ui.get_focused().unwrap(), &a));

    // Wrap to the last.
    assert!(ui.focus_next(true));
    assert!(same(ui.get_focused().unwrap(), &b));
}

#[test]
fn traversal_descends_into_nested_flex() {
    // Tree order is pre-order DFS: a, then the nested b, c.
    let a = input();
    let b = input();
    let c = input();
    let nested = flex_with(vec![b.clone(), c.clone()]);
    let root = flex_with(vec![a.clone(), nested]);
    let mut ui = UI::new(root);

    ui.focus_next(false);
    assert!(same(ui.get_focused().unwrap(), &a));
    ui.focus_next(false);
    assert!(same(ui.get_focused().unwrap(), &b));
    ui.focus_next(false);
    assert!(same(ui.get_focused().unwrap(), &c));
}

#[test]
fn no_focusables_is_a_noop() {
    let root = flex_with(vec![flex_with(vec![])]);
    let mut ui = UI::new(root);
    assert!(!ui.focus_next(false));
    assert!(ui.get_focused().is_none());
}

#[test]
fn traversal_confined_to_focus_trap_subtree() {
    // A base input plus a modal (focus_trap) holding two inputs. While the
    // trap is active, Tab must cycle only the modal's two inputs.
    let base = input();
    let m1 = input();
    let m2 = input();

    let mut modal = PortalWidget::new();
    modal.open = true;
    modal.focus_trap = true;
    modal.inner.children = vec![m1.clone(), m2.clone()];
    let modal: WidgetRef = Arc::new(Mutex::new(modal));

    let root = flex_with(vec![base.clone()]);
    let mut ui = UI::new(root);

    let info = modal.lock().unwrap().as_portal().unwrap();
    ui.portal_layers.push(PortalEntry {
        widget: modal.clone(),
        viewport_rect: ogham::widget::rect::Rect::zero(),
        layer: info.layer,
        focus_trap: info.focus_trap,
        cursor: info.cursor,
    });
    ui.sync_focus_stack();

    // Tab cycles only within the modal, never escaping to `base`.
    ui.focus_next(false);
    assert!(same(ui.get_focused().unwrap(), &m1));
    ui.focus_next(false);
    assert!(same(ui.get_focused().unwrap(), &m2));
    ui.focus_next(false);
    assert!(
        same(ui.get_focused().unwrap(), &m1),
        "trap must wrap within the modal, not reach the base input"
    );
}
