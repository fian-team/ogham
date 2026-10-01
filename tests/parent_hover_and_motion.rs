//! Five pieces of motion a tabbed rail needs and a document could not say:
//!
//! - a Text's colour springs (`transition: { color }`), on a new value
//!   from the document and on entering or leaving its `hover_style`;
//! - `hover_with_parent: true` — a widget hovered exactly while its
//!   parent is, so an icon reacts to its whole row being under the
//!   pointer rather than only its own glyph;
//! - a translation written as a percentage of the widget's own size
//!   (`translate_x: "-100%"`), resolved against the layout at paint;
//! - `Portal { open: "hover" }` — a tooltip that opens while its parent
//!   is hovered, with no state in the document, and replays its entry
//!   every time it opens;
//! - a hover wash over a box with no resting fill fades in and out on
//!   its declared `background_color` transition, where it used to snap.

use std::collections::HashMap;

use ogham::runtime::config::RuntimeConfig;
use ogham::runtime::value::Value;
use ogham::widget::event::Event;
use ogham::widget::flex_widget::FlexWidget;
use ogham::widget::point::Point;
use ogham::widget::portal_widget::PortalWidget;
use ogham::widget::style::Color;
use ogham::widget::text_widget::TextWidget;
use ogham::widget::vocabulary::scan_source;
use ogham::widget::{Widget, WidgetRef};
use ogham::Ogham;

const W: f32 = 800.0;
const H: f32 = 600.0;
const DT: f32 = 1.0 / 60.0;

fn find<T: Widget>(node: &WidgetRef) -> Option<WidgetRef> {
    let children = {
        let g = node.lock().unwrap();
        if g.downcast_ref::<T>().is_some() {
            return Some(node.clone());
        }
        g.get_children()
    };
    children.iter().find_map(find::<T>)
}

fn find_keyed(node: &WidgetRef, key: &str) -> Option<WidgetRef> {
    let children = {
        let g = node.lock().unwrap();
        if g.key() == Some(key) {
            return Some(node.clone());
        }
        g.get_children()
    };
    children.iter().find_map(|c| find_keyed(c, key))
}

fn mounted(src: &str, state: &[(&str, Value)]) -> Ogham {
    let config = RuntimeConfig::new().with_host_state(
        state
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect::<HashMap<_, _>>(),
    );
    let mut o = Ogham::from_source(src, config).expect("the document mounts");
    frames(&mut o, 3);
    o
}

fn frames(o: &mut Ogham, n: usize) {
    for _ in 0..n {
        o.frame(W, H, DT).expect("frame");
    }
}

fn hover(o: &mut Ogham, x: f32, y: f32) {
    o.get_ui_mut()
        .call_event(&Event::with_point("mouse_move".to_string(), Point::new(x, y)));
}

fn text_color(o: &Ogham) -> Color {
    let text = find::<TextWidget>(&o.get_ui().root).expect("a Text in the tree");
    let g = text.lock().unwrap();
    g.downcast_ref::<TextWidget>().unwrap().shown_color()
}

fn text_hovered(o: &Ogham) -> bool {
    let text = find::<TextWidget>(&o.get_ui().root).expect("a Text in the tree");
    let g = text.lock().unwrap();
    g.is_hovered()
}

const BLACK: Color = Color { r: 0, g: 0, b: 0, a: 255 };
const WHITE: Color = Color { r: 255, g: 255, b: 255, a: 255 };

// ── a Text's colour ─────────────────────────────────────────────────

const CHOSEN_TEXT: &str = r#"
let white = { r: 255, g: 255, b: 255, a: 255 };
let black = { r: 0, g: 0, b: 0, a: 255 };
let main = fn () {
  Text { text: "Inventory", style: {
    color: match chosen { true => white, false => black },
    transition: { color: { stiffness: 170, damping: 26 } },
  } }
};
"#;

#[test]
fn a_texts_colour_travels_to_a_new_value_rather_than_snapping() {
    let mut o = mounted(CHOSEN_TEXT, &[("chosen", Value::Boolean(false))]);
    assert_eq!(text_color(&o), BLACK);

    o.with_runtime_mut(|rt| rt.set_host_state("chosen", true));
    frames(&mut o, 1);
    let mid = text_color(&o);
    assert!(
        mid.r > 0 && mid.r < 255,
        "one frame in, the colour is between the two, got {mid:?}"
    );

    frames(&mut o, 120);
    assert_eq!(text_color(&o), WHITE, "and it arrives");
}

