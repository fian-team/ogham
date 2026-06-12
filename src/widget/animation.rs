//! Spring-driven style transitions for Ogham widgets.
//!
//! The framework compares the old and new style at reconciliation time. For
//! any style property declared in `transition: { ... }`, a spring is set up
//! (or retargeted) so the rendered value eases toward the new target instead
//! of snapping. Each animating property is tracked as one or more independent
//! scalar springs; the rendered style is recomputed on every frame tick.

use super::style::{
    Border, Color, CornerShape, Corners, FlexStyle, InnerGlow, Opacity, Spacing, Transform,
};

/// Displacement below which a spring is considered at its target. Values
/// come from UI-scale coordinates (pixels for sizes, 0-255 for color
/// channels), so a threshold of 0.01 is well below any visible change.
const SETTLE_POS: f32 = 0.01;
/// Velocity below which a spring is considered at rest.
const SETTLE_VEL: f32 = 0.01;

/// Parameters shared by all springs belonging to a transition. Mass is
/// fixed at 1; `stiffness` and `damping` follow the same convention as
/// react-spring.
#[derive(Debug, Clone, Copy)]
pub struct TransitionConfig {
    pub stiffness: f32,
    pub damping: f32,
    /// Seconds a freshly created spring holds at its starting value before
    /// integrating. Applies when the spring is *created* (entry animations,
    /// first divergence of a property); a retarget of an already-live spring
    /// does not re-arm it. This is what staggered entrances are built from.
    pub delay: f32,
}

impl TransitionConfig {
    /// Pleasant UI spring (~300ms settle), comparable to react-spring's default.
    pub const DEFAULT: Self = Self {
        stiffness: 170.0,
        damping: 26.0,
        delay: 0.0,
    };
}

impl Default for TransitionConfig {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// A single scalar value driven by a damped spring with unit mass.
/// Advanced via sub-stepped semi-implicit Euler so that large `dt`
/// values (e.g. recovery from a minimized window) remain stable.
#[derive(Debug, Clone)]
pub struct Spring {
    pub current: f32,
    pub velocity: f32,
    pub target: f32,
    /// Seconds left of the config's start `delay`. While positive, `tick`
    /// burns time instead of integrating and the spring holds at `current`.
    pub delay: f32,
    pub config: TransitionConfig,
}

impl Spring {
    pub fn new(initial: f32, config: TransitionConfig) -> Self {
        Self {
            current: initial,
            velocity: 0.0,
            target: initial,
            delay: config.delay,
            config,
        }
    }

    pub fn set_target(&mut self, target: f32) {
        self.target = target;
    }

    /// Step the spring forward by `dt` seconds. Returns `true` while the
    /// spring is still moving. When it settles, current is snapped exactly
    /// to the target and the velocity is zeroed so no further ticks fire.
    pub fn tick(&mut self, mut dt: f32) -> bool {
        if self.delay > 0.0 {
            let consumed = self.delay.min(dt);
            self.delay -= consumed;
            dt -= consumed;
            if dt <= 0.0 {
                // Still holding at the start value. Report "moving" while
                // displaced so ticks keep coming and the hold isn't culled.
                return !self.is_settled();
            }
        }
        // Cap each integration step to keep the simple Euler integrator
        // stable at our default stiffness (~170). 1/120s is tight enough
        // for stiffness well north of 500 while keeping the typical case
        // (dt ~16ms) to two iterations.
        const MAX_SUB_DT: f32 = 1.0 / 120.0;

        let mut remaining = dt;
        while remaining > 0.0 {
            let step = remaining.min(MAX_SUB_DT);
            remaining -= step;
            let delta = self.target - self.current;
            let accel = self.config.stiffness * delta - self.config.damping * self.velocity;
            self.velocity += accel * step;
            self.current += self.velocity * step;
        }

        if (self.target - self.current).abs() < SETTLE_POS && self.velocity.abs() < SETTLE_VEL {
            self.current = self.target;
            self.velocity = 0.0;
            false
        } else {
            true
        }
    }

    pub fn is_settled(&self) -> bool {
        (self.target - self.current).abs() < SETTLE_POS && self.velocity.abs() < SETTLE_VEL
    }

