//! The router: the active path, the walk that derives it, lifecycle,
//! depth arbitration and occlusion.
//!
//! # The walk
//!
//! The path is **derived, never pushed** (axiom 3). Every frame the router
//! asks the game's root resolver which route a path starts at, then walks
//! down: each route on the path answers `resolve_child(cx)`, and the walk
//! stops at the first route that claims none.
//!
//! `resolve_child` returns an `Option`, so a node claims **at most one
//! child, structurally**. That is what makes the walk total and ordered
//! with no arbiter above it, and it is why `n` independent stance booleans
//! — `2ⁿ` states of which `n+1` are legal, so every entry point has to
//! re-assert the invariant — collapse into one `Option<RouteId>` that
//! cannot hold two values (`ROUTING.md` §13.3).
//!
//! # Lifecycle
//!
//! `enter` and `leave` mean *the id entered or left the path*, not "became
//! deepest" (axiom 10). A prompt pushed above the map editor must not
//! disturb the transaction the map editor is holding, so the editor's
//! `leave` does not run when its own exit prompt appears above it.

use std::collections::BTreeMap;

use crate::route::table::{RouteTable, TableError};
use crate::route::{Departure, Escape, Handled, Occlusion, Outbox, Route, RouteEvent, RouteId};

/// What the router did with an Escape, so the host can tell "nobody wanted
/// it" from "it was consumed".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EscapeOutcome {
    /// A route popped; the path is shorter.
    Popped(RouteId),
    /// A route raised a prompt instead of leaving. The path is unchanged
    /// this frame; the prompt appears on the next walk, because the
    /// prompt is a child route and the walk is what finds it.
    Prompted(RouteId),
    /// A route claimed the key and did nothing visible with it.
    Consumed(RouteId),
    /// Every route on the path declined. The host may do what it likes —
    /// this is the only case in which anything above the router hears
    /// about Escape at all.
    Unclaimed,
}

/// One router, for one window.
///
/// `Cx` is the game's services; `A` is its action type. Neither is erased,
/// because all three consumers have exactly one session and one action
/// enum, and erasing them would buy nothing but a vtable.
pub struct Router<Cx, A> {
    table: RouteTable,
    routes: BTreeMap<RouteId, Box<dyn Route<Cx, A>>>,
    root: Box<dyn Fn(&Cx) -> RouteId>,
    path: Vec<RouteId>,
    /// Reported once per offending (parent, child) pair rather than every
    /// frame: a bad `resolve_child` would otherwise print sixty times a
    /// second and bury everything else.
    reported: BTreeMap<RouteId, RouteId>,
    /// Set when the last `resolve` changed the path, so a host can drive
    /// a transition without diffing the path itself.
    last_departure: Option<(RouteId, Departure)>,
}

impl<Cx, A> Router<Cx, A> {
    /// Build a router over a validated table.
    ///
    /// `root` is the game's lifecycle function: connected, seated,
    /// playing. About fifteen lines in the largest consumer, because
    /// everything below the first segment is `resolve_child`'s.
    pub fn new(
        table: RouteTable,
        routes: Vec<(RouteId, Box<dyn Route<Cx, A>>)>,
        root: impl Fn(&Cx) -> RouteId + 'static,
    ) -> Result<Self, TableError> {
        table.validate()?;
        let mut map: BTreeMap<RouteId, Box<dyn Route<Cx, A>>> = BTreeMap::new();
        for (id, route) in routes {
            if map.insert(id, route).is_some() {
                return Err(TableError::DuplicateId(id));
            }
        }
        // A registered id with no handler is the same class of mistake as
        // a registered id with no `screen` block, and is caught here for
        // the same reason: at startup, naming the id.
        for id in table.ids() {
            if !map.contains_key(id) {
                return Err(TableError::UnknownParent {
                    child: id,
                    parent: "<no handler registered>",
                });
            }
        }
        Ok(Self {
            table,
            routes: map,
            root: Box::new(root),
            path: Vec::new(),
            reported: BTreeMap::new(),
            last_departure: None,
        })
    }

    pub fn table(&self) -> &RouteTable {
        &self.table
    }

    /// The active path, outermost first.
    pub fn path(&self) -> &[RouteId] {
        &self.path
    }

