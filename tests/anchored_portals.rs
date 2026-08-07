//! Anchored portals — a Portal whose viewport origin comes from a host-set
//! point rather than from the slot it was declared in.
//!
//! Three layers of test, matching where the feature actually lives:
//!
//! - `resolve_anchor` is a pure function of five numbers, so the three
//!   seating policies are driven directly. Edge arithmetic is where this
//!   feature is most likely to be quietly wrong, and none of it needs a
//!   window.
//! - The builder tests pin the `.ogh` surface, including the rejections —
//!   an unknown `anchor_policy` must be a *loud* error, not a silent
//!   fallback to the default.
//! - The end-to-end tests drive a real `SkiaEnv` over an offscreen raster
//!   surface, because the override lives in the Skia Pass-A walk and the
//!   whole claim of the design is that everything downstream
//!   (paint, hit-test, occlusion) follows from the resolved
//!   `PortalEntry::viewport_rect` with no further changes. Asserting that
//!   claim means actually running the walk.

use std::sync::{Arc, Mutex};

use ogham::runtime::config::RuntimeConfig;
use ogham::runtime::value::Value;
use ogham::runtime::Runtime;
use ogham::skia::SkiaEnv;
use ogham::widget::event::Event;
use ogham::widget::flex_widget::FlexWidget;
use ogham::widget::point::Point;
use ogham::widget::portal_layer::{
    resolve_anchor, AnchorPolicy, PortalLayer, ANCHOR_VIEWPORT_INSET,
};
use ogham::widget::rect::Rect;
use ogham::widget::{builder, PortalEntry, Surface, WidgetRef, DRAG_PREVIEW_ANCHOR, UI};
use ogham::Ogham;

const DT: f32 = 1.0 / 60.0;
const W: f32 = 800.0;
const H: f32 = 600.0;

// ── helpers ────────────────────────────────────────────────────────────

// `BridgeError` has no `Debug` for its `WidgetRef`-bearing `VMError`
// variant; flatten the kinds these tests reach (same shape as
// tests/portal.rs and tests/canvas_widget.rs).
fn describe(err: &builder::BridgeError) -> String {
    match err {
        builder::BridgeError::InvalidWidgetType(s) => format!("InvalidWidgetType({})", s),
        builder::BridgeError::MissingProperty(s) => format!("MissingProperty({})", s),
        builder::BridgeError::InvalidPropertyType(n, m) => {
            format!("InvalidPropertyType({}, {})", n, m)
        }
        builder::BridgeError::VMError(_) => "VMError(...)".to_string(),
    }
}

fn try_build_root(src: &str) -> Result<WidgetRef, String> {
    let runtime = Arc::new(Mutex::new(Runtime::from_source(src, None).expect("parse")));
    let module = {
        let rt = runtime.lock().unwrap();
        rt.get_module().expect("module").clone()
    };
    let widget_value = {
        let mut rt = runtime.lock().unwrap();
        rt.execute_module(&module).expect("execute")
    };
    let registry = builder::WidgetRegistry::with_defaults();
    builder::widget_value_to_widget_ref(&registry, &runtime, &widget_value)
        .map_err(|e| describe(&e))
}

fn build_root(src: &str) -> WidgetRef {
    try_build_root(src).expect("build widget tree")
}

/// The bridge error a source is expected to produce, as a flat string.
fn build_error(src: &str) -> String {
    match try_build_root(src) {
        Ok(_) => panic!("expected the build to be rejected"),
        Err(err) => err,
    }
}

/// An `Ogham` run for enough frames to lay out and settle.
fn settled(src: &str) -> Ogham {
    let mut o = Ogham::from_source(src, RuntimeConfig::new()).expect("from_source");
    for _ in 0..4 {
        o.frame(W, H, DT).expect("frame");
    }
    o
}

/// An offscreen Skia environment — enough to run the Pass-A walk that
/// resolves anchors and populates `portal_layers`.
fn raster_env() -> SkiaEnv {
    let surface =
        ogham::skia_safe::surfaces::raster_n32_premul((256, 256)).expect("raster surface");
    SkiaEnv::new_with_dpi_scale(surface, 1.0)
}

/// Run the render walk so `portal_layers` reflects this frame.
fn draw(o: &mut Ogham) {
    let mut env = raster_env();
    env.draw(o.get_ui_mut());
}

/// The single portal entry the walk produced, or `None` if it produced
/// none. Anchoring is per-entry, so every test declares exactly one.
fn only_entry(o: &Ogham) -> Option<PortalEntry> {
    let layers = &o.get_ui().portal_layers;
    assert!(layers.len() <= 1, "tests declare a single portal");
    layers.iter_paint_order().next().cloned()
}

/// The resolved viewport origin of the tree's single portal entry.
fn entry_origin(o: &Ogham) -> (f32, f32) {
    let e = only_entry(o).expect("a portal entry was pushed");
    (e.viewport_rect.x, e.viewport_rect.y)
}