    /// Shift the remaining start delay by `secs` (clamped at zero).
    /// Staggered containers use this to inject group-cascade offsets
    /// into freshly created springs — and, negated, to strip an
    /// inherited offset from a subtree that turned out to mount
    /// individually rather than with its group.
    pub fn add_delay(&mut self, secs: f32) {
        self.delay = (self.delay + secs).max(0.0);
    }
}

/// Per-property flags indicating which style fields should transition
/// smoothly when their target value changes. `None` means the property
/// snaps as before.
#[derive(Debug, Default, Clone)]
pub struct TransitionSet {
    pub background_color: Option<TransitionConfig>,
    pub text_color: Option<TransitionConfig>,
    pub border: Option<TransitionConfig>,
    /// Unified per-corner shape spring. Both `corner_radius:` and
    /// `corner_chamfer:` transition keys in the source map write here,
    /// since the rendered corner shape is one field internally.
    pub corners: Option<TransitionConfig>,
    pub padding: Option<TransitionConfig>,
    pub margin: Option<TransitionConfig>,
    pub gap: Option<TransitionConfig>,
    pub text_size: Option<TransitionConfig>,
    pub opacity: Option<TransitionConfig>,
    pub transform: Option<TransitionConfig>,
    pub inner_glow: Option<TransitionConfig>,
}

impl TransitionSet {
    pub fn any_enabled(&self) -> bool {
        self.background_color.is_some()
            || self.text_color.is_some()
            || self.border.is_some()
            || self.corners.is_some()
            || self.padding.is_some()
            || self.margin.is_some()
            || self.gap.is_some()
            || self.text_size.is_some()
            || self.opacity.is_some()
            || self.transform.is_some()
            || self.inner_glow.is_some()
    }
}

// ---- Compound springs -------------------------------------------------------

#[derive(Debug, Clone)]
pub struct ColorSprings {
    pub r: Spring,
    pub g: Spring,
    pub b: Spring,
    pub a: Spring,
}

impl ColorSprings {
    pub fn new(color: Color, cfg: TransitionConfig) -> Self {
        Self {
            r: Spring::new(color.r as f32, cfg),
            g: Spring::new(color.g as f32, cfg),
            b: Spring::new(color.b as f32, cfg),
            a: Spring::new(color.a as f32, cfg),
        }
    }

    pub fn set_target(&mut self, color: Color) {
        self.r.set_target(color.r as f32);
        self.g.set_target(color.g as f32);
        self.b.set_target(color.b as f32);
        self.a.set_target(color.a as f32);
    }

    pub fn tick(&mut self, dt: f32) -> bool {
        let a = self.r.tick(dt);
        let b = self.g.tick(dt);
        let c = self.b.tick(dt);
        let d = self.a.tick(dt);
        a || b || c || d
    }

    pub fn current(&self) -> Color {
        Color::new(
            self.r.current.clamp(0.0, 255.0).round() as u8,
            self.g.current.clamp(0.0, 255.0).round() as u8,
            self.b.current.clamp(0.0, 255.0).round() as u8,
            self.a.current.clamp(0.0, 255.0).round() as u8,
        )
    }

    pub fn is_settled(&self) -> bool {
        self.r.is_settled() && self.g.is_settled() && self.b.is_settled() && self.a.is_settled()
    }

    pub fn add_delay(&mut self, secs: f32) {
        self.r.add_delay(secs);
        self.g.add_delay(secs);
        self.b.add_delay(secs);
        self.a.add_delay(secs);
    }
}

#[derive(Debug, Clone)]
pub struct SpacingSprings {
    pub top: Spring,
    pub right: Spring,
    pub bottom: Spring,
    pub left: Spring,
}

impl SpacingSprings {
    pub fn new(s: &Spacing, cfg: TransitionConfig) -> Self {
        Self {
            top: Spring::new(s.top, cfg),
            right: Spring::new(s.right, cfg),
            bottom: Spring::new(s.bottom, cfg),
            left: Spring::new(s.left, cfg),
        }
    }

    pub fn set_target(&mut self, s: &Spacing) {
        self.top.set_target(s.top);
        self.right.set_target(s.right);
        self.bottom.set_target(s.bottom);
        self.left.set_target(s.left);
    }

    pub fn tick(&mut self, dt: f32) -> bool {
        let a = self.top.tick(dt);
        let b = self.right.tick(dt);
        let c = self.bottom.tick(dt);
        let d = self.left.tick(dt);
        a || b || c || d
    }

    pub fn current(&self) -> Spacing {
        Spacing::new(
            self.top.current,
            self.right.current,
            self.bottom.current,
            self.left.current,
        )
    }

    pub fn is_settled(&self) -> bool {
        self.top.is_settled()
            && self.right.is_settled()
            && self.bottom.is_settled()
            && self.left.is_settled()
    }