    /// The deepest active route's id, if any.
    pub fn deepest(&self) -> Option<RouteId> {
        self.path.last().copied()
    }

    pub fn is_active(&self, id: RouteId) -> bool {
        self.path.contains(&id)
    }

    pub fn get(&self, id: RouteId) -> Option<&dyn Route<Cx, A>> {
        self.routes.get(id).map(|b| b.as_ref())
    }

    pub fn get_mut(&mut self, id: RouteId) -> Option<&mut (dyn Route<Cx, A> + 'static)> {
        self.routes.get_mut(id).map(|b| b.as_mut())
    }

    /// Recompute the path from session state, running `leave` for every id
    /// that dropped off it and `enter` for every id that joined.
    ///
    /// Call once per frame, before `update`. Returns `true` if the path
    /// changed.
    pub fn resolve(&mut self, cx: &mut Cx) -> bool {
        let next = self.walk(cx);
        if next == self.path {
            self.last_departure = None;
            return false;
        }

        // `leave` runs deepest-first, so a child releases before the
        // parent it was standing on. `enter` runs outermost-first for the
        // same reason in reverse.
        let departing: Vec<RouteId> = self
            .path
            .iter()
            .filter(|id| !next.contains(id))
            .copied()
            .collect();
        let arriving: Vec<RouteId> = next
            .iter()
            .filter(|id| !self.path.contains(id))
            .copied()
            .collect();

        let going_to = next.last().copied();
        if let Some(outgoing) = departing.last().copied() {
            if let Some(route) = self.routes.get_mut(outgoing) {
                let departure = route.depart(going_to);
                self.last_departure = Some((outgoing, departure));
            }
        } else {
            self.last_departure = None;
        }

        for id in departing.into_iter().rev() {
            if let Some(route) = self.routes.get_mut(id) {
                route.leave(cx);
            }
        }
        self.path = next;
        for id in arriving {
            if let Some(route) = self.routes.get_mut(id) {
                route.enter(cx);
            }
        }
        true
    }

    /// The departure the last path change asked for, if it asked for one.
    pub fn last_departure(&self) -> Option<(RouteId, Departure)> {
        self.last_departure
    }

    /// Derive the path without touching lifecycle. Split out so `resolve`
    /// can diff against the current path before running anything.
    fn walk(&mut self, cx: &Cx) -> Vec<RouteId> {
        let mut path = Vec::new();
        let mut id = (self.root)(cx);
        loop {
            if !self.table.contains(id) {
                self.report_once(id, "<root>");
                break;
            }
            if path.contains(&id) {
                // A `resolve_child` pointing back up the path. The table
                // is acyclic, so this is a handler bug rather than a
                // table one; stopping here keeps the frame renderable.
                self.report_once(id, "<already on the path>");
                break;
            }
            path.push(id);
            let Some(route) = self.routes.get(id) else {
                break;
            };
            let Some(child) = route.resolve_child(cx) else {
                break;
            };
            if !self.table.is_child_of(child, id) {
                self.report_once(child, id);
                break;
            }
            id = child;
        }
        path
    }

    fn report_once(&mut self, child: RouteId, parent: RouteId) {
        if self.reported.insert(child, parent).is_none() {
            eprintln!(
                "route: `{parent}` resolved to `{child}`, which is not a registered child of it; \
                 rendering as if it resolved to nothing"
            );
        }
    }

    /// The ids that actually render this frame, outermost first, after
    /// occlusion.
    ///
    /// Everything from the deepest [`Occlusion::Surface`] route down is
    /// dropped; everything above the deepest [`Occlusion::View`] route
    /// keeps its `draw` but loses its screen. See [`visible_views`] for
    /// the second half.
    pub fn drawing(&self) -> Vec<RouteId> {
        let cut = self.deepest_at_least(Occlusion::Surface);
        match cut {
            Some(i) => self.path[i..].to_vec(),
            None => self.path.clone(),
        }
    }