#[test]
fn a_text_without_a_transition_still_snaps() {
    let src = CHOSEN_TEXT.replace("transition: { color: { stiffness: 170, damping: 26 } },", "");
    let mut o = mounted(&src, &[("chosen", Value::Boolean(false))]);
    o.with_runtime_mut(|rt| rt.set_host_state("chosen", true));
    frames(&mut o, 1);
    assert_eq!(text_color(&o), WHITE);
}

// ── following the parent's hover ────────────────────────────────────

/// A 200 × 60 row with a 20 × 20 glyph in its top-left corner. The row
/// hit-tests for itself; the glyph either does too or follows the row.
fn row(follow: bool) -> String {
    format!(
        r#"
let main = fn () {{
  Flex {{
    style: {{ width: "grow", height: "grow", direction: "column" }},
    children: [
      Flex {{
        style: {{ width: 200, height: 60 }},
        children: [
          Text {{
            text: "x",
            hover_with_parent: {follow},
            style: {{ width: 20, height: 20, color: {{ r: 0, g: 0, b: 0, a: 255 }},
                      transition: "spring" }},
            hover_style: {{ color: {{ r: 255, g: 255, b: 255, a: 255 }} }},
          }},
        ],
      }},
    ],
  }}
}};
"#
    )
}

#[test]
fn a_widget_that_follows_its_parent_is_hovered_anywhere_in_the_parent() {
    let mut o = mounted(&row(true), &[]);
    hover(&mut o, 150.0, 40.0);
    assert!(text_hovered(&o), "the pointer is in the row, well clear of the glyph");

    frames(&mut o, 120);
    assert_eq!(text_color(&o), WHITE, "and its hover_style springs in");

    hover(&mut o, 150.0, 300.0);
    assert!(!text_hovered(&o), "out of the row, out of hover");
}

#[test]
fn without_the_flag_a_widget_hovers_only_under_the_pointer() {
    let mut o = mounted(&row(false), &[]);
    hover(&mut o, 150.0, 40.0);
    assert!(!text_hovered(&o), "the glyph is not under the pointer");
    hover(&mut o, 10.0, 10.0);
    assert!(text_hovered(&o));
}

// ── a fill that is not there at rest ────────────────────────────────

const WASH: &str = r#"
let main = fn () {
  Flex {
    style: { width: "grow", height: "grow", direction: "column" },
    children: [
      Flex { key: "row",
        style: { width: 200, height: 60,
                 transition: { background_color: { stiffness: 170, damping: 26 } } },
        hover_style: { background_color: { r: 67, g: 114, b: 42, a: 40 } } },
    ],
  }
};
"#;

fn row_fill(o: &Ogham) -> Option<Color> {
    let row = find_keyed(&o.get_ui().root, "row").expect("the row");
    let g = row.lock().unwrap();
    g.downcast_ref::<FlexWidget>().unwrap().style.background_color
}

#[test]
fn a_wash_over_no_fill_fades_in_and_out() {
    let mut o = mounted(WASH, &[]);
    assert_eq!(row_fill(&o), None);

    hover(&mut o, 100.0, 30.0);
    frames(&mut o, 1);
    let rising = row_fill(&o).expect("a fill in flight").a;
    assert!(rising > 0 && rising < 40, "fading in, got alpha {rising}");
    frames(&mut o, 120);
    assert_eq!(row_fill(&o).map(|c| c.a), Some(40));

    hover(&mut o, 400.0, 400.0);
    frames(&mut o, 1);
    let falling = row_fill(&o).expect("still fading").a;
    assert!(falling > 0 && falling < 40, "fading out, got alpha {falling}");
    frames(&mut o, 120);
    assert_eq!(row_fill(&o), None, "and gone once it settles");
}

// ── a percentage of the widget's own size ───────────────────────────

#[test]
fn a_percentage_translation_is_a_share_of_the_widgets_own_size() {
    let o = mounted(
        r#"
let main = fn () {
  Flex {
    style: { width: "grow", height: "grow" },
    children: [
      Flex { key: "panel", style: {
        width: 320, height: 200,
        transform: { translate_x: "-100%", translate_y: "25%" },
      } },
    ],
  }
};
"#,
        &[],
    );
    let panel = find_keyed(&o.get_ui().root, "panel").expect("the panel");
    let g = panel.lock().unwrap();
    let fx = g.render_effects().expect("a transformed widget has effects");
    assert_eq!(fx.transform.translate_x, -320.0);
    assert_eq!(fx.transform.translate_y, 50.0);
    assert_eq!(fx.transform.translate_x_percent, 0.0, "paint sees pixels only");
}