    pub fn add_delay(&mut self, secs: f32) {
        self.top.add_delay(secs);
        self.right.add_delay(secs);
        self.bottom.add_delay(secs);
        self.left.add_delay(secs);
    }
}

/// One corner's spring: tracks the kind (Sharp/Round/Chamfer) plus a
/// scalar spring on the size. Same-kind transitions interpolate the
/// scalar; kind-switches snap the kind and re-anchor the spring at
/// the current value, so the size eases from "wherever we are now"
/// up or down to the new target. Visible only if you tween across
/// kinds with both sizes nonzero — a deliberate v1 simplification.
#[derive(Debug, Clone, Copy, PartialEq)]
enum CornerKind {
    Sharp,
    Round,
    Chamfer,
}

impl CornerKind {
    fn of(shape: CornerShape) -> Self {
        match shape {
            CornerShape::Sharp => Self::Sharp,
            CornerShape::Round(_) => Self::Round,
            CornerShape::Chamfer(_) => Self::Chamfer,
        }
    }
}

#[derive(Debug, Clone)]
pub struct CornerSpring {
    kind: CornerKind,
    size: Spring,
}

impl CornerSpring {
    fn new(shape: CornerShape, cfg: TransitionConfig) -> Self {
        Self {
            kind: CornerKind::of(shape),
            size: Spring::new(shape.size(), cfg),
        }
    }

    fn set_target(&mut self, shape: CornerShape) {
        let target_kind = CornerKind::of(shape);
        if self.kind == target_kind {
            self.size.set_target(shape.size());
        } else {
            // Kind mismatch: snap kind, keep current scalar so the size
            // eases continuously from where it is to the new target.
            self.kind = target_kind;
            self.size.set_target(shape.size());
        }
    }

    fn current(&self) -> CornerShape {
        let s = self.size.current.max(0.0);
        match self.kind {
            CornerKind::Sharp => CornerShape::Sharp,
            CornerKind::Round => CornerShape::round(s),
            CornerKind::Chamfer => CornerShape::chamfer(s),
        }
    }

    fn tick(&mut self, dt: f32) -> bool {
        self.size.tick(dt)
    }

    fn is_settled(&self) -> bool {
        self.size.is_settled()
    }
}

#[derive(Debug, Clone)]
pub struct CornersSprings {
    pub top_left: CornerSpring,
    pub top_right: CornerSpring,
    pub bottom_left: CornerSpring,
    pub bottom_right: CornerSpring,
}

impl CornersSprings {
    pub fn new(c: &Corners, cfg: TransitionConfig) -> Self {
        Self {
            top_left: CornerSpring::new(c.top_left, cfg),
            top_right: CornerSpring::new(c.top_right, cfg),
            bottom_left: CornerSpring::new(c.bottom_left, cfg),
            bottom_right: CornerSpring::new(c.bottom_right, cfg),
        }
    }

    pub fn set_target(&mut self, c: &Corners) {
        self.top_left.set_target(c.top_left);
        self.top_right.set_target(c.top_right);
        self.bottom_left.set_target(c.bottom_left);
        self.bottom_right.set_target(c.bottom_right);
    }

    pub fn tick(&mut self, dt: f32) -> bool {
        let a = self.top_left.tick(dt);
        let b = self.top_right.tick(dt);
        let c = self.bottom_left.tick(dt);
        let d = self.bottom_right.tick(dt);
        a || b || c || d
    }

    pub fn current(&self) -> Corners {
        Corners::new(
            self.top_left.current(),
            self.top_right.current(),
            self.bottom_left.current(),
            self.bottom_right.current(),
        )
    }

    pub fn is_settled(&self) -> bool {
        self.top_left.is_settled()
            && self.top_right.is_settled()
            && self.bottom_left.is_settled()
            && self.bottom_right.is_settled()
    }

    pub fn add_delay(&mut self, secs: f32) {
        self.top_left.size.add_delay(secs);
        self.top_right.size.add_delay(secs);
        self.bottom_left.size.add_delay(secs);
        self.bottom_right.size.add_delay(secs);
    }
}

/// Springs for the three animatable components of an inner glow: the
/// four color channels, the blur sigma, and the spread. Width-style
/// (`Option<InnerGlow>`) appearance/disappearance is handled at the
/// reconcile site — these springs only interpolate Some→Some changes.
#[derive(Debug, Clone)]
pub struct InnerGlowSprings {
    pub color: ColorSprings,
    pub blur: Spring,
    pub spread: Spring,
}

impl InnerGlowSprings {
    pub fn new(g: &InnerGlow, cfg: TransitionConfig) -> Self {
        Self {
            color: ColorSprings::new(g.color, cfg),
            blur: Spring::new(g.blur, cfg),
            spread: Spring::new(g.spread, cfg),
        }
    }

    pub fn set_target(&mut self, g: &InnerGlow) {
        self.color.set_target(g.color);
        self.blur.set_target(g.blur);
        self.spread.set_target(g.spread);
    }

    pub fn tick(&mut self, dt: f32) -> bool {
        let a = self.color.tick(dt);
        let b = self.blur.tick(dt);
        let c = self.spread.tick(dt);
        a || b || c
    }

    pub fn current(&self) -> InnerGlow {
        InnerGlow {
            color: self.color.current(),
            blur: self.blur.current.max(0.0),
            spread: self.spread.current.max(0.0),
        }
    }

