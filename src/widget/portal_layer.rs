//! Phase 2.5 M0: portal layer system. Five named layers with
//! priority ordering and per-layer backdrop policies, per UL's
//! `UI_RUNTIME.md` §1.
//!
//! Cross-layer paint order is determined by priority (higher
//! paints on top); within-layer ordering is mount-order LIFO.
//! Hit-test walks the inverse: high-priority-to-low; within a
//! layer, reverse-mount-order (top-most-mount first).
//!
//! Layers are a fixed runtime-known set — not extensible from
//! userspace. New patterns that need a new layer require a
//! runtime change; that's a feature, not a bug, because it
//! forces design review of layer-priority decisions.
//!
//! This module also owns [`AnchorPolicy`] and [`resolve_anchor`]
//! — the seating rules for a Portal whose viewport origin comes
//! from a host-set anchor rather than from Pass-A translate
//! accumulation. They live here rather than in `skia.rs` so the
//! edge arithmetic is a pure function that tests can drive
//! without a window.

/// Named portal layers with priority ordering. Discriminants
/// are deliberate gaps (0/100/200/...) so future intermediate
/// layers can be inserted without renumbering. The values are
/// also the priority — higher == paints on top.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PortalLayer {
    /// The default panel layout tree. Rare to declare
    /// explicitly; widgets that aren't inside any Portal land
    /// in the main render pass and never reach
    /// [`PortalLayers`]. A `Portal { layer: "main" }` is legal
    /// but unusual — would participate in normal flex layout.
    Main = 0,
    /// Full-screen modals — escape menu, settings, dialogs.
    /// Default backdrop: [`BackdropPolicy::Block`].
    OverlayModal = 100,
    /// Dropdowns, context menus, sub-menus.
    /// Default backdrop: [`BackdropPolicy::None`].
    Popover = 200,
    /// Hover-spawned tooltips.
    /// Default backdrop: [`BackdropPolicy::None`].
    Tooltip = 300,
    /// Ephemeral notifications — toast queue.
    /// Default backdrop: [`BackdropPolicy::None`].
    Toast = 400,
    /// Drag previews, custom cursor effects.
    /// Default backdrop: [`BackdropPolicy::None`].
    CursorAttached = 500,
}

impl PortalLayer {
    /// All layers in priority order (low → high). Used to
    /// allocate per-frame storage and iterate Pass B paint
    /// (low-to-high so higher layers paint on top) and
    /// hit-test (high-to-low; reverse this slice).
    pub const ALL: [PortalLayer; 6] = [
        Self::Main,
        Self::OverlayModal,
        Self::Popover,
        Self::Tooltip,
        Self::Toast,
        Self::CursorAttached,
    ];

    /// Numeric priority — higher means paints on top.
    pub fn priority(self) -> u32 {
        self as u32
    }

    /// Index in [`Self::ALL`] (0..6). Used by `PortalLayers`
    /// to address its `[Vec; 6]` storage. Discriminants are
    /// gapped (0/100/200/...) so we can't use `self as usize`
    /// for array indexing.
    pub fn array_index(self) -> usize {
        match self {
            Self::Main => 0,
            Self::OverlayModal => 1,
            Self::Popover => 2,
            Self::Tooltip => 3,
            Self::Toast => 4,
            Self::CursorAttached => 5,
        }
    }

    /// Per-layer default backdrop policy. Per UL's
    /// `UI_RUNTIME.md` §1: only `OverlayModal` defaults to
    /// `Block`; everything else defaults to `None`. Userspace
    /// can render its own backdrop into the layer for finer
    /// control.
    pub fn default_backdrop(self) -> BackdropPolicy {
        match self {
            Self::OverlayModal => BackdropPolicy::Block,
            _ => BackdropPolicy::None,
        }
    }

    /// Parse a string layer name as it appears in `.ogh`
    /// source. Returns `None` for unknown names — the caller
    /// surfaces the diagnostic with the list of valid names.
    pub fn from_source_name(name: &str) -> Option<Self> {
        match name {
            "main" => Some(Self::Main),
            "overlay-modal" => Some(Self::OverlayModal),
            "popover" => Some(Self::Popover),
            "tooltip" => Some(Self::Tooltip),
            "toast" => Some(Self::Toast),
            "cursor-attached" => Some(Self::CursorAttached),
            _ => None,
        }
    }

    /// String name as it appears in `.ogh` source. Used by
    /// LSP hover, error messages, and the "list of valid
    /// names" diagnostic.
    pub fn source_name(self) -> &'static str {
        match self {
            Self::Main => "main",
            Self::OverlayModal => "overlay-modal",
            Self::Popover => "popover",
            Self::Tooltip => "tooltip",
            Self::Toast => "toast",
            Self::CursorAttached => "cursor-attached",
        }
    }

    /// Comma-separated list of all layer names, for
    /// diagnostic messages.
    pub fn all_names_for_diagnostic() -> &'static str {
        "main, overlay-modal, popover, tooltip, toast, cursor-attached"
    }
}

