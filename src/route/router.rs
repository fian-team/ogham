//! The router's surface-side face: the walk lives in `structure`, the
//! two surface-typed dispatches live here.
//!
//! [`structure::Router`] owns the path, the walk, lifecycle, occlusion,
//! escape and the leave protocol — everything statable without naming a
//! surface type. This module is scaffolding that keeps
//! `ogham::route::Router`'s API exactly what it was before the split
//! (`APPLICATION_BUILD.md` WP-1.1): it bridges the surface-coupled
//! [`Route`] trait onto [`structure::Node`], and keeps the two
//! dispatches whose signatures name surface types — [`event`]
//! (a [`RouteEvent`] carries a widget event) and [`draw`] (a skia
//! surface). Both move up into the driver in P4 (WP-4.1/WP-4.2), and
//! this file deletes with them.
//!
//! [`event`]: Router::event
//! [`draw`]: Router::draw

use crate::route::{Departure, Escape, Handled, Occlusion, Outbox, Route, RouteEvent, RouteId};
use structure::table::{RouteTable, TableError};

pub use structure::router::EscapeOutcome;

/// [`Route`], seen from the walk.
///
/// The bridge that lets `structure`'s router drive surface-coupled
/// routes without naming their types: every [`structure::Node`] method
/// is a [`Route`] method whose signature already carried no surface
/// type. On `dyn Route` rather than blanket-on-`T` because the orphan
/// rule allows exactly this, and because the router only ever holds the
/// boxed object anyway. Deletes in P4 with the wrapper around it.
impl<Cx, A> structure::Node<Cx, A> for dyn Route<Cx, A> + 'static {
    fn resolve_child(&self, cx: &Cx) -> Option<RouteId> {
        Route::resolve_child(self, cx)
    }

    fn occludes(&self) -> Occlusion {
        Route::occludes(self)
    }

    fn update(&mut self, cx: &Cx, out: &mut Outbox<A>, dt: f32) {
        Route::update(self, cx, out, dt)
    }

    fn escape(&mut self, cx: &Cx, out: &mut Outbox<A>) -> Escape {
        Route::escape(self, cx, out)
    }

    fn take_leave_request(&mut self) -> bool {
        Route::take_leave_request(self)
    }

    fn child_popped(&mut self, child: RouteId) {
        Route::child_popped(self, child)
    }

    fn enter(&mut self, cx: &mut Cx) {
        Route::enter(self, cx)
    }

    fn leave(&mut self, cx: &mut Cx) {
        Route::leave(self, cx)
    }

    fn depart(&mut self, to: Option<RouteId>) -> Departure {
        Route::depart(self, to)
    }
}

/// One router, for one window.
///
/// `Cx` is the game's services; `A` is its action type. Neither is erased,
/// because all three consumers have exactly one session and one action
/// enum, and erasing them would buy nothing but a vtable.
///
/// A thin wrapper over [`structure::Router`] since WP-1.1: everything but
/// [`event`](Self::event) and [`draw`](Self::draw) delegates.
pub struct Router<Cx, A> {
    core: structure::Router<Cx, A, dyn Route<Cx, A>>,
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
        Ok(Self {
            core: structure::Router::new(table, routes, root)?,
        })
    }

    pub fn table(&self) -> &RouteTable {
        self.core.table()
    }

    /// The application's facts (`docs/internal/APPLICATION.md` §5). What a
    /// consumer reads and subscribes through.
    pub fn store(&self) -> &structure::Store {
        self.core.store()
    }

    /// The store, to provide scopes and claim producer fields at startup,
    /// and to tick it — which is the frame barrier.
    pub fn store_mut(&mut self) -> &mut structure::Store {
        self.core.store_mut()
    }

    /// Ask whether a node's door would open, without going there
    /// (`APPLICATION.md` §3.4). The same evaluation the walk runs, offered
    /// to the panel row that grays itself.
    pub fn ask(&self, id: RouteId) -> Result<(), structure::Refusal> {
        self.core.ask(id)
    }

    /// The door the last walk was refused at, if it was refused at one.
    pub fn refused(&self) -> Option<(RouteId, &structure::Refusal)> {
        self.core.refused()
    }

    /// The active path, outermost first.
    pub fn path(&self) -> &[RouteId] {
        self.core.path()
    }

    /// The deepest active route's id, if any.
    pub fn deepest(&self) -> Option<RouteId> {
        self.core.deepest()
    }

    pub fn is_active(&self, id: RouteId) -> bool {
        self.core.is_active(id)
    }

    pub fn get(&self, id: RouteId) -> Option<&dyn Route<Cx, A>> {
        self.core.get(id)
    }

    pub fn get_mut(&mut self, id: RouteId) -> Option<&mut (dyn Route<Cx, A> + 'static)> {
        self.core.get_mut(id)
    }

    /// Recompute the path from session state, running `leave` for every id
    /// that dropped off it and `enter` for every id that joined.
    ///
    /// Call once per frame, before `update`. Returns `true` if the path
    /// changed.
    pub fn resolve(&mut self, cx: &mut Cx) -> bool {
        self.core.resolve(cx)
    }

    /// The departure the last path change asked for, if it asked for one.
    pub fn last_departure(&self) -> Option<(RouteId, Departure)> {
        self.core.last_departure()
    }

    /// The ids that actually render this frame, outermost first, after
    /// occlusion. See [`structure::Router::drawing`].
    pub fn drawing(&self) -> Vec<RouteId> {
        self.core.drawing()
    }

    /// The ids whose *screens* render this frame, outermost first. See
    /// [`structure::Router::visible_views`].
    pub fn visible_views(&self) -> Vec<RouteId> {
        self.core.visible_views()
    }

    /// Tick every active route, outermost first.
    pub fn update(&mut self, cx: &Cx, out: &mut Outbox<A>, dt: f32) {
        self.core.update(cx, out, dt)
    }

    /// Offer an event to the deepest active route first, stopping at the
    /// first that handles it (axiom 6).
    ///
    /// One rule, replacing five independently hand-ordered callbacks in
    /// one game and `app::routing::PointerRouter`'s two-layer special
    /// case, which is this rule at N=2.
    ///
    /// Here rather than in `structure` because a [`RouteEvent`] carries a
    /// widget event; the arbitration itself is one loop over the walk's
    /// primitives.
    pub fn event(&mut self, cx: &Cx, out: &mut Outbox<A>, ev: &RouteEvent) -> Handled {
        let path = self.core.path().to_vec();
        for (depth, id) in path.iter().copied().enumerate().rev() {
            let Some(route) = self.core.get_mut(id) else {
                continue;
            };
            let handled = route.event(cx, out, ev).is_handled();
            // Asked *after* every dispatch, handled or not: a route may
            // decide to leave on an event it also lets fall through.
            if Route::take_leave_request(route) {
                self.core.pop_at(depth);
            }
            if handled {
                return Handled::Yes;
            }
        }
        Handled::No
    }

    /// Offer Escape to the deepest active route, then up the path. See
    /// [`structure::Router::escape`].
    pub fn escape(&mut self, cx: &Cx, out: &mut Outbox<A>) -> EscapeOutcome {
        self.core.escape(cx, out)
    }

    /// Draw every route occlusion allows, shallowest first, so a deeper
    /// route paints over a shallower one.
    pub fn draw(&mut self, cx: &mut Cx, surface: &mut skia_safe::Surface, w: f32, h: f32) {
        for id in self.core.drawing() {
            if let Some(route) = self.core.get_mut(id) {
                route.draw(cx, surface, w, h);
            }
        }
    }
}
