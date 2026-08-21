//! Strict vocabulary: what the language does not recognise, said out loud.
//!
//! The five instances this exists for were all found by hand, in one
//! afternoon, in one game's UI:
//!
//! | written | meant | what it did |
//! |---|---|---|
//! | `on_click` | `mouse_down` | drew perfectly, never responded |
//! | `font_size` | `size` | a clock, at body size, for its whole life |
//! | `wrap: true` | `flex_wrap` | chip rows that never wrapped |
//! | `wrap: "wrap"` | `flex_wrap` | the same, in another repo |
//! | `cross_alignment: "stretch"` | — | read as `start`; harmless only while the children `"grow"` |
//!
//! Each is a fixture below. The last is the one that matters: it is a
//! *value* and not a key, and it produces layout that looks like it
//! works.
//!
//! Two feeders, and the split is the point. [`scan_source`] reads a file
//! and knows where things are; the builder reads the materialized
//! descriptor and cannot be fooled by a `let`. Neither changes what is
//! drawn.

use std::collections::HashMap;

use ogham::runtime::config::RuntimeConfig;
use ogham::runtime::value::Value;
use ogham::widget::vocabulary::{scan_source, Kind};
use ogham::widget::Widget;
use ogham::Ogham;

// ---------------------------------------------------------------------
// The five, through the source scan.
// ---------------------------------------------------------------------

fn paths(source: &str) -> Vec<String> {
    scan_source("fixture.ogh", source)
        .into_iter()
        .map(|v| v.path)
        .collect()
}