#[test]
fn a_percentage_slides_in_from_its_initial() {
    let o = mounted(
        r#"
let main = fn () {
  Flex {
    style: { width: "grow", height: "grow" },
    children: [
      Flex { key: "panel",
        initial: { transform: { translate_x: "-100%" } },
        style: { width: 400, height: 200,
                 transition: { transform: { stiffness: 170, damping: 26 } } } },
    ],
  }
};
"#,
        &[],
    );
    let panel = find_keyed(&o.get_ui().root, "panel").expect("the panel");
    let g = panel.lock().unwrap();
    let tx = g.render_effects().map_or(0.0, |fx| fx.transform.translate_x);
    assert!(
        tx < -100.0 && tx > -400.0,
        "three frames in, the panel is still coming in from its own width, got {tx}"
    );
}

// ── a portal that opens on hover ────────────────────────────────────

const TOOLTIP: &str = r#"
let main = fn () {
  Flex {
    style: { width: "grow", height: "grow", direction: "column" },
    children: [
      Flex {
        style: { width: 60, height: 60 },
        children: [
          Portal {
            open: "hover",
            layer: "tooltip",
            anchor: "parent",
            children: [
              Flex { key: "tip",
                initial: { opacity: 0 },
                style: { width: 120, height: 30,
                         transition: { opacity: { stiffness: 170, damping: 26, delay: 0.2 } } } },
            ],
          },
        ],
      },
    ],
  }
};
"#;

fn portal_open(o: &Ogham) -> bool {
    let p = find::<PortalWidget>(&o.get_ui().root).expect("the portal");
    let g = p.lock().unwrap();
    g.downcast_ref::<PortalWidget>().unwrap().is_open()
}

fn tip_opacity(o: &Ogham) -> Option<f32> {
    let tip = find_keyed(&o.get_ui().root, "tip")?;
    let g = tip.lock().unwrap();
    Some(g.downcast_ref::<FlexWidget>().unwrap().style.opacity.value())
}

#[test]
fn a_hover_portal_opens_while_its_parent_is_hovered() {
    let mut o = mounted(TOOLTIP, &[]);
    assert!(!portal_open(&o), "closed until something hovers the parent");
    assert_eq!(tip_opacity(&o), None, "and its content is out of the tree");

    hover(&mut o, 30.0, 30.0);
    frames(&mut o, 1);
    assert!(portal_open(&o));
    let early = tip_opacity(&o).expect("the content mounted");
    assert!(early < 0.05, "the delayed fade is the show delay, got {early}");

    frames(&mut o, 90);
    assert!(tip_opacity(&o).unwrap() > 0.99);

    hover(&mut o, 400.0, 400.0);
    frames(&mut o, 1);
    assert!(!portal_open(&o), "leaving the parent closes it at once");
    assert_eq!(tip_opacity(&o), None);
}

#[test]
fn a_hover_portal_replays_its_entry_every_time_it_opens() {
    let mut o = mounted(TOOLTIP, &[]);
    hover(&mut o, 30.0, 30.0);
    frames(&mut o, 90);
    hover(&mut o, 400.0, 400.0);
    frames(&mut o, 1);

    hover(&mut o, 30.0, 30.0);
    frames(&mut o, 1);
    let again = tip_opacity(&o).expect("mounted again");
    assert!(again < 0.05, "a second hover waits out the delay again, got {again}");
}

// ── the vocabulary knows all of it ──────────────────────────────────

#[test]
fn the_vocabulary_knows_the_new_keys() {
    let found = scan_source(
        "motion.ogh",
        r#"
let main = fn () {
  Flex {
    hover_with_parent: true,
    children: [
      Text { text: "x", hover_with_parent: true,
             style: { transition: { color: "spring" } } },
      Text { text: "y", style: { transition: "spring" } },
    ],
  }
};
"#,
    );
    assert!(found.is_empty(), "{found:#?}");
}

#[test]
fn a_text_transition_names_only_colour() {
    let found = scan_source(
        "motion.ogh",
        r#"let main = fn () { Text { text: "x", style: { transition: { size: "spring" } } } };"#,
    );
    assert_eq!(found.len(), 1, "{found:#?}");
    assert_eq!(found[0].path, "style.transition.size");
}