    pub fn is_settled(&self) -> bool {
        self.color.is_settled() && self.blur.is_settled() && self.spread.is_settled()
    }

    pub fn add_delay(&mut self, secs: f32) {
        self.color.add_delay(secs);
        self.blur.add_delay(secs);
        self.spread.add_delay(secs);
    }
}

/// Tracks spring state for each side's border width and color. Border
/// style (solid / dashed / dotted) is non-numeric and is not interpolated.
#[derive(Debug, Clone)]
pub struct BorderSprings {
    pub top_width: Spring,
    pub right_width: Spring,
    pub bottom_width: Spring,
    pub left_width: Spring,
    pub top_color: ColorSprings,
    pub right_color: ColorSprings,
    pub bottom_color: ColorSprings,
    pub left_color: ColorSprings,
}

impl BorderSprings {
    pub fn new(b: &Border, cfg: TransitionConfig) -> Self {
        Self {
            top_width: Spring::new(b.top.width, cfg),
            right_width: Spring::new(b.right.width, cfg),
            bottom_width: Spring::new(b.bottom.width, cfg),
            left_width: Spring::new(b.left.width, cfg),
            top_color: ColorSprings::new(b.top.color, cfg),
            right_color: ColorSprings::new(b.right.color, cfg),
            bottom_color: ColorSprings::new(b.bottom.color, cfg),
            left_color: ColorSprings::new(b.left.color, cfg),
        }
    }

    pub fn set_target(&mut self, b: &Border) {
        self.top_width.set_target(b.top.width);
        self.right_width.set_target(b.right.width);
        self.bottom_width.set_target(b.bottom.width);
        self.left_width.set_target(b.left.width);
        self.top_color.set_target(b.top.color);
        self.right_color.set_target(b.right.color);
        self.bottom_color.set_target(b.bottom.color);
        self.left_color.set_target(b.left.color);
    }

    pub fn tick(&mut self, dt: f32) -> bool {
        let r = [
            self.top_width.tick(dt),
            self.right_width.tick(dt),
            self.bottom_width.tick(dt),
            self.left_width.tick(dt),
            self.top_color.tick(dt),
            self.right_color.tick(dt),
            self.bottom_color.tick(dt),
            self.left_color.tick(dt),
        ];
        r.iter().any(|v| *v)
    }

    /// Overlay the spring-driven width and color values onto `base`,
    /// leaving style (solid/dashed/dotted) untouched.
    pub fn apply_to(&self, base: &mut Border) {
        base.top.width = self.top_width.current.max(0.0);
        base.right.width = self.right_width.current.max(0.0);
        base.bottom.width = self.bottom_width.current.max(0.0);
        base.left.width = self.left_width.current.max(0.0);
        base.top.color = self.top_color.current();
        base.right.color = self.right_color.current();
        base.bottom.color = self.bottom_color.current();
        base.left.color = self.left_color.current();
    }

    pub fn is_settled(&self) -> bool {
        self.top_width.is_settled()
            && self.right_width.is_settled()
            && self.bottom_width.is_settled()
            && self.left_width.is_settled()
            && self.top_color.is_settled()
            && self.right_color.is_settled()
            && self.bottom_color.is_settled()
            && self.left_color.is_settled()
    }

    /// True if any of the four width springs is still moving. Border
    /// *width* feeds into the box-model inset and so changes layout;
    /// border *color* is paint-only. A color-only hover transition must
    /// not trigger relayout.
    pub fn width_animating(&self) -> bool {
        !(self.top_width.is_settled()
            && self.right_width.is_settled()
            && self.bottom_width.is_settled()
            && self.left_width.is_settled())
    }

    pub fn add_delay(&mut self, secs: f32) {
        self.top_width.add_delay(secs);
        self.right_width.add_delay(secs);
        self.bottom_width.add_delay(secs);
        self.left_width.add_delay(secs);
        self.top_color.add_delay(secs);
        self.right_color.add_delay(secs);
        self.bottom_color.add_delay(secs);
        self.left_color.add_delay(secs);
    }
}

/// Springs for the five scalar components of an affine transform.
#[derive(Debug, Clone)]
pub struct TransformSprings {
    pub translate_x: Spring,
    pub translate_y: Spring,
    pub scale_x: Spring,
    pub scale_y: Spring,
    pub rotate: Spring,
}

impl TransformSprings {
    pub fn new(t: &Transform, cfg: TransitionConfig) -> Self {
        Self {
            translate_x: Spring::new(t.translate_x, cfg),
            translate_y: Spring::new(t.translate_y, cfg),
            scale_x: Spring::new(t.scale_x, cfg),
            scale_y: Spring::new(t.scale_y, cfg),
            rotate: Spring::new(t.rotate, cfg),
        }
    }

