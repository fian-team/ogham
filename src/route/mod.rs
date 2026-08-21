//! One router, one path, one arbitration rule.
//!
//! `lorekeeper/docs/ROUTING.md` is the record and its axioms are locked.
//! This module is the route tier's *surface-side face*: since
//! `APPLICATION_BUILD.md` WP-1.1 the table, the walk, the outbox and the
//! route vocabulary live in the `structure` crate (the structure
//! framework, `APPLICATION.md` §2), and this module re-exports them at
//! their old paths so no consumer changes until the migrations do.
//!
//! # What is still here, and why
//!
//! Everything whose signature names a surface type. WP-4.1 built the
//! driver (`lorekeeper/driver`) and moved every *mechanism* out of this
//! module into it: the binding owns the store, mounts one instance per
//! instance root from the table's document names, dispatches events and
//! draws, and runs the crossing. What is left here is the **surface**
//! three games are written against, and it stays until their migrations
//! (P6) rewrite their `impl Route` blocks — which is the first moment it
//! can go, because there is no way to re-export a driver type from the
//! crate the driver depends on.
//!
//! - the [`Route`] trait — `read_state` is superseded by a document's
//!   selection against the store (§4.1); `draw` and `occludes` by the
//!   binding's draw slot and the table's declared occlusion (§6.2);
//!   `own_ui` and `brings_own_document` by the table's
//!   `mounts(id, document)` (§6.1);
//! - [`RouteEvent`] — its `Input` half carries a widget event, and the
//!   dispatch that consumes it is the driver's now;
//! - [`Chrome`] — one mounted document, which the binding creates and
//!   owns; a consumer stops constructing one when the table names its
//!   documents;
//! - the [`Router`] wrapper in [`router`] — a store of its own for a walk
//!   driven with no binding, which is what three games' route tests do.
//!
//! `cargo tree -p structure` showing no ogham is the §2 dependency edge
//! that holds and is checked at every phase gate. The other direction —
//! `cargo tree -p ogham` showing no structure — is held by **this module
//! alone** now that the contract has left for a crate of its own, and it
//! comes true when the three games' `impl Route` blocks are rewritten in
//! P6. See the dependency's own comment in `Cargo.toml`.
//!
//! # Why this is in the language
//!
//! Because the other half of it already was. `screen "<id>" { … }`,
//! `outlet()` and the scoped `"<id>::<field>"` keys are constructs of the
//! document; what was missing was anything that could *decide* a path, so
//! every host reimplemented the interesting half. A UI framework that can
//! render a route but not choose one has committed to the concept and
//! then declined to finish it.
//!
//! Nothing here is game-shaped. `Cx` and `A` are type parameters — the
//! host's context and its action type — and the only concrete types in
//! the whole module are ogham's own and a skia `Surface` to draw onto.
//! What *is* game-shaped is a title screen, a pause overlay and a save
//! browser, and those stay in the engine (`lorekeeper/front`).
//!
//! # Derived, not pushed
//!
//! There is no `push`/`pop` navigation stack. The path is re-derived
//! every frame by asking each node on it for its one child
//! ([`Route::resolve_child`]), which makes "two screens are up at once"
//! unrepresentable rather than merely discouraged, and makes a stale
//! screen after a state change impossible rather than a bug to remember.
//!
//! # What a router is not allowed to know
//!
//! Nothing about a session. The connection, the snapshot pump, the hosted
//! server and the autosave timer tick regardless of what is on screen
//! (axiom 7), and the way that stays true is that this crate never names
//! them: a game's services arrive as an opaque `Cx` type parameter, and a
//! route's only way to *change* them is to push an action onto an
//! [`Outbox`] that the host drains. `update`, `event` and `escape` take
//! `&Cx`, so "a route mutated the session" is not a bug to catch — it does
//! not typecheck.
//!
//! # The four things this replaces
//!
//! `shell::Screen`, `editor_host`'s own screen enum, each game's private
//! stance booleans, and the `mode: String` in host state whose only reader
//! in all three games was a root `match`. Five vocabularies, one path.
//!
//! # Shape
//!
//! ```text
//! RouteTable   ids and edges — static, built once, validated at startup
//! Router       the active path, the walk, lifecycle, arbitration
//! Route        what a game implements, once per surface
//! Chrome       one Ogham instance + the path and per-route projection
//! ```

pub mod chrome;
pub mod router;

// The moved halves, at their old paths. Scaffolding: deletes in P6, with
// the consumers' `impl Route` blocks.
pub use structure::{guard, outbox, table};

use std::collections::HashMap;

use crate::runtime::value::Value;

pub use chrome::Chrome;
pub use outbox::Outbox;
pub use router::Router;
pub use structure::{
    Area, Departure, Escape, Guard, Handled, Occlusion, RaiseArg, Refusal, RouteId, Store,
};
pub use table::{RouteTable, TableError, Tier};

