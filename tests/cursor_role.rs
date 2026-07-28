//! `UI::hovered_cursor` / `UI::hovered_blocks` — the CSS-`cursor` and
//! click-consumption queries hosts read (off the hover chain the UI already
//! tracks) to drive their own pointer glyph. Cursor affordance
//! (`cursor: "pointer"`) and click-consumption (a `mouse_down` listener)
//! are deliberately separate; these tests pin that the resolved cursor is
//! leaf-wins and that empty / non-declaring regions stay `Default`.

use ogham::runtime::config::RuntimeConfig;
use ogham::widget::event::Event;
use ogham::widget::point::Point;
use ogham::widget::style::CursorRole;
use ogham::Ogham;

const DT: f32 = 1.0 / 60.0;
const W: f32 = 800.0;
const H: f32 = 600.0;

/// A `pointer` button (which also consumes presses) and a plain panel,
/// absolutely placed so the test knows exactly where each sits. The button
/// wraps a `Text` child that declares no cursor of its own — so hovering the
/// child must still resolve to the button's `Pointer` (leaf-wins falls back
/// up to the nearest declaring ancestor).
///
/// The root and the plain panel declare `block_interactions: false`: a Flex
/// blocks presses by default, and this test wants the "transparent chrome,
/// world shows through" case (the in-game overlay's contract) so
/// `hovered_blocks` reports what a click would actually reach.
const SRC: &str = r##"
    let main = fn () {
      Flex {
        block_interactions: false,
        style: { width: "grow", height: "grow" },
        children: [
          Flex {
            style: {
              position: { type: "absolute", x: 100, y: 100 },
              width: 200, height: 50,
              cursor: "pointer",
            },
            mouse_down: fn () {},
            children: [ Text { text: "Enter", style: { size: 16 } } ],
          },
          Flex {
            block_interactions: false,
            style: {
              position: { type: "absolute", x: 400, y: 300 },
              width: 200, height: 50,
            },
            children: [],
          },
        ],
      }
    };
"##;

fn settled() -> Ogham {
    let mut o = Ogham::from_source(SRC, RuntimeConfig::default()).expect("from_source");
    // A couple of frames to lay out (and settle any entry animation).
    for _ in 0..8 {
        o.frame(W, H, DT).expect("frame");
    }
    o
}

/// Route a pointer move so the UI re-tags its hover chain, then read it back.
fn hover(o: &mut Ogham, x: f32, y: f32) -> (CursorRole, bool) {
    let ui = o.get_ui_mut();
    ui.call_event(&Event::with_point("mouse_move".to_string(), Point::new(x, y)));
    (ui.hovered_cursor(), ui.hovered_blocks())
}

#[test]
fn pointer_button_declares_pointer_and_blocks() {
    let mut o = settled();
    // Dead center of the 200×50 button at (100,100).
    assert_eq!(hover(&mut o, 200.0, 125.0), (CursorRole::Pointer, true));
}

#[test]
fn text_child_inherits_the_buttons_pointer() {
    let mut o = settled();
    // Over the label text, which declares no cursor of its own: leaf-wins
    // walks back up to the button's declared `pointer`.
    assert_eq!(hover(&mut o, 130.0, 125.0).0, CursorRole::Pointer);
}

#[test]
fn plain_panel_stays_default_and_transparent() {
    let mut o = settled();
    // Inside the non-declaring, non-consuming panel at (400,300).
    assert_eq!(hover(&mut o, 500.0, 325.0), (CursorRole::Default, false));
}

#[test]
fn empty_ground_is_default() {
    let mut o = settled();
    // Root background, over nothing interactive.
    assert_eq!(hover(&mut o, 10.0, 10.0), (CursorRole::Default, false));
}