    pub fn set_target(&mut self, t: &Transform) {
        self.translate_x.set_target(t.translate_x);
        self.translate_y.set_target(t.translate_y);
        self.scale_x.set_target(t.scale_x);
        self.scale_y.set_target(t.scale_y);
        self.rotate.set_target(t.rotate);
    }

    pub fn tick(&mut self, dt: f32) -> bool {
        let a = self.translate_x.tick(dt);
        let b = self.translate_y.tick(dt);
        let c = self.scale_x.tick(dt);
        let d = self.scale_y.tick(dt);
        let e = self.rotate.tick(dt);
        a || b || c || d || e
    }

    pub fn current(&self) -> Transform {
        Transform {
            translate_x: self.translate_x.current,
            translate_y: self.translate_y.current,
            scale_x: self.scale_x.current,
            scale_y: self.scale_y.current,
            rotate: self.rotate.current,
        }
    }

    pub fn is_settled(&self) -> bool {
        self.translate_x.is_settled()
            && self.translate_y.is_settled()
            && self.scale_x.is_settled()
            && self.scale_y.is_settled()
            && self.rotate.is_settled()
    }

    pub fn add_delay(&mut self, secs: f32) {
        self.translate_x.add_delay(secs);
        self.translate_y.add_delay(secs);
        self.scale_x.add_delay(secs);
        self.scale_y.add_delay(secs);
        self.rotate.add_delay(secs);
    }
}

/// All springs active on a widget. Each field is populated lazily when a
/// transition is declared AND the corresponding property is set in the
/// target style.
#[derive(Debug, Default, Clone)]
pub struct AnimationState {
    pub background_color: Option<ColorSprings>,
    pub text_color: Option<ColorSprings>,
    pub border: Option<BorderSprings>,
    pub corners: Option<CornersSprings>,
    pub padding: Option<SpacingSprings>,
    pub margin: Option<SpacingSprings>,
    pub gap: Option<Spring>,
    pub text_size: Option<Spring>,
    pub opacity: Option<Spring>,
    pub transform: Option<TransformSprings>,
    pub inner_glow: Option<InnerGlowSprings>,
}

impl AnimationState {
    pub fn is_empty(&self) -> bool {
        self.background_color.is_none()
            && self.text_color.is_none()
            && self.border.is_none()
            && self.corners.is_none()
            && self.padding.is_none()
            && self.margin.is_none()
            && self.gap.is_none()
            && self.text_size.is_none()
            && self.opacity.is_none()
            && self.transform.is_none()
            && self.inner_glow.is_none()
    }