/// A tooltip-shaped source: a `card_w` × `card_h` card inside an anchored
/// Portal, with `extra` spliced into the Portal's property list.
fn anchored_src(extra: &str, card_w: f32, card_h: f32) -> String {
    format!(
        r#"
let main = fn () {{
  Flex {{
    style: {{ width: "grow", height: "grow" }},
    children: [
      Portal {{
        open: true,
        layer: "tooltip",
        anchor: "probe",
        {extra}
        children: [
          Flex {{
            style: {{ width: {card_w}, height: {card_h} }},
            mouse_down: fn () {{ event("card_press"); }},
            children: [],
          }}
        ],
      }}
    ],
  }}
}};
"#
    )
}

// ── M2 — resolve_anchor, the three policies ────────────────────────────

/// `raw` is the escape hatch: the point plus the offset, and nothing else.
/// A drag preview needs this — the pointer can be at the edge of the
/// window, and a preview yanked back inside would lag the cursor.
#[test]
fn raw_passes_the_point_straight_through() {
    let at = resolve_anchor(
        (700.0, 580.0),
        (0.0, 0.0),
        AnchorPolicy::Raw,
        (200.0, 60.0),
        (W, H),
    );
    assert_eq!(at, (700.0, 580.0), "raw must not clamp");

    let nudged = resolve_anchor(
        (700.0, 580.0),
        (14.0, 22.0),
        AnchorPolicy::Raw,
        (200.0, 60.0),
        (W, H),
    );
    assert_eq!(nudged, (714.0, 602.0), "raw still honours the offset");
}

/// A box that already fits is left exactly where the host put it. The
/// clamp is a correction, not a re-layout.
#[test]
fn clamp_leaves_a_box_that_already_fits_alone() {
    let at = resolve_anchor(
        (300.0, 200.0),
        (0.0, 0.0),
        AnchorPolicy::Clamp,
        (200.0, 60.0),
        (W, H),
    );
    assert_eq!(at, (300.0, 200.0));
}

/// The 8 px inset holds on all four edges. This is the hand-rolled
/// `x.min(w - card_w - 8.0).max(8.0)` the feature exists to delete, so it
/// gets asserted edge by edge rather than in aggregate.
#[test]
fn clamp_holds_the_inset_on_all_four_edges() {
    let size = (200.0, 60.0);
    let inset = ANCHOR_VIEWPORT_INSET;

    let left = resolve_anchor(
        (-50.0, 300.0),
        (0.0, 0.0),
        AnchorPolicy::Clamp,
        size,
        (W, H),
    );
    assert_eq!(left.0, inset, "clamped off the left edge");

    let right = resolve_anchor(
        (790.0, 300.0),
        (0.0, 0.0),
        AnchorPolicy::Clamp,
        size,
        (W, H),
    );
    assert_eq!(right.0, W - size.0 - inset, "clamped off the right edge");

    let top = resolve_anchor(
        (300.0, -20.0),
        (0.0, 0.0),
        AnchorPolicy::Clamp,
        size,
        (W, H),
    );
    assert_eq!(top.1, inset, "clamped off the top edge");

    let bottom = resolve_anchor(
        (300.0, 595.0),
        (0.0, 0.0),
        AnchorPolicy::Clamp,
        size,
        (W, H),
    );
    assert_eq!(bottom.1, H - size.1 - inset, "clamped off the bottom edge");
}

/// A box bigger than the viewport can't satisfy both insets. It gets
/// pinned to the top-left inset rather than pushed off the top-left —
/// the `min` runs before the `max` deliberately.
#[test]
fn clamp_pins_an_oversized_box_to_the_leading_inset() {
    let at = resolve_anchor(
        (400.0, 300.0),
        (0.0, 0.0),
        AnchorPolicy::Clamp,
        (2000.0, 1500.0),
        (W, H),
    );
    assert_eq!(at, (ANCHOR_VIEWPORT_INSET, ANCHOR_VIEWPORT_INSET));
}

/// The offset lands before the policy, so an author writes "below-right of
/// the pointer" and the clamp still keeps the result on screen.
#[test]
fn the_offset_is_applied_before_the_policy() {
    // 790 + 14 = 804, off the right edge; clamped back to the inset.
    let at = resolve_anchor(
        (790.0, 100.0),
        (14.0, 22.0),
        AnchorPolicy::Clamp,
        (200.0, 60.0),
        (W, H),
    );
    assert_eq!(at.0, W - 200.0 - ANCHOR_VIEWPORT_INSET);
    assert_eq!(at.1, 122.0, "the y offset survives a clamp it doesn't hit");
}

/// `flip` behaves exactly like `clamp` until the box would overrun the
/// bottom. Near the top of the window there is nothing to flip.
#[test]
fn flip_stays_below_the_anchor_when_there_is_room() {
    let at = resolve_anchor(
        (300.0, 100.0),
        (14.0, 22.0),
        AnchorPolicy::Flip,
        (200.0, 60.0),
        (W, H),
    );
    assert_eq!(at, (314.0, 122.0), "no overrun, no flip");
}