#[test]
fn a_click_listener_that_reaches_nobody() {
    assert_eq!(
        paths(r#"let main = fn () { Flex { on_click: fn () { 1 } } };"#),
        ["on_click"]
    );
}

#[test]
fn a_clock_drawn_at_body_size() {
    assert_eq!(
        paths(r#"let main = fn () { Text { text: "12:04", style: { font_size: 48 } } };"#),
        ["style.font_size"]
    );
}

#[test]
fn a_chip_row_that_never_wrapped() {
    assert_eq!(
        paths(r#"let main = fn () { Flex { style: { wrap: true } } };"#),
        ["style.wrap"]
    );
    assert_eq!(
        paths(r#"let main = fn () { Flex { style: { wrap: "wrap" } } };"#),
        ["style.wrap"]
    );
}

/// The subtle one. Not a key — a *value*, in a closed set the parser
/// returns `None` from, which the builder then reads as `start`. It has
/// shipped in three repositories and looks like working layout in all of
/// them, because the children carry `width: "grow"`.
#[test]
fn stretch_is_not_an_alignment_and_never_was() {
    let found = scan_source(
        "fixture.ogh",
        r#"let main = fn () {
             Flex { style: { cross_alignment: "stretch", width: "grow" } }
           };"#,
    );
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].path, "style.cross_alignment");
    assert_eq!(found[0].kind, Kind::Value);
    assert_eq!(found[0].found, "stretch");
    // Named where it is, not merely counted.
    assert_eq!(
        found[0].site.to_string(),
        "fixture.ogh:2:30",
        "{}",
        found[0]
    );
}

// ---------------------------------------------------------------------
// What must NOT be reported. A lint that cries wolf gets turned off.
// ---------------------------------------------------------------------

#[test]
fn the_whole_vocabulary_passes_clean() {
    let source = r#"
      let main = fn () {
        Flex {
          key: "root",
          block_interactions: false,
          mouse_down: fn () { 1 },
          mouse_up: fn () { 1 },
          mouse_enter: fn () { 1 },
          mouse_leave: fn () { 1 },
          contextmenu: fn () { 1 },
          drag_payload: "x",
          drag_dead_zone: 4,
          style: {
            direction: "column_reverse",
            main_alignment: "space_between",
            cross_alignment: "end",
            width: "grow",
            height: { grow: 2 },
            gap: 8,
            flex_wrap: true,
            padding: { top: 1, right: 2, bottom: 3, left: 4 },
            margin: 6,
            background_color: { r: 1, g: 2, b: 3, a: 4 },
            border: { top: { width: 1, style: "dashed", color: { r: 1, g: 1, b: 1, a: 1 } } },
            corner_radius: { top_left: 1, top_right: 2, bottom_left: 3, bottom_right: 4 },
            corner_chamfer: 2,
            inner_glow: { color: { r: 1, g: 1, b: 1, a: 1 }, blur: 2, spread: 1 },
            shadow: { color: { r: 0, g: 0, b: 0, a: 90 }, blur: 4, offset_x: 0, offset_y: 2 },
            background_image: "bg.png",
            position: { type: "absolute", x: 4, y: 5 },
            overflow: "scroll",
            cursor: "pointer",
            scroll_follow_end: true,
            opacity: 0.5,
            transform: { translate_x: 1, translate_y: 2, scale: 1, rotate: 4 },
            backdrop_filter: { blur: 12 },
            transition: { opacity: { stiffness: 200, damping: 30, delay: 0.1 }, border: "spring" },
            stagger: { step: 0.04, exit_step: 0.02, exit_order: "forward" },
          },
          hover_style: { background_color: { r: 9, g: 9, b: 9, a: 9 } },
          initial: { opacity: 0 },
          exit: { opacity: 0 },
          children: [
            Text {
              text: "hi",
              style: {
                size: 14, color: { r: 1, g: 1, b: 1, a: 1 }, align: "center",
                weight: "semi_bold", decoration: "underline", width: "shrink",
                font: "Serif", outline: { color: { r: 0, g: 0, b: 0, a: 255 }, width: 2 },
                letter_spacing: 0.5,
              },
              hover_style: { color: { r: 2, g: 2, b: 2, a: 2 } },
            },
            TextInput {
              value: "",
              on_change: fn (t) { t },
              on_submit: fn (t) { t },
              style: { padding: 4, size: 12 },
              focus_style: { border: 2 },
            },
            Grid {
              style: { columns: 2, rows: 2, cell_width: 10, cell_height: 10, cell_color: { r: 1, g: 1, b: 1, a: 1 } },
              children: [ Flex { grid_col: 1, grid_row: 0, grid_col_span: 2, grid_row_span: 1 } ],
            },
            Presence { key: "k", mode: "wait", style: { width: "grow" }, children: [] },
            Portal {
              layer: "tooltip", open: true, cursor: "free",
              anchor: "cursor", anchor_policy: "flip", anchor_offset: { x: 0, y: 22 },
              children: [],
            },
          ],
        }
      };
    "#;
    assert_eq!(paths(source), Vec::<String>::new());
}

#[test]
fn a_host_widget_is_checked_against_nothing() {
    // A host registers whatever widget it likes and reads whatever
    // properties it likes off it. Flagging those would be the false
    // positive that gets strictness switched off.
    assert!(paths(r#"let main = fn () { Dial { needle: 3, glow: "on" } };"#).is_empty());
}

// ---------------------------------------------------------------------
// The builder feeder: what the source scan cannot see.
// ---------------------------------------------------------------------

/// The idiomatic shape in every consuming repo: a style map bound to a
/// `let` (a map literal after `=>` is a *block*, so match arms have to),
/// then handed to a widget. The source scan sees an identifier and says
/// nothing, which is right. The builder sees the map.
const INDIRECT: &str = r#"
  let chip = { wrap: true, cross_alignment: "stretch" };
  let main = fn () { Flex { style: chip } };
"#;

#[test]
fn a_let_bound_style_map_is_invisible_to_the_source_scan() {
    assert!(scan_source("fixture.ogh", INDIRECT).is_empty());
}

#[test]
fn a_let_bound_style_map_is_not_invisible_to_the_builder() {
    let config = RuntimeConfig::new().with_strict_vocabulary();
    let ogham = Ogham::from_source(INDIRECT, config).expect("from_source");

    let mut found: Vec<String> = ogham
        .vocabulary_violations()
        .into_iter()
        .map(|v| format!("{}={}", v.path, v.found))
        .collect();
    found.sort();
    assert_eq!(found, ["style.cross_alignment=stretch", "style.wrap=wrap"]);
}

#[test]
fn strictness_is_off_by_default() {
    let ogham = Ogham::from_source(INDIRECT, RuntimeConfig::new()).expect("from_source");
    assert!(ogham.vocabulary_violations().is_empty());
}

/// Diagnostics, not semantics. The same document builds the same tree
/// either way — the whole promise that lets four repositories adopt this
/// on their own schedule.
#[test]
fn strictness_changes_nothing_about_the_tree() {
    const SRC: &str = r#"
      let main = fn () {
        Flex {
          style: { width: "grow", cross_alignment: "stretch", wrap: true },
          children: [ Text { text: "x", style: { font_size: 30 } } ],
        }
      };"#;

    let loose = Ogham::from_source(SRC, RuntimeConfig::new()).expect("loose");
    let strict = Ogham::from_source(SRC, RuntimeConfig::new().with_strict_vocabulary())
        .expect("strict");

    let describe = |o: &Ogham| {
        let root = o.get_ui().root.clone();
        let g = root.lock().expect("widget lock poisoned");
        let flex = g
            .downcast_ref::<ogham::widget::flex_widget::FlexWidget>()
            .expect("root is a Flex");
        format!("{:?} {:?}", flex.style, flex.get_children().len())
    };
    assert_eq!(describe(&loose), describe(&strict));
    assert!(!strict.vocabulary_violations().is_empty());
}

/// A widget rebuilt every frame is one mistake, not one per frame — and
/// a list that grows without bound is a leak wearing a diagnostic's coat.
#[test]
fn one_mistake_is_reported_once() {
    let config = RuntimeConfig::new()
        .with_strict_vocabulary()
        .with_host_state(HashMap::from([("n".to_string(), Value::Integer(0))]));
    let mut ogham = Ogham::from_source(
        r#"host_state { n: int };
           let main = fn () { Flex { style: { wrap: true }, children: [ Text { text: n } ] } };"#,
        config,
    )
    .expect("from_source");

    for i in 0..8 {
        ogham.with_runtime_mut(|rt| rt.inject_host_state("n".to_string(), Value::Integer(i)));
        ogham.frame(100.0, 100.0, 0.016).expect("frame");
    }
    assert_eq!(ogham.vocabulary_violations().len(), 1);
}

// ---------------------------------------------------------------------
// This repository's own fixtures.
// ---------------------------------------------------------------------

/// Every `.ogh` this crate ships. A language whose own examples miss its
/// own vocabulary has no standing to ask a game to fix its UI.
///
/// `tests/documents/` is exempt and is the exception that proves the rule:
/// those files are another repo's shipped document, checked in **verbatim**
/// as the fixture for `APPLICATION.md` §4.6, and editing them would delete
/// the property the test that reads them exists to hold. They are held to
/// the vocabulary where they live, which is regency — where this scan found
/// two live `cross_alignment: "baseline"`, a value the builder has never
/// had and silently reads as `start`.
#[test]
fn the_shipped_ogh_files_use_the_language_they_document() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    collect_ogh(&root.join("examples"), &mut files);
    collect_ogh(&root.join("tests"), &mut files);
    files.retain(|path| !path.starts_with(root.join("tests").join("documents")));
    assert!(!files.is_empty(), "no .ogh fixtures found");

    let mut report = String::new();
    for path in &files {
        let source = std::fs::read_to_string(path).expect("read fixture");
        for violation in scan_source(&path.display().to_string(), &source) {
            report.push_str(&format!("\n  {violation}"));
        }
    }
    assert!(report.is_empty(), "{report}");
}

fn collect_ogh(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_ogh(&path, out);
        } else if path.extension().is_some_and(|e| e == "ogh") {
            out.push(path);
        }
    }
}