/// What a route is offered.
///
/// Two streams, because a mounted document produces two genuinely different
/// things and collapsing them loses the distinction that matters:
///
/// - [`Input`](RouteEvent::Input) is a pointer or keyboard event from the
///   platform. This is the one depth arbitration is *for*: whichever route
///   is deepest gets first refusal, and a press it claims is its whole
///   gesture.
/// - [`Ui`](RouteEvent::Ui) is a named raise from the document —
///   `event("menu", "new")` in a `.ogh`. It is already addressed: the
///   screen that raised it is a route's screen. It still runs deepest-first
///   so that a prompt over a workspace answers before the workspace does.
///
/// All three games currently funnel raises through a private `mpsc` and
/// drain it somewhere else in the frame, which is how one game ended up
/// with two drains on one channel each dropping what the other wanted.
pub enum RouteEvent<'a> {
    Input(&'a crate::widget::event::Event),
    Ui { name: &'a str, args: &'a [RaiseArg] },
}

impl From<&crate::runtime::value::Value> for RaiseArg {
    fn from(v: &crate::runtime::value::Value) -> Self {
        use crate::runtime::value::Value;
        match v {
            Value::String(s) => RaiseArg::Str(s.clone()),
            Value::Integer(i) => RaiseArg::Int(*i),
            Value::Float(f) => RaiseArg::Float(*f),
            Value::Boolean(b) => RaiseArg::Bool(*b),
            _ => RaiseArg::Opaque,
        }
    }
}

/// Turn a handler's `&[Value]` into something that can leave it.
pub fn raise_args(values: &[crate::runtime::value::Value]) -> Vec<RaiseArg> {
    values.iter().map(RaiseArg::from).collect()
}

impl<'a> RouteEvent<'a> {
    /// The raise's name, if this is one. `None` for platform input.
    pub fn ui_name(&self) -> Option<&str> {
        match self {
            RouteEvent::Ui { name, .. } => Some(name),
            RouteEvent::Input(_) => None,
        }
    }

    /// A named raise whose first argument is a string — by far the
    /// commonest shape (`menu("new")`, `save("quick")`, `edit_exit("save")`).
    pub fn ui_str(&self, want: &str) -> Option<&str> {
        match self {
            RouteEvent::Ui { name, args } if *name == want => {
                args.first().and_then(RaiseArg::as_str)
            }
            _ => None,
        }
    }

    /// Every argument of a named raise, for the rare call that takes more
    /// than one.
    pub fn ui_args(&self, want: &str) -> Option<&[RaiseArg]> {
        match self {
            RouteEvent::Ui { name, args } if *name == want => Some(args),
            _ => None,
        }
    }

    /// A named raise with no arguments.
    pub fn is_ui(&self, want: &str) -> bool {
        matches!(self, RouteEvent::Ui { name, .. } if *name == want)
    }
}

/// A route, as [`Any`](std::any::Any) — the one method that cannot be
/// written as a default, so it is written once, here, for every type at
/// once.
///
/// [`Route`]'s supertrait rather than `Any` itself, because `dyn
/// Route<Cx, A>: Any` would make `Cx` and `A` part of a `'static`
/// requirement on every router, binding and host in five repositories, to
/// buy one downcast.
pub trait AsAny {
    fn as_any(&self) -> &dyn std::any::Any;
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any;
}