/// The tooltip rule: when the card would be clipped by the bottom of the
/// window it goes *above* the anchor, clearing it by the same offset it
/// would have used below.
#[test]
fn flip_goes_above_only_when_the_bottom_would_overrun() {
    let size = (200.0, 60.0);
    // 560 + 22 + 60 = 642 > 600 - 8. Flip.
    let flipped = resolve_anchor(
        (300.0, 560.0),
        (14.0, 22.0),
        AnchorPolicy::Flip,
        size,
        (W, H),
    );
    assert_eq!(
        flipped,
        (314.0, 560.0 - 22.0 - 60.0),
        "flipped above the anchor, mirroring the offset"
    );

    // One pixel of headroom either side of the threshold, so the
    // condition can't drift into an off-by-one without a failure.
    let just_fits = 600.0 - 8.0 - 60.0 - 22.0;
    let below = resolve_anchor(
        (300.0, just_fits),
        (0.0, 22.0),
        AnchorPolicy::Flip,
        size,
        (W, H),
    );
    assert_eq!(below.1, just_fits + 22.0, "exactly fits — stays below");
    let over = resolve_anchor(
        (300.0, just_fits + 1.0),
        (0.0, 22.0),
        AnchorPolicy::Flip,
        size,
        (W, H),
    );
    assert_eq!(over.1, just_fits + 1.0 - 22.0 - 60.0, "one px over — flips");
}

/// Flipping is a *vertical* rule. The horizontal clamp is unconditional,
/// so a card anchored at the bottom-right corner both flips up and pulls
/// left.
#[test]
fn flip_still_clamps_horizontally() {
    let size = (200.0, 60.0);
    let at = resolve_anchor(
        (795.0, 590.0),
        (14.0, 22.0),
        AnchorPolicy::Flip,
        size,
        (W, H),
    );
    assert_eq!(at.0, W - size.0 - ANCHOR_VIEWPORT_INSET, "pulled left");
    assert!(at.1 < 590.0, "flipped above the anchor");
}

/// A flipped box that still can't fit gets clamped rather than parked off
/// the top of the screen.
#[test]
fn flip_clamps_the_flipped_result_into_a_short_viewport() {
    let at = resolve_anchor(
        (100.0, 90.0),
        (0.0, 22.0),
        AnchorPolicy::Flip,
        (100.0, 80.0),
        (200.0, 120.0),
    );
    assert!(
        at.1 >= ANCHOR_VIEWPORT_INSET,
        "flipped above but clamped back below the top inset, got {}",
        at.1
    );
}

/// Before the first layout pass there is no viewport. Clamping against
/// `(0, 0)` would park every anchored portal at the inset corner, which
/// reads as a positioning bug; leaving the point alone reads as "not laid
/// out yet", which is what it is.
#[test]
fn a_zero_viewport_disables_clamping() {
    for policy in AnchorPolicy::ALL {
        let at = resolve_anchor(
            (300.0, 200.0),
            (0.0, 0.0),
            policy,
            (200.0, 60.0),
            (0.0, 0.0),
        );
        assert_eq!(at, (300.0, 200.0), "{:?} with no viewport", policy);
    }
}

// ── M0 — anchor storage on `UI` / `Ogham` ──────────────────────────────

#[test]
fn set_read_and_clear_an_anchor() {
    let mut ui = UI::new(Arc::new(Mutex::new(FlexWidget::new())));
    assert!(ui.anchor("hud").is_none(), "unset ids read as None");

    ui.set_anchor("hud", Point::new(12.0, 34.0));
    let p = ui.anchor("hud").expect("set");
    assert_eq!((p.x(), p.y()), (12.0, 34.0));

    ui.clear_anchor("hud");
    assert!(ui.anchor("hud").is_none());
    // Idempotent — clearing what isn't there is not an error.
    ui.clear_anchor("hud");
}

#[test]
fn clear_anchors_drops_every_id() {
    let mut ui = UI::new(Arc::new(Mutex::new(FlexWidget::new())));
    ui.set_anchor("a", Point::new(1.0, 1.0));
    ui.set_anchor("b", Point::new(2.0, 2.0));
    ui.clear_anchors();
    assert!(ui.anchor("a").is_none());
    assert!(ui.anchor("b").is_none());
}

/// Anchors are host state, not frame state: a rerender that rebuilds the
/// descriptor tree doesn't disturb them. This is the property that lets a
/// host set a nameplate's anchor only when its entity moves.
#[test]
fn anchors_survive_a_rerender() {
    let src = r#"
let main = fn () {
  Flex { style: { width: "grow", height: "grow" }, children: [] }
};
"#;
    let mut o = settled(src);
    o.set_anchor("nameplate", Point::new(240.0, 180.0));

    o.with_runtime_mut(|rt| rt.request_rerender());
    for _ in 0..4 {
        o.frame(W, H, DT).expect("frame");
    }

    let p = o.anchor("nameplate").expect("anchor survives rerenders");
    assert_eq!((p.x(), p.y()), (240.0, 180.0));
}