    /// Reconcile springs against `target`'s declared transitions. For each
    /// transition-enabled property, create a spring starting at `old`'s
    /// value (so the animation runs from old → new) or retarget an existing
    /// spring to preserve its current velocity mid-animation. Properties
    /// that lose their transition declaration have their springs cleared.
    pub fn retarget(&mut self, old: &FlexStyle, target: &FlexStyle) {
        // For every transition-enabled property: if a spring already
        // exists, ALWAYS update its target (preserves current + velocity).
        // Only the spring's *creation* is gated on old != target — once
        // a spring is live, multiple retargets in one frame must all
        // land, otherwise rapid hover on/off gets the spring stuck
        // targeting the wrong value because `old` is read from the
        // un-ticked `self.style`.

        // background_color
        match target.transitions.background_color {
            Some(cfg) => {
                if let Some(new_c) = target.background_color {
                    if let Some(springs) = self.background_color.as_mut() {
                        springs.set_target(new_c);
                    } else if let Some(old_c) = old.background_color {
                        if !color_matches(new_c, old_c) {
                            let mut springs = ColorSprings::new(old_c, cfg);
                            springs.set_target(new_c);
                            self.background_color = Some(springs);
                        }
                    }
                }
            }
            None => {
                self.background_color = None;
            }
        }

        // text_color
        match target.transitions.text_color {
            Some(cfg) => {
                if let Some(new_c) = target.text_color {
                    if let Some(springs) = self.text_color.as_mut() {
                        springs.set_target(new_c);
                    } else if let Some(old_c) = old.text_color {
                        if !color_matches(new_c, old_c) {
                            let mut springs = ColorSprings::new(old_c, cfg);
                            springs.set_target(new_c);
                            self.text_color = Some(springs);
                        }
                    }
                }
            }
            None => {
                self.text_color = None;
            }
        }

        // border
        if let Some(cfg) = target.transitions.border {
            if let Some(springs) = self.border.as_mut() {
                springs.set_target(&target.border);
            } else if !border_matches(&old.border, &target.border) {
                let mut springs = BorderSprings::new(&old.border, cfg);
                springs.set_target(&target.border);
                self.border = Some(springs);
            }
        } else {
            self.border = None;
        }

        // corners
        if let Some(cfg) = target.transitions.corners {
            if let Some(springs) = self.corners.as_mut() {
                springs.set_target(&target.corners);
            } else if !corners_match(&old.corners, &target.corners) {
                let mut springs = CornersSprings::new(&old.corners, cfg);
                springs.set_target(&target.corners);
                self.corners = Some(springs);
            }
        } else {
            self.corners = None;
        }

        // padding
        if let Some(cfg) = target.transitions.padding {
            if let Some(springs) = self.padding.as_mut() {
                springs.set_target(&target.padding);
            } else if !spacing_matches(&old.padding, &target.padding) {
                let mut springs = SpacingSprings::new(&old.padding, cfg);
                springs.set_target(&target.padding);
                self.padding = Some(springs);
            }
        } else {
            self.padding = None;
        }

        // margin
        if let Some(cfg) = target.transitions.margin {
            if let Some(springs) = self.margin.as_mut() {
                springs.set_target(&target.margin);
            } else if !spacing_matches(&old.margin, &target.margin) {
                let mut springs = SpacingSprings::new(&old.margin, cfg);
                springs.set_target(&target.margin);
                self.margin = Some(springs);
            }
        } else {
            self.margin = None;
        }

        // gap
        if let Some(cfg) = target.transitions.gap {
            if let Some(spring) = self.gap.as_mut() {
                spring.set_target(target.gap);
            } else if (old.gap - target.gap).abs() > f32::EPSILON {
                let mut spring = Spring::new(old.gap, cfg);
                spring.set_target(target.gap);
                self.gap = Some(spring);
            }
        } else {
            self.gap = None;
        }

        // text_size
        if let Some(cfg) = target.transitions.text_size {
            if let Some(new_ts) = target.text_size {
                if let Some(spring) = self.text_size.as_mut() {
                    spring.set_target(new_ts);
                } else if let Some(old_ts) = old.text_size {
                    if (new_ts - old_ts).abs() > f32::EPSILON {
                        let mut spring = Spring::new(old_ts, cfg);
                        spring.set_target(new_ts);
                        self.text_size = Some(spring);
                    }
                }
            }
        } else {
            self.text_size = None;
        }

        // opacity
        if let Some(cfg) = target.transitions.opacity {
            let new_o = target.opacity.value();
            if let Some(spring) = self.opacity.as_mut() {
                spring.set_target(new_o);
            } else {
                let old_o = old.opacity.value();
                if (new_o - old_o).abs() > f32::EPSILON {
                    let mut spring = Spring::new(old_o, cfg);
                    spring.set_target(new_o);
                    self.opacity = Some(spring);
                }
            }
        } else {
            self.opacity = None;
        }

        // transform
        if let Some(cfg) = target.transitions.transform {
            if let Some(springs) = self.transform.as_mut() {
                springs.set_target(&target.transform);
            } else if !transform_matches(&old.transform, &target.transform) {
                let mut springs = TransformSprings::new(&old.transform, cfg);
                springs.set_target(&target.transform);
                self.transform = Some(springs);
            }
        } else {
            self.transform = None;
        }

        // inner_glow — Some→Some interpolates; None↔Some snaps for v1
        // (mirrors how `text_size` only animates when both old and new
        // are Some). For fade-in on appearance, the call site can use
        // `initial:` on the parent widget.
        if let Some(cfg) = target.transitions.inner_glow {
            if let (Some(new_g), Some(old_g)) = (target.inner_glow, old.inner_glow) {
                if let Some(springs) = self.inner_glow.as_mut() {
                    springs.set_target(&new_g);
                } else if !inner_glow_matches(&old_g, &new_g) {
                    let mut springs = InnerGlowSprings::new(&old_g, cfg);
                    springs.set_target(&new_g);
                    self.inner_glow = Some(springs);
                }
            } else {
                self.inner_glow = None;
            }
        } else {
            self.inner_glow = None;
        }
    }

