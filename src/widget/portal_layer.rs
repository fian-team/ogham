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
