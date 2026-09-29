//! `max_height` on a Flex style — CSS `max-height`: a ceiling on whatever
//! height the sizing rules resolve. The fixtures pair a box that stops at
//! its ceiling with one that stays below it, so a ceiling that clamped
//! everything, or nothing, fails one of them.

use ogham::runtime::config::RuntimeConfig;
use ogham::widget::vocabulary::scan_source;
use ogham::widget::WidgetRef;
use ogham::Ogham;

fn find(node: &WidgetRef, key: &str) -> Option<WidgetRef> {
    let children = {
        let g = node.lock().unwrap();
        if g.key() == Some(key) {
            return Some(node.clone());
        }
        g.get_children()
    };
    children.iter().find_map(|c| find(c, key))
}

fn height_of(o: &Ogham, key: &str) -> f32 {
    let node = find(&o.get_ui().root, key).unwrap_or_else(|| panic!("no widget keyed {key}"));
    let g = node.lock().unwrap();
    g.get_layout_rect().expect("laid out").height
}

fn settled(src: &str) -> Ogham {
    let mut o = Ogham::from_source(src, RuntimeConfig::default()).expect("from_source");
    for _ in 0..4 {
        o.frame(800.0, 600.0, 1.0 / 60.0).expect("frame");
    }
    o
}

/// Five 40-px rows in a shrink column: 200 px of content. Capped at 120 it
/// stops there; capped at 500 it is its content.
#[test]
fn a_shrink_box_grows_to_its_ceiling_and_no_further() {
    let src = r#"
let main = fn () {
  Flex { style: { width: "grow", height: "grow", direction: "row", cross_alignment: "start" }, children: [
    Flex { key: "capped", style: { width: 100, height: "shrink", max_height: 120, direction: "column", overflow: "scroll" }, children: for (i in 0..5) { Flex { style: { width: 100, height: 40 } } } },
    Flex { key: "roomy", style: { width: 100, height: "shrink", max_height: 500, direction: "column" }, children: for (i in 0..5) { Flex { style: { width: 100, height: 40 } } } },
  ] }
};"#;
    let o = settled(src);
    assert_eq!(height_of(&o, "capped"), 120.0, "stopped at its ceiling");
    assert_eq!(height_of(&o, "roomy"), 200.0, "below its ceiling it is its content");
}

/// The ceiling wins over a fixed height, as CSS's does, and a column's
/// later children are laid out under the clamped box, not the natural one.
#[test]
fn the_ceiling_wins_over_a_fixed_height_and_moves_what_follows() {
    let src = r#"
let main = fn () {
  Flex { style: { width: "grow", height: "grow", direction: "column" }, children: [
    Flex { key: "tall", style: { width: 100, height: 300, max_height: 80 } },
    Flex { key: "next", style: { width: 100, height: 10 } },
  ] }
};"#;
    let o = settled(src);
    assert_eq!(height_of(&o, "tall"), 80.0);
    let next = find(&o.get_ui().root, "next").unwrap();
    let y = next.lock().unwrap().get_layout_rect().unwrap().y;
    assert_eq!(y, 80.0, "the next box starts under the clamped one");
}

#[test]
fn the_vocabulary_knows_the_key() {
    let found = scan_source(
        "fixture.ogh",
        r#"let main = fn () { Flex { style: { max_height: 120 } } };"#,
    );
    assert!(found.is_empty(), "{:?}", found.iter().map(|v| &v.path).collect::<Vec<_>>());
}