    /// Step each active spring by `dt`. Clears springs that have settled
    /// so they stop driving redraws. Returns `true` while any spring is
    /// still moving.
    pub fn tick(&mut self, dt: f32) -> bool {
        let mut any_moving = false;

        if let Some(springs) = self.background_color.as_mut() {
            if springs.tick(dt) {
                any_moving = true;
            } else {
                self.background_color = None;
            }
        }
        if let Some(springs) = self.text_color.as_mut() {
            if springs.tick(dt) {
                any_moving = true;
            } else {
                self.text_color = None;
            }
        }
        if let Some(springs) = self.border.as_mut() {
            if springs.tick(dt) {
                any_moving = true;
            } else {
                self.border = None;
            }
        }
        if let Some(springs) = self.corners.as_mut() {
            if springs.tick(dt) {
                any_moving = true;
            } else {
                self.corners = None;
            }
        }
        if let Some(springs) = self.padding.as_mut() {
            if springs.tick(dt) {
                any_moving = true;
            } else {
                self.padding = None;
            }
        }
        if let Some(springs) = self.margin.as_mut() {
            if springs.tick(dt) {
                any_moving = true;
            } else {
                self.margin = None;
            }
        }
        if let Some(spring) = self.gap.as_mut() {
            if spring.tick(dt) {
                any_moving = true;
            } else {
                self.gap = None;
            }
        }
        if let Some(spring) = self.text_size.as_mut() {
            if spring.tick(dt) {
                any_moving = true;
            } else {
                self.text_size = None;
            }
        }
        if let Some(spring) = self.opacity.as_mut() {
            if spring.tick(dt) {
                any_moving = true;
            } else {
                self.opacity = None;
            }
        }
        if let Some(springs) = self.transform.as_mut() {
            if springs.tick(dt) {
                any_moving = true;
            } else {
                self.transform = None;
            }
        }
        if let Some(springs) = self.inner_glow.as_mut() {
            if springs.tick(dt) {
                any_moving = true;
            } else {
                self.inner_glow = None;
            }
        }

        any_moving
    }

    /// Produce the rendered style by overlaying active spring values onto
    /// `target`. Called after `tick()` to refresh the cached style used by
    /// `effective_style()`.
    pub fn render_onto(&self, target: &FlexStyle) -> FlexStyle {
        let mut out = target.clone();

        if let Some(springs) = self.background_color.as_ref() {
            out.background_color = Some(springs.current());
        }
        if let Some(springs) = self.text_color.as_ref() {
            out.text_color = Some(springs.current());
        }
        if let Some(springs) = self.border.as_ref() {
            springs.apply_to(&mut out.border);
        }
        if let Some(springs) = self.corners.as_ref() {
            out.corners = springs.current();
        }
        if let Some(springs) = self.padding.as_ref() {
            out.padding = springs.current();
        }
        if let Some(springs) = self.margin.as_ref() {
            out.margin = springs.current();
        }
        if let Some(spring) = self.gap.as_ref() {
            out.gap = spring.current.max(0.0);
        }
        if let Some(spring) = self.text_size.as_ref() {
            out.text_size = Some(spring.current.max(0.0));
        }
        if let Some(spring) = self.opacity.as_ref() {
            out.opacity = Opacity(spring.current.clamp(0.0, 1.0));
        }
        if let Some(springs) = self.transform.as_ref() {
            out.transform = springs.current();
        }
        if let Some(springs) = self.inner_glow.as_ref() {
            out.inner_glow = Some(springs.current());
        }

        out
    }

    /// True if any active spring drives a layout-affecting property
    /// (padding, margin, border width, gap, text size). Border *color*
    /// and corner-radius springs are paint-only — see `FlexStyle::layout_equal`
    /// — so we only flag a border spring when its width components are
    /// still moving, and corner_radius is excluded entirely.
    pub fn has_layout_effects(&self) -> bool {
        self.border.as_ref().is_some_and(|b| b.width_animating())
            || self.padding.is_some()
            || self.margin.is_some()
            || self.gap.is_some()
            || self.text_size.is_some()
    }