/// Phase 2.5 M1: per-portal/per-widget cursor coordination
/// signal. Per [`UI_RUNTIME.md`](../../../untold_lore/docs/UI_RUNTIME.md)
/// §4: a runtime-side declaration that the cursor should be
/// visible / free. Game-side composes this with its own
/// cursor-lock demand.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CursorPreference {
    /// Cursor should be visible / unlocked. Used by modal
    /// portals (so the user can interact with the dialog) and
    /// focused TextInputs (so the user can see what they're
    /// typing).
    Free,
    /// Don't influence cursor state — defer to other signals.
    /// Default for tooltip / toast / cursor-attached layers.
    Inherit,
}

impl PortalLayer {
    /// Phase 2.5 M1: per-layer default cursor preference.
    /// `OverlayModal` and `Popover` default to `Free` (the
    /// user is interacting with the modal/menu); other layers
    /// default to `Inherit` (tooltip / toast don't influence
    /// cursor state).
    pub fn default_cursor(self) -> CursorPreference {
        match self {
            Self::OverlayModal | Self::Popover => CursorPreference::Free,
            _ => CursorPreference::Inherit,
        }
    }
}

/// How an anchored Portal's box is seated against the viewport
/// once its content size is known.
///
/// A Portal that names an `anchor:` takes its viewport origin
/// from a host-set point instead of from Pass-A translate
/// accumulation. The point alone isn't enough: chrome pinned to
/// the pointer near the right edge of the window runs off it, and
/// a tooltip near the bottom wants to sit *above* the pointer
/// rather than be shoved up into it. Both corrections need the
/// subtree's measured size, which `.ogh` cannot see — hence a
/// named policy resolved in the renderer rather than arithmetic
/// in the language.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AnchorPolicy {
    /// Use the anchor point (plus offset) as-is. The escape
    /// hatch: honest about going off-screen, which is what a
    /// host that has already done its own edge math wants.
    Raw,
    /// Keep the whole box inside the viewport, inset by
    /// [`ANCHOR_VIEWPORT_INSET`] on every edge. The default,
    /// because "visible" is the overwhelmingly common intent.
    #[default]
    Clamp,
    /// Clamp horizontally; vertically, flip to sit *above* the
    /// anchor when the box would overrun the bottom edge. The
    /// cursor-tooltip rule: chrome that would be clipped by the
    /// bottom of the window reads better above the pointer than
    /// jammed against the sill.
    Flip,
}

/// Margin an anchored Portal's box keeps from every viewport
/// edge under [`AnchorPolicy::Clamp`] and [`AnchorPolicy::Flip`].
///
/// 8 logical px, matching the hand-rolled clamps this feature
/// exists to delete (regency's `x.min(w - card_w - 8.0).max(8.0)`).
/// Not configurable: a per-portal inset is a style knob dressed
/// up as a policy, and no consumer has wanted a second value.
pub const ANCHOR_VIEWPORT_INSET: f32 = 8.0;

impl AnchorPolicy {
    /// All policies, in the order the diagnostic lists them.
    pub const ALL: [AnchorPolicy; 3] = [Self::Raw, Self::Clamp, Self::Flip];

    /// Parse a string policy name as it appears in `.ogh`
    /// source. Returns `None` for unknown names — the caller
    /// surfaces the diagnostic with the list of valid names.
    /// Mirrors [`PortalLayer::from_source_name`].
    pub fn from_source_name(name: &str) -> Option<Self> {
        match name {
            "raw" => Some(Self::Raw),
            "clamp" => Some(Self::Clamp),
            "flip" => Some(Self::Flip),
            _ => None,
        }
    }

    /// String name as it appears in `.ogh` source. Used by LSP
    /// hover, error messages, and the "list of valid names"
    /// diagnostic.
    pub fn source_name(self) -> &'static str {
        match self {
            Self::Raw => "raw",
            Self::Clamp => "clamp",
            Self::Flip => "flip",
        }
    }

    /// Comma-separated list of all policy names, for diagnostic
    /// messages.
    pub fn all_names_for_diagnostic() -> &'static str {
        "raw, clamp, flip"
    }
}