/// A hot reload drops anchors (INTENT §7: a reload drops what it cannot
/// verify still means anything — an anchor id names a Portal in the *old*
/// program). Hosts that set an anchor once must re-set it after a reload;
/// hosts that set per frame never notice.
#[test]
fn a_hot_reload_drops_anchors() {
    let dir = std::env::temp_dir().join(format!("ogham_anchor_reload_{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("scratch dir");
    let path = dir.join("anchored.ogh");
    std::fs::write(
        &path,
        "let main = fn () { Flex { style: { width: \"grow\" }, children: [] } };\n",
    )
    .expect("write source");

    let mut o =
        Ogham::watch(path.to_str().unwrap().to_string(), RuntimeConfig::new()).expect("watch");
    o.frame(W, H, DT).expect("frame");
    o.set_anchor("nameplate", Point::new(10.0, 20.0));
    assert!(o.anchor("nameplate").is_some());

    o.reload().expect("reload");
    assert!(
        o.anchor("nameplate").is_none(),
        "a reload drops host-set anchors"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

// ── M1 — the `.ogh` surface ────────────────────────────────────────────

#[test]
fn anchor_properties_round_trip_from_source() {
    let src = anchored_src(
        r#"anchor_policy: "flip",
        anchor_offset: { x: 14, y: 22 },"#,
        200.0,
        60.0,
    );
    let root = build_root(&src);
    let g = root.lock().unwrap();
    let portal = g.get_children()[0].clone();
    let pg = portal.lock().unwrap();
    let info = pg.as_portal().expect("is portal");
    assert_eq!(info.anchor.as_deref(), Some("probe"));
    assert_eq!(info.anchor_policy, AnchorPolicy::Flip);
    assert_eq!(info.anchor_offset, (14.0, 22.0));
}

/// Absent `anchor`, a Portal is exactly what it was. The whole feature is
/// additive; this is the assertion that says so.
#[test]
fn a_portal_without_an_anchor_is_unchanged() {
    let src = r#"
let main = fn () {
  Portal { open: true, layer: "tooltip", children: [] }
};
"#;
    let root = build_root(src);
    let g = root.lock().unwrap();
    let info = g.as_portal().expect("is portal");
    assert!(info.anchor.is_none());
    assert_eq!(info.anchor_policy, AnchorPolicy::Clamp, "the default");
    assert_eq!(info.anchor_offset, (0.0, 0.0));
}

/// Either offset component may be omitted — `{ y: 22 }` is a reasonable
/// "below the pointer, not beside it".
#[test]
fn anchor_offset_components_are_individually_optional() {
    let src = anchored_src("anchor_offset: { y: 22 },", 100.0, 40.0);
    let root = build_root(&src);
    let g = root.lock().unwrap();
    let portal = g.get_children()[0].clone();
    let pg = portal.lock().unwrap();
    assert_eq!(pg.as_portal().unwrap().anchor_offset, (0.0, 22.0));
}

/// An unknown policy name is a build error listing the valid ones — NOT a
/// silent fall back to the default. `position: "relative"` parsing to a
/// no-op is the failure shape this codebase keeps repeating; anchoring
/// does not join it.
#[test]
fn an_unknown_anchor_policy_is_rejected_by_name() {
    let err = build_error(&anchored_src(r#"anchor_policy: "clamped","#, 100.0, 40.0));
    assert!(err.contains("anchor_policy"), "{}", err);
    assert!(
        err.contains("raw, clamp, flip"),
        "lists valid names: {}",
        err
    );
}

#[test]
fn a_non_string_anchor_is_rejected() {
    let err =
        build_error(&anchored_src("", 100.0, 40.0).replace(r#"anchor: "probe","#, "anchor: 7,"));
    assert!(err.contains("anchor"), "{}", err);
    assert!(err.contains("string"), "{}", err);
}

#[test]
fn a_non_map_anchor_offset_is_rejected() {
    let err = build_error(&anchored_src("anchor_offset: 14,", 100.0, 40.0));
    assert!(err.contains("anchor_offset"), "{}", err);
}

/// A focus trap that follows a host-set point can strand input over
/// chrome the user can't reach — a modal the pointer drags around. Reject
/// the combination at build time rather than ship the failure mode.
#[test]
fn an_anchored_portal_cannot_also_trap_focus() {
    let src = r#"
let main = fn () {
  Portal {
    open: true,
    anchor: "probe",
    focus_trap: true,
    children: [],
  }
};
"#;
    let err = build_error(src);
    assert!(err.contains("focus_trap"), "{}", err);
}

/// `__`-prefixed ids belong to the runtime (the drag preview lives at
/// `__drag_preview`). Without the reservation a userspace Portal could
/// silently attach itself to the drag cursor.
#[test]
fn reserved_anchor_ids_are_rejected() {
    let src = r#"
let main = fn () {
  Portal { open: true, anchor: "__drag_preview", children: [] }
};
"#;
    let err = build_error(src);
    assert!(err.contains("reserved"), "{}", err);
}

#[test]
fn an_empty_anchor_id_is_rejected() {
    let src = r#"
let main = fn () {
  Portal { open: true, anchor: "", children: [] }
};
"#;
    let err = build_error(src);
    assert!(err.contains("anchor"), "{}", err);
}

// ── M2 — resolution in the Pass-A walk ─────────────────────────────────

/// The core claim: a host-set point becomes the entry's viewport origin.
#[test]
fn an_anchored_portal_lands_at_the_host_point() {
    let mut o = settled(&anchored_src(r#"anchor_policy: "raw","#, 200.0, 60.0));
    o.set_anchor("probe", Point::new(310.0, 240.0));
    draw(&mut o);
    assert_eq!(entry_origin(&o), (310.0, 240.0));
}

/// The entry's size is the *children's* extent, not the Portal's own
/// laid-out rect. The Portal's inner flex is grow/grow, so its rect is
/// the whole available box — clamping against that would pin every
/// anchored portal to the inset corner.
#[test]
fn the_anchored_entry_is_sized_from_its_children_not_its_slot() {
    let mut o = settled(&anchored_src(r#"anchor_policy: "raw","#, 200.0, 60.0));
    o.set_anchor("probe", Point::new(100.0, 100.0));
    draw(&mut o);
    let e = only_entry(&o).expect("entry");
    assert_eq!(
        (e.viewport_rect.width, e.viewport_rect.height),
        (200.0, 60.0),
        "the card's size, not the viewport's"
    );
}

/// The measured size is the *union* of the children's rects, which is
/// the one real footgun: a full-viewport sibling (the `Modal`
/// composition pattern's backdrop) makes the measured box the viewport
/// and `clamp` then pins it to the corner. Legal, does what it says,
/// and not what anyone wants — so pin the behaviour rather than let it
/// surprise someone.
#[test]
fn the_extent_unions_every_child_which_is_why_anchored_portals_take_one() {
    let src = r#"
let main = fn () {
  Portal {
    open: true,
    layer: "tooltip",
    anchor: "probe",
    children: [
      Flex { style: { width: "grow", height: "grow" }, children: [] },
      Flex { style: { width: 200, height: 60 }, children: [] },
    ],
  }
};
"#;
    let mut o = settled(src);
    o.set_anchor("probe", Point::new(300.0, 200.0));
    draw(&mut o);
    let e = only_entry(&o).expect("entry");
    assert!(
        e.viewport_rect.width >= W,
        "the grow sibling dominates the union: {}",
        e.viewport_rect.width
    );
    assert_eq!(
        (e.viewport_rect.x, e.viewport_rect.y),
        (ANCHOR_VIEWPORT_INSET, ANCHOR_VIEWPORT_INSET),
        "a viewport-sized box can't fit, so clamp pins it to the inset corner"
    );
}

/// An id the host never set means "the thing I point at is gone": the
/// entry is not pushed and the portal paints nothing. Not an error.
#[test]
fn an_unset_anchor_renders_nothing() {
    let mut o = settled(&anchored_src("", 200.0, 60.0));
    draw(&mut o);
    assert!(
        only_entry(&o).is_none(),
        "no host anchor → no portal entry this frame"
    );

    // …and it comes back the moment the host sets the id.
    o.set_anchor("probe", Point::new(50.0, 50.0));
    draw(&mut o);
    assert!(only_entry(&o).is_some(), "set the anchor, get the portal");

    // …and goes away again when cleared.
    o.clear_anchor("probe");
    draw(&mut o);
    assert!(only_entry(&o).is_none(), "cleared → gone");
}

/// `clamp` against the real measured card, end to end. The size has to
/// come from the layout pass that already ran this frame for this to work
/// at all — that is the piece `.ogh` cannot reach.
#[test]
fn clamp_pulls_an_edge_hugging_card_back_inside() {
    let mut o = settled(&anchored_src("", 200.0, 60.0));
    o.set_anchor("probe", Point::new(795.0, 595.0));
    draw(&mut o);
    assert_eq!(
        entry_origin(&o),
        (
            W - 200.0 - ANCHOR_VIEWPORT_INSET,
            H - 60.0 - ANCHOR_VIEWPORT_INSET
        ),
        "clamp used the card's measured size"
    );
}

#[test]
fn flip_seats_the_card_above_a_low_anchor() {
    let mut o = settled(&anchored_src(
        r#"anchor_policy: "flip",
        anchor_offset: { x: 14, y: 22 },"#,
        200.0,
        60.0,
    ));
    o.set_anchor("probe", Point::new(300.0, 560.0));
    draw(&mut o);
    assert_eq!(entry_origin(&o), (314.0, 560.0 - 22.0 - 60.0));

    // The same portal higher up the window stays below the anchor.
    o.set_anchor("probe", Point::new(300.0, 100.0));
    draw(&mut o);
    assert_eq!(entry_origin(&o), (314.0, 122.0));
}

/// An anchored portal ignores where it was declared. Declaring it inside a
/// translated subtree — which for an *unanchored* portal is exactly what
/// `accumulated_translate` exists to capture — changes nothing.
#[test]
fn an_anchored_portal_ignores_its_declaration_site() {
    let src = r#"
let main = fn () {
  Flex {
    style: { width: "grow", height: "grow", padding: { left: 120, top: 90 } },
    children: [
      Flex {
        style: { width: "grow", height: "grow" },
        children: [
          Portal {
            open: true,
            layer: "tooltip",
            anchor: "probe",
            anchor_policy: "raw",
            children: [Flex { style: { width: 200, height: 60 }, children: [] }],
          }
        ],
      }
    ],
  }
};
"#;
    let mut o = settled(src);
    o.set_anchor("probe", Point::new(310.0, 240.0));
    draw(&mut o);
    assert_eq!(
        entry_origin(&o),
        (310.0, 240.0),
        "the 120/90 padding translate plays no part"
    );
}

// ── M3 — hit-test and occlusion parity ─────────────────────────────────
//
// These assert the design's central efficiency claim: because anchoring
// resolves into `PortalEntry::viewport_rect` and every hit-test path
// already reads that field, an anchored tooltip is clickable *where it is
// drawn* with no changes to hit-testing at all. The claim is cheap to
// state and easy to get wrong, so it is verified rather than assumed.

#[test]
fn a_click_at_the_anchored_position_reaches_the_child() {
    let mut o = settled(&anchored_src(r#"anchor_policy: "raw","#, 200.0, 60.0));
    o.set_anchor("probe", Point::new(300.0, 400.0));
    draw(&mut o);

    let ui = o.get_ui();
    let hit = ui
        .hit_test_drag_target(&Point::new(350.0, 420.0))
        .expect("something under the anchored card");
    let card = only_entry(&o)
        .unwrap()
        .widget
        .lock()
        .unwrap()
        .get_children()[0]
        .clone();
    assert!(
        Arc::ptr_eq(&hit, &card),
        "the hit lands on the portal's child, not the base tree"
    );
}

/// The mirror image: the portal's *declaration* site is not where it is
/// clickable. A Portal contributes nothing to its parent's flow, so the
/// point where it was written resolves to the base tree.
#[test]
fn a_click_at_the_declaration_site_does_not_reach_the_child() {
    let mut o = settled(&anchored_src(r#"anchor_policy: "raw","#, 200.0, 60.0));
    o.set_anchor("probe", Point::new(300.0, 400.0));
    draw(&mut o);

    let card = only_entry(&o)
        .unwrap()
        .widget
        .lock()
        .unwrap()
        .get_children()[0]
        .clone();
    if let Some(hit) = o.get_ui().hit_test_drag_target(&Point::new(10.0, 10.0)) {
        assert!(
            !Arc::ptr_eq(&hit, &card),
            "the declaration site must not route to the anchored card"
        );
    }
}

/// `UI::blocks_point` is what a host gates its world picking on. An
/// anchored tooltip has to occlude the world where it is drawn, or the
/// game keeps reacting underneath it.
#[test]
fn blocks_point_is_true_under_an_anchored_card() {
    // A transparent shell for a root, so the only thing that can block a
    // point is the anchored card itself — the shape a game uses when its
    // world renders under the chrome.
    let src = r#"
let main = fn () {
  Flex {
    style: { width: "grow", height: "grow" },
    block_interactions: false,
    children: [
      Portal {
        open: true,
        layer: "tooltip",
        anchor: "probe",
        anchor_policy: "raw",
        children: [
          Flex {
            style: { width: 200, height: 60 },
            mouse_down: fn () { event("card_press"); },
            children: [],
          }
        ],
      }
    ],
  }
};
"#;
    let mut o = settled(src);
    o.set_anchor("probe", Point::new(300.0, 400.0));
    draw(&mut o);

    assert!(
        o.get_ui().blocks_point(&Point::new(350.0, 420.0)),
        "the anchored card occludes world picking where it is drawn"
    );
    assert!(
        !o.get_ui().blocks_point(&Point::new(350.0, 100.0)),
        "and nowhere else — the world keeps its vote outside the card"
    );

    // Drop the anchor and the occlusion goes with it.
    o.clear_anchor("probe");
    draw(&mut o);
    assert!(
        !o.get_ui().blocks_point(&Point::new(350.0, 420.0)),
        "an unanchored portal occludes nothing"
    );
}

/// An anchored entry in a `Block`-policy layer still swallows clicks that
/// miss it — the backdrop policy is a property of the *layer*, and
/// anchoring doesn't opt out of it.
#[test]
fn an_anchored_overlay_modal_still_honours_the_block_backdrop() {
    let src = r#"
let main = fn () {
  Flex {
    style: { width: "grow", height: "grow" },
    mouse_down: fn () { event("base_press"); },
    children: [
      Portal {
        open: true,
        layer: "overlay-modal",
        anchor: "probe",
        anchor_policy: "raw",
        children: [Flex { style: { width: 100, height: 40 }, children: [] }],
      }
    ],
  }
};
"#;
    let mut o = settled(src);
    o.set_anchor("probe", Point::new(300.0, 400.0));
    draw(&mut o);

    // Far from the anchored card, but under a Block-policy layer.
    let hit = o.get_ui().hit_test_drag_target(&Point::new(20.0, 20.0));
    assert!(
        hit.is_none(),
        "a Block-policy layer suppresses fall-through wherever the entry sits"
    );
}

/// Nesting keeps working: `paint_portal_entry` recurses with
/// `accumulated_translate = viewport_rect.{x,y}`, so an ordinary portal
/// nested inside an anchored one inherits the anchored origin.
#[test]
fn a_portal_nested_inside_an_anchored_one_inherits_its_origin() {
    let src = r#"
let main = fn () {
  Portal {
    open: true,
    layer: "tooltip",
    anchor: "probe",
    anchor_policy: "raw",
    children: [
      Flex {
        style: { width: 200, height: 60, padding: { left: 10, top: 5 } },
        children: [
          Portal {
            open: true,
            layer: "toast",
            children: [Flex { style: { width: 40, height: 20 }, children: [] }],
          }
        ],
      }
    ],
  }
};
"#;
    let mut o = settled(src);
    o.set_anchor("probe", Point::new(300.0, 200.0));
    draw(&mut o);

    let ui = o.get_ui();
    let outer = ui
        .portal_layers
        .entries_in(PortalLayer::Tooltip)
        .first()
        .expect("outer entry");
    assert_eq!(
        (outer.viewport_rect.x, outer.viewport_rect.y),
        (300.0, 200.0)
    );
    // The nested portal is discovered during Pass B (inside
    // `paint_portal_entry`'s throwaway layer storage), so it doesn't
    // appear in `ui.portal_layers`. What matters here is that the outer
    // entry resolved from the anchor and Pass B ran without incident.
    assert!(
        ui.portal_layers.entries_in(PortalLayer::Toast).is_empty(),
        "nested portals paint through the outer entry's recursion"
    );
}

// ── M4 — the drag preview, re-expressed on anchors ─────────────────────
//
// The falsification test for the whole design. `tests/drag_preview.rs`
// stays green unmodified; these assert the *mechanism* underneath it is
// now the anchor path rather than a hardcoded second one.

#[test]
fn a_drag_seats_its_preview_at_the_reserved_anchor() {
    let mut origin = FlexWidget::new();
    origin.layout = Some(Rect::new(0.0, 0.0, 100.0, 100.0));
    origin.drag_preview = Some(Arc::new(Mutex::new(FlexWidget::new())));
    let origin_ref: WidgetRef = Arc::new(Mutex::new(origin));

    let mut ui = UI::new(origin_ref.clone());
    assert!(ui.anchor(DRAG_PREVIEW_ANCHOR).is_none());

    let mut state = ui.dispatch_drag_start(origin_ref.clone(), Value::Void, Point::new(50.0, 50.0));
    let seated = ui.anchor(DRAG_PREVIEW_ANCHOR).expect("drag_start anchors");
    assert_eq!((seated.x(), seated.y()), (50.0, 50.0));

    ui.dispatch_drag_move(&mut state, Point::new(40.0, 70.0));
    let moved = ui
        .anchor(DRAG_PREVIEW_ANCHOR)
        .expect("drag_move re-anchors");
    assert_eq!((moved.x(), moved.y()), (40.0, 70.0));
    assert_eq!(
        (
            ui.active_drag_preview().unwrap().cursor.x(),
            ui.active_drag_preview().unwrap().cursor.y()
        ),
        (40.0, 70.0),
        "the host-facing read-back tracks the anchor"
    );

    ui.dispatch_drag_end(&mut state, Point::new(40.0, 70.0));
    assert!(
        ui.anchor(DRAG_PREVIEW_ANCHOR).is_none(),
        "drag_end drops the reserved anchor with the preview"
    );
}

#[test]
fn clearing_an_active_drag_drops_the_reserved_anchor() {
    let mut origin = FlexWidget::new();
    origin.layout = Some(Rect::new(0.0, 0.0, 100.0, 100.0));
    origin.drag_preview = Some(Arc::new(Mutex::new(FlexWidget::new())));
    let origin_ref: WidgetRef = Arc::new(Mutex::new(origin));

    let mut ui = UI::new(origin_ref.clone());
    ui.dispatch_drag_start(origin_ref.clone(), Value::Void, Point::new(10.0, 10.0));
    ui.clear_active_drag();
    assert!(ui.anchor(DRAG_PREVIEW_ANCHOR).is_none());
}

/// End to end: the preview reaches the `CursorAttached` layer at the
/// cursor, through the same `AnchorContext::resolve` an `.ogh` Portal
/// uses. `Raw` policy — a preview pinned to a pointer at the window edge
/// must not be yanked back inside.
#[test]
fn the_drag_preview_paints_through_the_anchor_path() {
    let src = r#"
let main = fn () {
  Flex {
    style: { width: "grow", height: "grow" },
    drag_preview: Flex { style: { width: 32, height: 32 }, children: [] },
    children: [],
  }
};
"#;
    let mut o = settled(src);
    let root = o.get_ui().root.clone();
    let mut state = o.dispatch_drag_start(root.clone(), Value::Void, Point::new(795.0, 595.0));
    o.frame(W, H, DT).expect("frame");
    draw(&mut o);

    let entry = o
        .get_ui()
        .portal_layers
        .entries_in(PortalLayer::CursorAttached)
        .first()
        .cloned()
        .expect("the preview reached the CursorAttached layer");
    assert_eq!(
        (entry.viewport_rect.x, entry.viewport_rect.y),
        (795.0, 595.0),
        "raw: the preview stays on the pointer even at the window edge"
    );

    o.dispatch_drag_end(&mut state, Point::new(795.0, 595.0));
    draw(&mut o);
    assert!(
        o.get_ui()
            .portal_layers
            .entries_in(PortalLayer::CursorAttached)
            .is_empty(),
        "no anchor, no entry"
    );
}

// ── M5 — the shipped example ───────────────────────────────────────────

/// `examples/portals/anchored_tooltip.ogh` plus the `client` previewer's
/// `"cursor"` anchor are the two routes by which anyone discovers this
/// feature. A broken example is a feature nobody finds, so keep it
/// building — and keep it seated at the anchor the previewer publishes.
#[test]
fn the_shipped_example_builds_and_anchors_to_the_cursor() {
    let src = std::fs::read_to_string("examples/portals/anchored_tooltip.ogh")
        .expect("examples/portals/anchored_tooltip.ogh");
    let root = build_root(&src);

    fn anchors_in(node: &WidgetRef, out: &mut Vec<(String, AnchorPolicy)>) {
        let children = {
            let g = node.lock().unwrap();
            if let Some(info) = g.as_portal() {
                if let Some(id) = info.anchor.clone() {
                    out.push((id, info.anchor_policy));
                }
            }
            g.get_children()
        };
        for child in &children {
            anchors_in(child, out);
        }
    }

    let mut found = Vec::new();
    anchors_in(&root, &mut found);
    assert!(
        found.iter().all(|(id, _)| id == "cursor"),
        "the example anchors to the id the previewer sets: {:?}",
        found
    );
    assert!(
        found.iter().any(|(_, p)| *p == AnchorPolicy::Flip),
        "the example demonstrates flip: {:?}",
        found
    );
    assert!(
        found.iter().any(|(_, p)| *p == AnchorPolicy::Clamp),
        "…beside a clamp, so the difference is visible: {:?}",
        found
    );

    // Building isn't enough — an example whose cards measure 0×0 would
    // still "build" and show nothing. Run it, anchor it near the bottom
    // right, and check both cards came out with real size and landed
    // inside the viewport.
    let mut o = settled(&src);
    o.set_anchor("cursor", Point::new(760.0, 570.0));
    draw(&mut o);

    let entries: Vec<PortalEntry> = o
        .get_ui()
        .portal_layers
        .iter_paint_order()
        .cloned()
        .collect();
    assert_eq!(entries.len(), 2, "both example portals rendered");
    for e in &entries {
        let r = &e.viewport_rect;
        assert!(
            r.width > 0.0 && r.height > 0.0,
            "card measured {:?}",
            (r.width, r.height)
        );
        assert!(
            r.x >= 0.0 && r.y >= 0.0 && r.x + r.width <= W && r.y + r.height <= H,
            "anchored near the corner, both policies keep the card on screen: {:?}",
            (r.x, r.y, r.width, r.height)
        );
    }
}

// Keep the click-event import honest: anchored chrome is reachable
// through the ordinary event path too, not only the hit-test queries.
#[test]
fn an_anchored_card_consumes_a_press_at_its_drawn_position() {
    let mut o = settled(&anchored_src(r#"anchor_policy: "raw","#, 200.0, 60.0));
    o.set_anchor("probe", Point::new(300.0, 400.0));
    draw(&mut o);

    let mut event = Event::with_point("mouse_down".to_string(), Point::new(350.0, 420.0));
    event.payload = None;
    assert!(
        o.get_ui_mut().call_event(&event),
        "the press is handled by the anchored card"
    );
}