    /// Shift every active spring's remaining start delay by `secs`
    /// (each clamped at zero). The group-stagger injection/strip hook.
    pub fn add_delay(&mut self, secs: f32) {
        if let Some(s) = self.background_color.as_mut() {
            s.add_delay(secs);
        }
        if let Some(s) = self.text_color.as_mut() {
            s.add_delay(secs);
        }
        if let Some(s) = self.border.as_mut() {
            s.add_delay(secs);
        }
        if let Some(s) = self.corners.as_mut() {
            s.add_delay(secs);
        }
        if let Some(s) = self.padding.as_mut() {
            s.add_delay(secs);
        }
        if let Some(s) = self.margin.as_mut() {
            s.add_delay(secs);
        }
        if let Some(s) = self.gap.as_mut() {
            s.add_delay(secs);
        }
        if let Some(s) = self.text_size.as_mut() {
            s.add_delay(secs);
        }
        if let Some(s) = self.opacity.as_mut() {
            s.add_delay(secs);
        }
        if let Some(s) = self.transform.as_mut() {
            s.add_delay(secs);
        }
        if let Some(s) = self.inner_glow.as_mut() {
            s.add_delay(secs);
        }
    }
}

fn spacing_matches(a: &Spacing, b: &Spacing) -> bool {
    (a.top - b.top).abs() < f32::EPSILON
        && (a.right - b.right).abs() < f32::EPSILON
        && (a.bottom - b.bottom).abs() < f32::EPSILON
        && (a.left - b.left).abs() < f32::EPSILON
}

fn corner_shape_matches(a: CornerShape, b: CornerShape) -> bool {
    match (a, b) {
        (CornerShape::Sharp, CornerShape::Sharp) => true,
        (CornerShape::Round(x), CornerShape::Round(y)) => (x - y).abs() < f32::EPSILON,
        (CornerShape::Chamfer(x), CornerShape::Chamfer(y)) => (x - y).abs() < f32::EPSILON,
        _ => false,
    }
}

fn corners_match(a: &Corners, b: &Corners) -> bool {
    corner_shape_matches(a.top_left, b.top_left)
        && corner_shape_matches(a.top_right, b.top_right)
        && corner_shape_matches(a.bottom_left, b.bottom_left)
        && corner_shape_matches(a.bottom_right, b.bottom_right)
}

fn inner_glow_matches(a: &InnerGlow, b: &InnerGlow) -> bool {
    color_matches(a.color, b.color)
        && (a.blur - b.blur).abs() < f32::EPSILON
        && (a.spread - b.spread).abs() < f32::EPSILON
}

fn border_matches(a: &Border, b: &Border) -> bool {
    (a.top.width - b.top.width).abs() < f32::EPSILON
        && (a.right.width - b.right.width).abs() < f32::EPSILON
        && (a.bottom.width - b.bottom.width).abs() < f32::EPSILON
        && (a.left.width - b.left.width).abs() < f32::EPSILON
        && color_matches(a.top.color, b.top.color)
        && color_matches(a.right.color, b.right.color)
        && color_matches(a.bottom.color, b.bottom.color)
        && color_matches(a.left.color, b.left.color)
}

fn color_matches(a: Color, b: Color) -> bool {
    a.r == b.r && a.g == b.g && a.b == b.b && a.a == b.a
}

fn transform_matches(a: &Transform, b: &Transform) -> bool {
    (a.translate_x - b.translate_x).abs() < f32::EPSILON
        && (a.translate_y - b.translate_y).abs() < f32::EPSILON
        && (a.scale_x - b.scale_x).abs() < f32::EPSILON
        && (a.scale_y - b.scale_y).abs() < f32::EPSILON
        && (a.rotate - b.rotate).abs() < f32::EPSILON
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spring_settles_at_target() {
        let mut s = Spring::new(0.0, TransitionConfig::DEFAULT);
        s.set_target(100.0);
        // Integrate ~2 seconds — well past settle time for default config.
        for _ in 0..120 {
            s.tick(1.0 / 60.0);
        }
        assert!(s.is_settled(), "spring did not settle");
        assert!((s.current - 100.0).abs() < 0.5, "current={}", s.current);
    }

    #[test]
    fn spring_holds_through_delay_then_settles() {
        let cfg = TransitionConfig {
            delay: 0.5,
            ..TransitionConfig::DEFAULT
        };
        let mut s = Spring::new(0.0, cfg);
        s.set_target(100.0);
        // During the hold: no movement, but still reported as animating.
        assert!(s.tick(0.3), "held spring must keep ticking");
        assert_eq!(s.current, 0.0, "spring moved during its delay");
        assert!(!s.is_settled());
        // This tick crosses the delay boundary: 0.2s burns the hold, the
        // remaining 0.1s integrates.
        assert!(s.tick(0.3));
        assert!(s.current > 0.0, "spring should move once the delay elapses");
        for _ in 0..120 {
            s.tick(1.0 / 60.0);
        }
        assert!(s.is_settled());
        assert!((s.current - 100.0).abs() < 0.5);
    }

    #[test]
    fn spring_clamps_huge_dt() {
        // Huge dt from e.g. a stalled window. The spring must not NaN or
        // explode; it should reach the target deterministically.
        let mut s = Spring::new(0.0, TransitionConfig::DEFAULT);
        s.set_target(50.0);
        s.tick(5.0);
        assert!(s.current.is_finite());
    }

    #[test]
    fn interrupted_target_carries_velocity() {
        let mut s = Spring::new(0.0, TransitionConfig::DEFAULT);
        s.set_target(100.0);
        // Partway through.
        for _ in 0..10 {
            s.tick(1.0 / 60.0);
        }
        let mid = s.current;
        let vel_before = s.velocity;
        s.set_target(-100.0);
        assert_eq!(s.velocity, vel_before, "velocity should survive retarget");
        assert_eq!(s.current, mid, "current should survive retarget");
    }

    #[test]
    fn color_springs_interpolate_channels_independently() {
        let mut cs = ColorSprings::new(Color::new(0, 0, 0, 255), TransitionConfig::DEFAULT);
        cs.set_target(Color::new(255, 128, 0, 255));
        for _ in 0..120 {
            cs.tick(1.0 / 60.0);
        }
        let c = cs.current();
        assert_eq!(c.r, 255);
        assert_eq!(c.g, 128);
        assert_eq!(c.b, 0);
        assert_eq!(c.a, 255);
    }
}
