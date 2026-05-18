//! `Presence { style: ... }` parsing: the builder must apply
//! `style:` overrides to the inner Flex so authors can wrap
//! shrink-height content in a shrink-height Presence (or any
//! other non-default layout) instead of being stuck with the
//! default Grow × Grow column.

use ogham::widget::flex_widget::FlexWidget;
use ogham::widget::presence_widget::PresenceWidget;
use ogham::widget::style::{Direction, Size};
use ogham::Ogham;

fn build(source: &str) -> Ogham {
    Ogham::from_source(source, ogham::runtime::config::RuntimeConfig::default())
        .expect("Ogham::from_source")
}

#[test]
fn presence_style_overrides_default_grow() {
    let src = r#"
        let main = fn () {
          Presence {
            key: "a",
            style: { width: "grow", height: "shrink", direction: "column" },
            children: [Flex { children: [] }],
          }
        };
    "#;
    let mut ogham = build(src);
    let root = ogham.get_ui_mut().root.clone();
    let guard = root.lock().expect("root lock");
    let presence = guard
        .downcast_ref::<PresenceWidget>()
        .expect("root should be a PresenceWidget");
    assert_eq!(presence.inner.declared_style.width, Size::Grow(1.0));
    assert_eq!(presence.inner.declared_style.height, Size::Shrink);
    assert_eq!(presence.inner.declared_style.direction, Direction::Column);
    // style mirrors declared at first mount.
    assert_eq!(presence.inner.style.height, Size::Shrink);
}

#[test]
fn presence_without_style_uses_default_grow() {
    // No `style:` — original Grow × Grow column behavior is preserved.
    let src = r#"
        let main = fn () {
          Presence {
            key: "a",
            children: [Flex { children: [] }],
          }
        };
    "#;
    let mut ogham = build(src);
    let root = ogham.get_ui_mut().root.clone();
    let guard = root.lock().expect("root lock");
    let presence = guard
        .downcast_ref::<PresenceWidget>()
        .expect("root should be a PresenceWidget");
    assert_eq!(presence.inner.declared_style.width, Size::Grow(1.0));
    assert_eq!(presence.inner.declared_style.height, Size::Grow(1.0));
    assert_eq!(presence.inner.declared_style.direction, Direction::Column);
}

#[test]
fn presence_style_can_pick_row_direction() {
    let src = r#"
        let main = fn () {
          Presence {
            key: "a",
            style: { direction: "row" },
            children: [Flex { children: [] }],
          }
        };
    "#;
    let mut ogham = build(src);
    let root = ogham.get_ui_mut().root.clone();
    let guard = root.lock().expect("root lock");
    let presence = guard
        .downcast_ref::<PresenceWidget>()
        .expect("root should be a PresenceWidget");
    assert_eq!(presence.inner.declared_style.direction, Direction::Row);
    // Defaults preserved for unspecified fields.
    assert_eq!(presence.inner.declared_style.width, Size::Grow(1.0));
    assert_eq!(presence.inner.declared_style.height, Size::Grow(1.0));
}

#[test]
fn presence_holds_child_that_inherits_its_shrink_height() {
    // Layout smoke test: a shrink-height Presence inside a shrink
    // column should size to its content rather than forcing the
    // parent to allocate Grow space. Lay out at a fixed window and
    // verify the Presence's resolved height equals its single child's.
    let src = r#"
        let main = fn () {
          Flex {
            style: { width: 200, height: "shrink", direction: "column" },
            children: [
              Presence {
                key: "a",
                style: { width: "grow", height: "shrink", direction: "column" },
                children: [
                  Flex {
                    style: { width: "grow", height: 48 },
                    children: [],
                  },
                ],
              },
            ],
          }
        };
    "#;
    let mut ogham = build(src);
    ogham.set_screen_size(800.0, 600.0);
    ogham.get_ui_mut().layout(800.0, 600.0);

    let root = ogham.get_ui_mut().root.clone();
    // root is a FlexWidget; descend into its single Presence child.
    let presence_ref = {
        let g = root.lock().expect("root lock");
        let flex = g
            .downcast_ref::<FlexWidget>()
            .expect("root should be FlexWidget");
        flex.children
            .first()
            .cloned()
            .expect("Flex should have a Presence child")
    };
    let g = presence_ref.lock().expect("presence lock");
    let presence = g
        .downcast_ref::<PresenceWidget>()
        .expect("child should be a Presence");
    let rect = presence
        .inner
        .layout
        .as_ref()
        .expect("Presence should have a laid-out rect");
    // The child is height 48; with shrink height, the Presence sizes to fit.
    assert!(
        (rect.height - 48.0).abs() < 0.5,
        "Presence height should match its single 48-tall child; got {}",
        rect.height
    );
}