impl<T: std::any::Any> AsAny for T {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

/// One routable surface.
///
/// Implemented once per surface by a game (or by the services tier, for
/// the six front-of-game routes every game gets). `Cx` is the game's
/// services — one concrete struct, not erased, because all three consumers
/// have exactly one session. `A` is the game's action type, the only way a
/// route reaches back.
///
/// Every method has a default except [`event`](Route::event) and
/// [`escape`](Route::escape), because a surface that answers neither is
/// decoration rather than a route (axiom 4).
///
/// The walk never sees this trait: `structure`'s router drives it through
/// the [`structure::Node`] bridge in [`router`], which carries exactly
/// the methods below whose signatures name no surface type.
///
/// # Why [`Any`](std::any::Any) is a supertrait
///
/// So a host can get a route back **at the type that registered it**.
/// The walk hands out `&dyn Route`, and a producer that needs a route's
/// own state — the half-typed address on the connect screen, which is
/// route state by `ROUTING.md` §7.1 and by §5's own scope lifetime — had
/// no way to reach it except through [`read_state`](Route::read_state)'s
/// map of `Value`s, which is on the deletion ledger. One consumer read it
/// back through that shim in fourteen labelled lines; the other did not,
/// and its reply card showed a placeholder forever.
///
/// The alternative was to make the address a store field, and it does not
/// fit: a route may write only through the outbox (§3.3) and a producer
/// only inside the barrier (§5.4), so a keystroke that *appends* to a
/// buffer has no single place to live. It is route state; what was
/// missing was a typed way to read it.
pub trait Route<Cx, A>: AsAny {
    /// Emit this route's own state slice through `editable`'s read walk.
    ///
    /// **Superseded** by the store (§4.1): a scope's provider owns the
    /// schema and a document's `screen` block selects from it, so nothing
    /// transcribes a struct into a map of names any more.
    ///
    /// The [`Chrome`] turns it into the scoped host state the route's
    /// `screen` block reads. A route with no UI of its own leaves this a
    /// no-op — which is the honest thing for `/table/seating`, a route
    /// that exists to be a place on the path.
    ///
    /// Returns ogham `Value`s directly, because that is what the runtime
    /// takes (`set_screen_state` accepts `impl IntoHostValue`). It used to
    /// hand back an `editable` walk, which made the router depend on the
    /// engine's reflection crate for a convenience — deriving the map
    /// rather than writing it. That convenience still exists and is still
    /// one line at the call site; it just lives in `editable-ogham` now,
    /// on the engine's side of the boundary, where a crate that knows
    /// about both belongs.
    ///
    /// `None` for a route with no UI of its own — which is the honest
    /// answer for `/table/seating`, a route that exists to be a place on
    /// the path.
    fn read_state(&self) -> Option<HashMap<String, Value>> {
        None
    }

    /// Which of this route's children, if any, is active.
    ///
    /// Called every frame while this route is on the path: this is
    /// `resolve` at one node (axiom 3, and `ROUTING.md` §13.3). At most
    /// one child, structurally — which is why the walk needs no arbiter
    /// and why five independent stance booleans collapse into one
    /// `Option`.
    ///
    /// **Whatever field this reads must be cleared in
    /// [`leave`](Route::leave)** — not in `enter`, which is a frame too
    /// late: the walk that builds the path runs *before* lifecycle, so a
    /// claim cleared on the way in has already been read once. The symptom
    /// is a pause overlay that is already up the next time you host a
    /// lobby, because nothing between the two ever reset it.
    ///
    /// `route/tests/router.rs` guards both halves — one route that clears
    /// and one that does not, so the guard is known to be testing
    /// something.
    ///
    /// Returning an id that is not a registered child of this route is a
    /// programming error; the router reports it once and renders as if
    /// `None`, because a blank surface is recoverable mid-frame and a
    /// panic is not.
    fn resolve_child(&self, _cx: &Cx) -> Option<RouteId> {
        None
    }

    /// The same question, asked over the **facts** the store holds
    /// (`APPLICATION.md` §5).
    ///
    /// A claim that is somebody else's to make is a field of this node's
    /// scope, not a getter on the host's context: a game whose own pane
    /// hangs under the engine's title writes the claim into the title's
    /// scope, and the title reads it here. Defaulted to
    /// [`resolve_child`](Route::resolve_child), so a route that reads no
    /// facts writes nothing and four repositories' `impl Route` blocks are
    /// untouched.
    ///
    /// What arrives is **committed** state: the walk runs outside the
    /// frame barrier (§5.4), so a node reads last tick's commit whole and
    /// never a mixture.
    fn resolve_child_in(&self, cx: &Cx, _store: &structure::Store) -> Option<RouteId> {
        self.resolve_child(cx)
    }

    /// What this route hides beneath it. See [`Occlusion`].
    ///
    /// **Superseded** by `RouteTable::occludes(id, …)`: occlusion is node
    /// data (§6.2), the walk reads the table first, and a table that
    /// declares it never calls this. The method survives for a consumer
    /// whose table does not declare yet.
    fn occludes(&self) -> Occlusion {
        Occlusion::View
    }

    /// Ticked while on the path, deepest last. The session ticks
    /// elsewhere (axiom 7), which is why `cx` is shared.
    ///
    /// **No input.** A route that read the raw device would be arbitrating
    /// input twice — once here and once through `event`, which is where
    /// depth arbitration actually lives. The trait carried an
    /// `&input::Input` for its first week and not one implementation in
    /// four games ever read it; a host that genuinely needs the device
    /// puts it on `Cx`, where the rest of its per-frame state already is.
    fn update(&mut self, _cx: &Cx, _out: &mut Outbox<A>, _dt: f32) {}

    /// An event offered to this route. Arbitration runs deepest-first and
    /// stops at the first [`Handled::Yes`].
    fn event(&mut self, cx: &Cx, out: &mut Outbox<A>, ev: &RouteEvent) -> Handled;