/// Seat a box of `size` at a host-set anchor `point`, nudged by
/// `offset`, under `policy`, inside a `viewport`. Returns the
/// box's viewport-absolute top-left.
///
/// A **pure function of five numbers** on purpose: the three
/// policies are the part of anchoring most likely to be wrong at
/// an edge, and keeping them out of the Skia walk means they are
/// unit-testable without a window (see `tests/anchored_portals.rs`).
///
/// `offset` is applied *before* the policy, so a cursor tooltip
/// declared at `{ x: 14, y: 22 }` sits below-right of the pointer
/// and only then gets pulled back inside the viewport.
///
/// A non-positive viewport dimension disables clamping on that
/// axis: before the first layout pass there is no viewport to
/// resolve against, and inventing one would park every anchored
/// portal at the inset corner instead of leaving it where the
/// host asked.
pub fn resolve_anchor(
    point: (f32, f32),
    offset: (f32, f32),
    policy: AnchorPolicy,
    size: (f32, f32),
    viewport: (f32, f32),
) -> (f32, f32) {
    let (x, y) = (point.0 + offset.0, point.1 + offset.1);
    match policy {
        AnchorPolicy::Raw => (x, y),
        AnchorPolicy::Clamp => (
            clamp_axis(x, size.0, viewport.0),
            clamp_axis(y, size.1, viewport.1),
        ),
        AnchorPolicy::Flip => {
            // Flip only when the below-the-anchor placement would
            // overrun the bottom inset. The flipped placement
            // mirrors the offset too, so a `+22` nudge downward
            // becomes a `-22` nudge upward and the box clears the
            // anchor by the same margin on either side.
            let overruns_bottom =
                viewport.1 > 0.0 && y + size.1 > viewport.1 - ANCHOR_VIEWPORT_INSET;
            let seated_y = if overruns_bottom {
                point.1 - offset.1 - size.1
            } else {
                y
            };
            (
                clamp_axis(x, size.0, viewport.0),
                // Clamp the result either way: a flipped box can
                // still run off the top in a short viewport, and
                // an un-flipped one can start above the top inset
                // if the host anchored near y = 0.
                clamp_axis(seated_y, size.1, viewport.1),
            )
        }
    }
}

/// One axis of the clamp. `min` before `max` so the inset wins
/// when the box is larger than the viewport — a box that cannot
/// fit is better pinned to the top-left than pushed off it.
fn clamp_axis(v: f32, size: f32, viewport: f32) -> f32 {
    if viewport <= 0.0 {
        return v;
    }
    v.min(viewport - size - ANCHOR_VIEWPORT_INSET)
        .max(ANCHOR_VIEWPORT_INSET)
}

/// Backdrop / pointer-event policy for a portal layer. Applied
/// at layer boundaries during Pass B paint and hit-test.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackdropPolicy {
    /// No dimming, lower layers receive clicks normally.
    None,
    /// Lower layers rendered, pointer events blocked from
    /// reaching them. The renderer paints a viewport-sized
    /// translucent backdrop before the first entry in a
    /// Block-policy layer; the hit-test gate suppresses
    /// fall-through to the base tree.
    Block,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn priority_ordering_is_strict() {
        let priorities: Vec<u32> = PortalLayer::ALL.iter().map(|l| l.priority()).collect();
        let mut sorted = priorities.clone();
        sorted.sort();
        assert_eq!(priorities, sorted, "ALL is not in priority order");
    }

    #[test]
    fn from_source_name_round_trips() {
        for layer in PortalLayer::ALL {
            let name = layer.source_name();
            assert_eq!(PortalLayer::from_source_name(name), Some(layer));
        }
    }

    #[test]
    fn from_source_name_rejects_unknown() {
        assert!(PortalLayer::from_source_name("modal").is_none());
        assert!(PortalLayer::from_source_name("MODAL").is_none());
        assert!(PortalLayer::from_source_name("").is_none());
        assert!(PortalLayer::from_source_name("overlay_modal").is_none());
    }

    #[test]
    fn anchor_policy_from_source_name_round_trips() {
        for policy in AnchorPolicy::ALL {
            let name = policy.source_name();
            assert_eq!(AnchorPolicy::from_source_name(name), Some(policy));
            assert!(
                AnchorPolicy::all_names_for_diagnostic().contains(name),
                "{:?} missing from the diagnostic list",
                policy
            );
        }
    }

    #[test]
    fn anchor_policy_rejects_unknown() {
        // Same shape as the layer names: no case folding, no
        // near-misses. An unrecognised policy is a build error,
        // not a silent fallback to the default.
        assert!(AnchorPolicy::from_source_name("Clamp").is_none());
        assert!(AnchorPolicy::from_source_name("clamped").is_none());
        assert!(AnchorPolicy::from_source_name("none").is_none());
        assert!(AnchorPolicy::from_source_name("").is_none());
    }

    #[test]
    fn anchor_policy_defaults_to_clamp() {
        assert_eq!(AnchorPolicy::default(), AnchorPolicy::Clamp);
    }

    #[test]
    fn only_overlay_modal_defaults_to_block() {
        for layer in PortalLayer::ALL {
            let policy = layer.default_backdrop();
            if layer == PortalLayer::OverlayModal {
                assert_eq!(policy, BackdropPolicy::Block);
            } else {
                assert_eq!(
                    policy,
                    BackdropPolicy::None,
                    "{:?} should default to None",
                    layer
                );
            }
        }
    }
}