    /// The ids whose *screens* render this frame, outermost first.
    ///
    /// A route declaring [`Occlusion::None`] leaves its ancestors' screens
    /// up — which is what the exit prompts want and cannot express today,
    /// and is the seventh symptom bug in `ROUTING.md` §2.4.
    pub fn visible_views(&self) -> Vec<RouteId> {
        let cut = self.deepest_at_least(Occlusion::View);
        match cut {
            Some(i) => self.path[i..].to_vec(),
            None => self.path.clone(),
        }
    }

    /// Index of the deepest route on the path whose occlusion is at least
    /// `level`. Everything before it is hidden at that level.
    fn deepest_at_least(&self, level: Occlusion) -> Option<usize> {
        self.path
            .iter()
            .enumerate()
            .rev()
            .find(|(_, id)| {
                self.routes
                    .get(*id)
                    .map(|r| r.occludes() >= level)
                    .unwrap_or(false)
            })
            .map(|(i, _)| i)
    }

    /// Tick every active route, outermost first.
    pub fn update(&mut self, cx: &Cx, out: &mut Outbox<A>, dt: f32) {
        for id in self.path.clone() {
            if let Some(route) = self.routes.get_mut(id) {
                route.update(cx, out, dt);
            }
        }
    }

    /// Offer an event to the deepest active route first, stopping at the
    /// first that handles it (axiom 6).
    ///
    /// One rule, replacing five independently hand-ordered callbacks in
    /// one game and `app::routing::PointerRouter`'s two-layer special
    /// case, which is this rule at N=2.
    pub fn event(&mut self, cx: &Cx, out: &mut Outbox<A>, ev: &RouteEvent) -> Handled {
        let path = self.path.clone();
        for (depth, id) in path.iter().copied().enumerate().rev() {
            let Some(route) = self.routes.get_mut(id) else {
                continue;
            };
            let handled = route.event(cx, out, ev).is_handled();
            // Asked *after* every dispatch, handled or not: a route may
            // decide to leave on an event it also lets fall through.
            if route.take_leave_request() {
                self.pop_at(depth);
            }
            if handled {
                return Handled::Yes;
            }
        }
        Handled::No
    }

    /// Tell whoever is claiming the route at `depth` to stop.
    ///
    /// The path is not edited here — the next `resolve` shortens it,
    /// which is what keeps "derived, never pushed" true for the two
    /// gestures whose whole purpose is to change the path.
    fn pop_at(&mut self, depth: usize) {
        if depth == 0 {
            // A root route asking to leave has nobody to ask. The
            // lifecycle is what put it there, and only the lifecycle can
            // take it away.
            return;
        }
        let (child, parent) = (self.path[depth], self.path[depth - 1]);
        if let Some(parent) = self.routes.get_mut(parent) {
            parent.child_popped(child);
        }
    }

    /// Offer Escape to the deepest active route, then up the path.
    ///
    /// No layer above intercepts, which is the whole of axiom 9. A `Pop`
    /// does not edit the path directly: it asks the route to stop
    /// claiming itself, and the next `resolve` is what shortens the path.
    /// That keeps "the path is derived, never pushed" true even for the
    /// one gesture whose whole purpose is to change it.
    pub fn escape(&mut self, cx: &Cx, out: &mut Outbox<A>) -> EscapeOutcome {
        let path = self.path.clone();
        for (depth, id) in path.iter().copied().enumerate().rev() {
            let Some(route) = self.routes.get_mut(id) else {
                continue;
            };
            match route.escape(cx, out) {
                Escape::Pop => {
                    // Tell whoever was claiming this id to stop. Without
                    // this the next walk re-derives the same path and the
                    // pop does nothing — the child cannot shorten a path
                    // it does not derive.
                    self.pop_at(depth);
                    return EscapeOutcome::Popped(id);
                }
                Escape::Prompt => return EscapeOutcome::Prompted(id),
                Escape::Ignore => return EscapeOutcome::Consumed(id),
                Escape::Fall => continue,
            }
        }
        EscapeOutcome::Unclaimed
    }

    /// Draw every route occlusion allows, shallowest first, so a deeper
    /// route paints over a shallower one.
    pub fn draw(&mut self, cx: &mut Cx, surface: &mut skia_safe::Surface, w: f32, h: f32) {
        for id in self.drawing() {
            if let Some(route) = self.routes.get_mut(id) {
                route.draw(cx, surface, w, h);
            }
        }
    }
}
