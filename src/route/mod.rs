//! One router, one path, one arbitration rule.
//!
//! `lorekeeper/docs/ROUTING.md` is the record and its axioms are locked.
//! This module is the route tier: the table, the walk, depth arbitration,
//! occlusion, escape, departure, and the one generic [`Chrome`] that
//! projects a route's state into its `screen` block.
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
pub mod outbox;
pub mod router;
pub mod table;

use std::collections::HashMap;

use crate::runtime::value::Value;

pub use chrome::Chrome;
pub use outbox::Outbox;
pub use router::Router;
pub use table::{RouteTable, TableError};

/// A route's id: the name a table registers and a `screen` block declares.
///
/// A `&'static str` rather than a `String` because the table is static and
/// built once at startup (axiom 10) — and because comparing ids is on the
/// per-frame path, where the walk asks every node on the path for its
/// child.
pub type RouteId = &'static str;

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

/// One argument of a named raise.
///
/// Not `crate::Value`, and the reason is a real constraint rather than
/// taste: an event handler must be `Send + Sync`, and a `Value` may hold
/// an `Rc<VMClosure>`, so it cannot cross the handler boundary at all.
/// What can cross is data — which is also the only thing a host has any
/// business acting on.
#[derive(Clone, Debug, PartialEq)]
pub enum RaiseArg {
    Str(String),
    Int(i32),
    Float(f64),
    Bool(bool),
    /// A closure, a widget, or void — nothing a host can act on.
    Opaque,
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

impl RaiseArg {
    pub fn as_str(&self) -> Option<&str> {
        match self {
            RaiseArg::Str(s) => Some(s.as_str()),
            _ => None,
        }
    }

    pub fn as_f32(&self) -> Option<f32> {
        match self {
            RaiseArg::Float(f) => Some(*f as f32),
            RaiseArg::Int(i) => Some(*i as f32),
            _ => None,
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

/// Whether a route consumed an event.
///
/// Depth arbitration offers an event to the deepest active route first and
/// stops at the first `Handled::Yes` (axiom 6). One rule, replacing five
/// hand-ordered callbacks in one game and a two-layer special case in
/// `app`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Handled {
    Yes,
    No,
}

impl Handled {
    pub fn is_handled(self) -> bool {
        matches!(self, Handled::Yes)
    }

    /// `true` → `Yes`. For routes whose inner call already returns a bool.
    pub fn from_bool(consumed: bool) -> Self {
        match consumed {
            true => Handled::Yes,
            false => Handled::No,
        }
    }
}

/// What a route hides beneath it.
///
/// Ordered, and `Surface` implies `View`. That it is one ladder rather than
/// two independent bits is an *empirical* claim about the three consumers,
/// not a simplification: nothing in any of them wants an ancestor's 3D
/// hidden while its HUD still draws. `ROUTING.md` §7.2 is the record, and
/// this is the line to attack first if that turns out wrong.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Occlusion {
    /// Ancestors draw and their screens render behind this one. What the
    /// exit prompts want and cannot express today: a scrim and a card
    /// authored to sit *over* the workspace it is leaving.
    None,
    /// Ancestors' `draw` runs; their screens do not. Every ordinary child
    /// — the journal, the wardrobe, the sea and sky stances, pause — and
    /// the default, because it is what a `mode` swap does today.
    View,
    /// Neither. A route that brings its own world: the map editor, the
    /// backstory workspace, the library.
    Surface,
}

impl Default for Occlusion {
    fn default() -> Self {
        Occlusion::View
    }
}

/// What Escape means here (axiom 9).
///
/// The deepest active route decides, and no layer above it intercepts.
/// Today the shell claims Escape unconditionally while `InGame`, which is
/// why two games' Escape handlers are dead code that reads as correct.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Escape {
    /// Leave this route; the path shortens by one. The ordinary case.
    Pop,
    /// This route has unsaved work and raises its own prompt instead. The
    /// prompt is a child route; the router does not pop.
    Prompt,
    /// Escape means nothing here and must not reach anyone else — a text
    /// field clearing itself, a stance that owns the key.
    Ignore,
    /// Not this route's key. Offer it to the parent.
    Fall,
}

/// How the outgoing route leaves the frame.
///
/// Two of the three consumers already implement cross-screen transitions
/// by re-rendering the *outgoing* surface offscreen and bleeding it over
/// the incoming one. A router that swaps the top of the stack and drops
/// the old route breaks both, so departure is part of the contract rather
/// than something a game bolts on beside it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Departure {
    /// Gone this frame. The default, and it costs nothing.
    Cut,
    /// Hold the outgoing route's last frame for `seconds`, composited
    /// under the incoming one. The router keeps the frame; what is done
    /// with it is the host's (a page turn, a cross-fade, a bloom).
    Bleed { seconds: f32 },
}

impl Default for Departure {
    fn default() -> Self {
        Departure::Cut
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
pub trait Route<Cx, A> {
    /// Emit this route's own state slice through `editable`'s read walk.
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

    /// What this route hides beneath it. See [`Occlusion`].
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
    fn child_popped(&mut self, _child: RouteId) {}

    /// 3D, Skia, anything. Runs every frame that occlusion allows, with
    /// no injection — the principled home for a per-frame canvas paint.
    ///
    /// Exclusive and sequential (one route draws at a time, shallowest
    /// first), so this is the one method that takes the services mutably.
    fn draw(
        &mut self,
        _cx: &mut Cx,
        _surface: &mut skia_safe::Surface,
        _width: f32,
        _height: f32,
    ) {
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
    /// Several instances may be mounted at once; depth decides draw order
    /// and input (axiom 8). *Which* instance is mounted is never a
    /// question a game answers, which is what `window_surface` was.
    fn own_ui(&mut self) -> Option<&mut crate::Ogham> {
        None
    }

    /// A name for diagnostics. Defaults to the id the table registered.
    fn debug_name(&self) -> Option<&'static str> {
        None
    }
}
