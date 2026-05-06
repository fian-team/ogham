//! Phase 2.5 M2 — key suppression contract tests.
//!
//! `Runtime::consumes_character_key` is true when a focused
//! widget claims character keys. Hosts (lorekeeper-side input
//! pump) consult this before pumping character events to the
//! game pump. Per UL `UI_RUNTIME.md` §2: ONLY
//! `Key::Character(_)` is suppressed; Escape, F-keys, arrows,
//! Tab, and modifiers always pass through.
//!
//! These tests exercise the ogham-side surface
//! (`UI::consumes_character_key`). The lorekeeper-side input
//! pump's adoption is Pass 2 work — see
//! `UL_ADOPTION_READINESS.md` §2.2.

use std::sync::{Arc, Mutex};

use ogham::widget::flex_widget::FlexWidget;
use ogham::widget::text_input_widget::TextInputWidget;
use ogham::widget::{Widget, WidgetRef, UI};

fn make_flex() -> WidgetRef {
    Arc::new(Mutex::new(FlexWidget::new()))
}

fn make_text_input() -> WidgetRef {
    Arc::new(Mutex::new(TextInputWidget::new()))
}

#[test]
fn consumes_character_key_false_when_nothing_focused() {
    let root = make_flex();
    let ui = UI::new(root);
    assert!(!ui.consumes_character_key());
}

#[test]
fn consumes_character_key_false_when_flex_focused() {
    let root = make_flex();
    let mut ui = UI::new(root);
    let f = make_flex();
    ui.try_set_focus(f);
    assert!(
        !ui.consumes_character_key(),
        "non-text-input widgets don't claim character keys"
    );
}

#[test]
fn consumes_character_key_true_when_text_input_focused() {
    let root = make_flex();
    let mut ui = UI::new(root);
    let ti = make_text_input();
    ui.try_set_focus(ti);
    assert!(
        ui.consumes_character_key(),
        "focused TextInput should consume character keys"
    );
}

#[test]
fn text_input_claims_character_keys_at_widget_level() {
    // Sanity that the trait method is wired. Tests above
    // verify the UI-level integration; this verifies the
    // widget itself.
    let ti = TextInputWidget::new();
    assert!(ti.claims_character_keys());
}

#[test]
fn flex_widget_does_not_claim_character_keys() {
    let f = FlexWidget::new();
    assert!(!f.claims_character_keys());
}