    /// Escape, offered to the deepest active route first and then up.
    fn escape(&mut self, cx: &Cx, out: &mut Outbox<A>) -> Escape;

    /// This route asked to leave in response to something that was not
    /// Escape — a Back or a Resume button. Read *and cleared* by the
    /// router after every event dispatch.
    ///
    /// A route cannot pop itself: its presence on the path is its
    /// parent's claim, so leaving is always something the parent does.
    /// `Escape::Pop` is the same request arriving down the Escape path;
    /// this is the one that arrives from a click. Without it a Resume
    /// button sets a field on the wrong route and nothing happens, which
    /// is exactly what it did.
    fn take_leave_request(&mut self) -> bool {
        false
    }

    /// A child of this route popped, and this route is what was claiming
    /// it. Stop.
    ///
    /// This is the other half of [`Escape::Pop`], and without it `Pop`
    /// does nothing: a child asking to leave cannot shorten the path,
    /// because the path is derived and the *parent* is what derives it.
    /// The parent clears its own claim here and the next walk is a segment
    /// shorter — so "derived, never pushed" holds even for the one gesture
    /// whose entire purpose is to change the path.
    ///
    /// A route with a single `Option<RouteId>` claim ignores the argument
    /// and clears it; one that can claim several children distinguishes.
    ///
    /// **The outbox is the other half.** A claim is not always a field of
    /// the route that holds it: since the store, a claim may be a fact of
    /// a *scope*, whose lifetime is the node's presence on the path
    /// (`APPLICATION.md` §5) — and this callback could not reach one,
    /// because it was the only route method handed nothing to ask with. A
    /// route pushes an action here exactly as it does from
    /// [`escape`](Route::escape); the producer that owns the fact clears
    /// it inside the same frame's barrier, and the next walk is a segment
    /// shorter.
    fn child_popped(&mut self, _out: &mut Outbox<A>, _child: RouteId) {}

    /// 3D, Skia, anything. Runs every frame that occlusion allows, with
    /// no injection — the principled home for a per-frame canvas paint.
    ///
    /// Exclusive and sequential (one route draws at a time, shallowest
    /// first), so this is the one method that takes the services mutably.
    ///
    /// **Superseded** by the binding's draw slot (§6.2): what paints under
    /// an instance's document is declared once, at the mount, and is
    /// handed the store's *read* verb rather than whatever the route could
    /// reach. This method has no damage flag, which is why a consumer that
    /// used it painted only because the old render pass called
    /// unconditionally.
    fn draw(&mut self, _cx: &mut Cx, _surface: &mut skia_safe::Surface, _width: f32, _height: f32) {
    }

    /// The id entered the path. Not "became deepest" — a prompt pushed
    /// above the map editor must not disturb the transaction it holds
    /// (axiom 10).
    fn enter(&mut self, _cx: &mut Cx) {}

    /// The id left the path. Route-owned state dies here, which is the
    /// test for what is route state at all: anything that must outlive
    /// the route it is edited in belongs to `Cx` (`ROUTING.md` §7.1).
    fn leave(&mut self, _cx: &mut Cx) {}

    /// How this route leaves the frame when the path moves to `to`.
    fn depart(&mut self, _to: Option<RouteId>) -> Departure {
        Departure::Cut
    }

    /// A route that brings its own ogham instance — an editor from
    /// another crate. `None` means it projects into the shared chrome.
    ///
    /// **Superseded** by `RouteTable::mounts(id, document)` (§6.1): the
    /// binding mounts what the table names, so a route holding an
    /// instance — and a host holding one beside it — is the drift the
    /// declaration replaces.
    ///
    /// Several instances may be mounted at once; depth decides draw order
    /// and input (axiom 8). *Which* instance is mounted is never a
    /// question a game answers, which is what `window_surface` was.
    fn own_ui(&mut self) -> Option<&mut crate::Ogham> {
        None
    }

    /// This route's screen lives in a document of its own, so the shared
    /// chrome declares no `screen` block for its id.
    ///
    /// **Superseded** by the same declaration: an instance root's document
    /// is table data, so which ids a given document must declare is
    /// derived rather than answered.
    ///
    /// Read only by the startup check ([`Chrome::validate`]), which would
    /// otherwise name every editor a game mounts from another crate as
    /// drift. Not derived from [`own_ui`](Route::own_ui): an editor
    /// mounted on `enter` has none yet at startup, which is exactly when
    /// the check runs.
    ///
    /// Declaring it wrong costs one thing and it is the right one — a
    /// missing `screen` block stops being reported for this id, so the
    /// route that lied is the route that loses the check.
    fn brings_own_document(&self) -> bool {
        false
    }

    /// A name for diagnostics. Defaults to the id the table registered.
    fn debug_name(&self) -> Option<&'static str> {
        None
    }
}
