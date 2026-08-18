//! The router, driven with no window.
//!
//! Every rule in `ROUTING.md` that can be stated without a GPU is stated
//! here, because the whole argument for the route tier is that "what is on
//! screen" stops being a thing you have to boot a game to find out.
//!
//! The fake game below is deliberately the awkward shape rather than the
//! easy one: two roots, a route reachable from two parents, a prompt that
//! sits *over* the workspace it is leaving, and a workspace that occludes
//! everything under it.

use std::cell::RefCell;
use std::rc::Rc;

use ogham::route::router::EscapeOutcome;
use ogham::route::table::TableError;
use ogham::route::{
    Departure, Escape, Handled, Occlusion, Outbox, Route, RouteEvent, RouteId, RouteTable, Router,
};

// ── the fake game ──────────────────────────────────────────────────────

/// What the routes read. Stands in for a game's services.
#[derive(Default)]
struct Cx {
    /// Which root the lifecycle is in: "title" or "world".
    lifecycle: &'static str,
    /// `/world`'s one authored input — the whole of what five stance
    /// booleans collapse into (`ROUTING.md` §13.3).
    stance: Option<RouteId>,
    /// The map editor's own "am I leaving?", which is why its exit prompt
    /// is resolved by the editor and not by the game.
    editing_exit: bool,
    /// Set by a route's `resolve_child` to something that is not its
    /// child, to prove the router refuses it.
    misroute: bool,
}

#[derive(Debug, PartialEq)]
enum Act {
    Saved,
    Left,
}

/// Every lifecycle call the routes made, in order. Shared so a test can
/// assert `leave` ran deepest-first and `enter` outermost-first.
type Log = Rc<RefCell<Vec<String>>>;

struct Fake {
    id: RouteId,
    log: Log,
    occludes: Occlusion,
    /// What `event` reports. The arbitration tests set exactly one route
    /// to claim.
    claims_events: bool,
    escape: Escape,
    departure: Departure,
    child: Box<dyn Fn(&Cx) -> Option<RouteId>>,
}

impl Fake {
    fn new(id: RouteId, log: &Log) -> Self {
        Self {
            id,
            log: log.clone(),
            occludes: Occlusion::View,
            claims_events: false,
            escape: Escape::Fall,
            departure: Departure::Cut,
            child: Box::new(|_| None),
        }
    }

    fn occluding(mut self, o: Occlusion) -> Self {
        self.occludes = o;
        self
    }

    fn claiming(mut self) -> Self {
        self.claims_events = true;
        self
    }

    fn escaping(mut self, e: Escape) -> Self {
        self.escape = e;
        self
    }

    fn departing(mut self, d: Departure) -> Self {
        self.departure = d;
        self
    }

    fn with_child(mut self, f: impl Fn(&Cx) -> Option<RouteId> + 'static) -> Self {
        self.child = Box::new(f);
        self
    }

    fn boxed(self) -> Box<dyn Route<Cx, Act>> {
        Box::new(self)
    }
}

impl Route<Cx, Act> for Fake {
    fn resolve_child(&self, cx: &Cx) -> Option<RouteId> {
        (self.child)(cx)
    }

    fn occludes(&self) -> Occlusion {
        self.occludes
    }

    fn event(&mut self, _cx: &Cx, _out: &mut Outbox<Act>, _ev: &RouteEvent) -> Handled {
        self.log.borrow_mut().push(format!("event:{}", self.id));
        Handled::from_bool(self.claims_events)
    }

    fn escape(&mut self, _cx: &Cx, out: &mut Outbox<Act>) -> Escape {
        self.log.borrow_mut().push(format!("escape:{}", self.id));
        if matches!(self.escape, Escape::Pop) {
            out.push(Act::Left);
        }
        self.escape
    }

    fn enter(&mut self, _cx: &mut Cx) {
        self.log.borrow_mut().push(format!("enter:{}", self.id));
    }

    fn leave(&mut self, _cx: &mut Cx) {
        self.log.borrow_mut().push(format!("leave:{}", self.id));
    }

    fn depart(&mut self, to: Option<RouteId>) -> Departure {
        self.log
            .borrow_mut()
            .push(format!("depart:{}->{}", self.id, to.unwrap_or("-")));
        self.departure
    }
}

/// The `ROUTING.md` §9 shape, shrunk to what the rules need:
///
/// ```text
/// /title
/// /title/settings          settings is one route under two parents
/// /world                   draws the 3D world
/// /world/journal           an ordinary child: View
/// /world/map-edit          a workspace: Surface
/// /world/map-edit/exit     a prompt over it: None
/// /world/pause
/// /world/pause/settings
/// ```
fn game(log: &Log) -> Router<Cx, Act> {
    let mut table = RouteTable::new();
    table
        .at_root("title")
        .at_root("world")
        .under("settings", "title")
        .under("settings", "pause")
        .under("pause", "world")
        .under("journal", "world")
        .under("map-edit", "world")
        .under("exit", "map-edit");

    let routes: Vec<(RouteId, Box<dyn Route<Cx, Act>>)> = vec![
        (
            "title",
            Fake::new("title", log)
                .with_child(|cx| cx.stance.filter(|s| *s == "settings"))
                .boxed(),
        ),
        (
            "world",
            Fake::new("world", log)
                .with_child(|cx| match cx.misroute {
                    // `exit` is a child of `map-edit`, not of `world`.
                    true => Some("exit"),
                    false => cx.stance,
                })
                .boxed(),
        ),
        ("settings", Fake::new("settings", log).boxed()),
        ("pause", Fake::new("pause", log).boxed()),
        ("journal", Fake::new("journal", log).boxed()),
        (
            "map-edit",
            Fake::new("map-edit", log)
                .occluding(Occlusion::Surface)
                .with_child(|cx| cx.editing_exit.then_some("exit"))
                .boxed(),
        ),
        (
            "exit",
            Fake::new("exit", log).occluding(Occlusion::None).boxed(),
        ),
    ];

    Router::new(table, routes, |cx: &Cx| cx.lifecycle).expect("the table is well formed")
}

fn drained(log: &Log) -> Vec<String> {
    log.borrow_mut().drain(..).collect()
}

fn ev(name: &str) -> ogham::widget::event::Event {
    ogham::widget::event::Event::new(name.to_string())
}

// ── the walk ───────────────────────────────────────────────────────────

#[test]
fn the_path_is_derived_from_session_state_every_frame() {
    let log = Log::default();
    let mut r = game(&log);
    let mut cx = Cx {
        lifecycle: "title",
        ..Cx::default()
    };

    r.resolve(&mut cx);
    assert_eq!(r.path(), ["title"]);

    // Nothing was pushed: the lifecycle field changed and the path
    // followed.
    cx.lifecycle = "world";
    r.resolve(&mut cx);
    assert_eq!(r.path(), ["world"]);
}

#[test]
fn each_node_claims_at_most_one_child_and_the_walk_stops_there() {
    let log = Log::default();
    let mut r = game(&log);
    let mut cx = Cx {
        lifecycle: "world",
        stance: Some("map-edit"),
        editing_exit: true,
        ..Cx::default()
    };
    r.resolve(&mut cx);
    assert_eq!(r.path(), ["world", "map-edit", "exit"]);
    assert_eq!(r.deepest(), Some("exit"));
}

#[test]
fn one_handler_under_two_parents_reaches_both_paths() {
    let log = Log::default();
    let mut r = game(&log);

    let mut cx = Cx {
        lifecycle: "title",
        stance: Some("settings"),
        ..Cx::default()
    };
    r.resolve(&mut cx);
    assert_eq!(r.path(), ["title", "settings"]);

    // The same route id, arrived at down a different edge. Nothing
    // remembers an origin; the walk *is* the origin.
    let mut cx = Cx {
        lifecycle: "world",
        stance: Some("pause"),
        ..Cx::default()
    };
    r.resolve(&mut cx);
    assert_eq!(r.path(), ["world", "pause"]);
}

#[test]
fn a_resolve_child_naming_a_non_child_is_refused_rather_than_followed() {
    // The table is the authority on what may sit beneath what. A handler
    // bug renders as if it resolved to nothing, because a blank surface is
    // recoverable mid-frame and a panic is not.
    let log = Log::default();
    let mut r = game(&log);
    let mut cx = Cx {
        lifecycle: "world",
        misroute: true,
        ..Cx::default()
    };
    r.resolve(&mut cx);
    assert_eq!(r.path(), ["world"], "`exit` is not a child of `world`");
}

// ── lifecycle ──────────────────────────────────────────────────────────

#[test]
fn enter_runs_outermost_first_and_leave_runs_deepest_first() {
    let log = Log::default();
    let mut r = game(&log);
    let mut cx = Cx {
        lifecycle: "world",
        stance: Some("map-edit"),
        editing_exit: true,
        ..Cx::default()
    };

    r.resolve(&mut cx);
    assert_eq!(
        drained(&log)
            .into_iter()
            .filter(|l| l.starts_with("enter:"))
            .collect::<Vec<_>>(),
        ["enter:world", "enter:map-edit", "enter:exit"]
    );

    cx.lifecycle = "title";
    cx.stance = None;
    cx.editing_exit = false;
    r.resolve(&mut cx);
    let calls = drained(&log);
    let leaves: Vec<&String> = calls.iter().filter(|l| l.starts_with("leave:")).collect();
    assert_eq!(leaves, ["leave:exit", "leave:map-edit", "leave:world"]);
}

#[test]
fn a_prompt_above_a_workspace_does_not_disturb_the_workspace() {
    // Axiom 10: `enter`/`leave` mean the id entered or left the *path*,
    // not "became deepest". The map editor is holding a transaction; its
    // own exit prompt appearing above it must not run its `leave`.
    let log = Log::default();
    let mut r = game(&log);
    let mut cx = Cx {
        lifecycle: "world",
        stance: Some("map-edit"),
        ..Cx::default()
    };
    r.resolve(&mut cx);
    let _ = drained(&log);

    cx.editing_exit = true;
    r.resolve(&mut cx);
    let calls = drained(&log);
    assert!(
        calls.contains(&"enter:exit".to_string()),
        "the prompt entered: {calls:?}"
    );
    assert!(
        !calls.iter().any(|c| c == "leave:map-edit"),
        "the workspace must not be torn down under its own prompt: {calls:?}"
    );
}

#[test]
fn an_unchanged_path_runs_no_lifecycle_at_all() {
    let log = Log::default();
    let mut r = game(&log);
    let mut cx = Cx {
        lifecycle: "world",
        ..Cx::default()
    };
    r.resolve(&mut cx);
    let _ = drained(&log);

    assert!(!r.resolve(&mut cx), "the path did not change");
    assert!(drained(&log).is_empty(), "nothing to announce");
}

#[test]
fn the_outgoing_route_is_asked_how_it_leaves() {
    // Two of the three consumers bleed the outgoing surface over the
    // incoming one. A router that swaps the top of the stack and drops
    // the old route breaks both, so `depart` runs before `leave`.
    let log = Log::default();
    let mut table = RouteTable::new();
    table.at_root("a").at_root("b");
    let routes: Vec<(RouteId, Box<dyn Route<Cx, Act>>)> = vec![
        (
            "a",
            Fake::new("a", &log)
                .departing(Departure::Bleed { seconds: 0.4 })
                .boxed(),
        ),
        ("b", Fake::new("b", &log).boxed()),
    ];
    let mut r = Router::new(table, routes, |cx: &Cx| cx.lifecycle).expect("well formed");

    let mut cx = Cx {
        lifecycle: "a",
        ..Cx::default()
    };
    r.resolve(&mut cx);
    let _ = drained(&log);

    cx.lifecycle = "b";
    r.resolve(&mut cx);
    assert_eq!(
        r.last_departure(),
        Some(("a", Departure::Bleed { seconds: 0.4 }))
    );
    let calls = drained(&log);
    let depart = calls.iter().position(|c| c == "depart:a->b");
    let leave = calls.iter().position(|c| c == "leave:a");
    assert!(
        depart < leave,
        "depart must run while the route is still whole: {calls:?}"
    );
}

// ── occlusion ──────────────────────────────────────────────────────────

#[test]
fn an_ordinary_child_hides_its_parents_view_but_not_its_draw() {
    let log = Log::default();
    let mut r = game(&log);
    let mut cx = Cx {
        lifecycle: "world",
        stance: Some("journal"),
        ..Cx::default()
    };
    r.resolve(&mut cx);
    assert_eq!(r.drawing(), ["world", "journal"], "the 3D world keeps drawing");
    assert_eq!(r.visible_views(), ["journal"], "the HUD does not");
}

#[test]
fn a_workspace_hides_both() {
    let log = Log::default();
    let mut r = game(&log);
    let mut cx = Cx {
        lifecycle: "world",
        stance: Some("map-edit"),
        ..Cx::default()
    };
    r.resolve(&mut cx);
    assert_eq!(r.drawing(), ["map-edit"]);
    assert_eq!(r.visible_views(), ["map-edit"]);
}

#[test]
fn a_prompt_leaves_the_workspace_it_is_leaving_on_screen() {
    // The seventh symptom bug in `ROUTING.md` §2.4: today exactly one
    // widget tree draws, so the save-or-discard card takes the tree from
    // the map editor and the editor's rail and toolbar vanish while its
    // canvas keeps rendering. `Occlusion::None` is what says otherwise.
    let log = Log::default();
    let mut r = game(&log);
    let mut cx = Cx {
        lifecycle: "world",
        stance: Some("map-edit"),
        editing_exit: true,
        ..Cx::default()
    };
    r.resolve(&mut cx);
    assert_eq!(r.drawing(), ["map-edit", "exit"]);
    assert_eq!(
        r.visible_views(),
        ["map-edit", "exit"],
        "the card is a scrim over a workspace that is still drawn"
    );
}

// ── arbitration ────────────────────────────────────────────────────────

#[test]
fn input_goes_to_the_deepest_route_that_claims_it() {
    let log = Log::default();
    let mut table = RouteTable::new();
    table.at_root("world").under("journal", "world");
    let routes: Vec<(RouteId, Box<dyn Route<Cx, Act>>)> = vec![
        ("world", Fake::new("world", &log).claiming().boxed()),
        ("journal", Fake::new("journal", &log).claiming().boxed()),
    ];
    let mut r = Router::new(table, routes, |_: &Cx| "world").expect("well formed");
    let mut cx = Cx::default();
    // `world` has no child resolver here, so drive the path by hand.
    r.resolve(&mut cx);
    let _ = drained(&log);

    let mut out = Outbox::new();
    assert_eq!(r.event(&cx, &mut out, &RouteEvent::Input(&ev("mouse_down"))), Handled::Yes);
    assert_eq!(
        drained(&log),
        ["event:world"],
        "only the deepest active route was offered it"
    );
}

#[test]
fn an_unclaimed_event_falls_all_the_way_through() {
    let log = Log::default();
    let mut r = game(&log);
    let mut cx = Cx {
        lifecycle: "world",
        stance: Some("journal"),
        ..Cx::default()
    };
    r.resolve(&mut cx);
    let _ = drained(&log);

    let mut out = Outbox::new();
    assert_eq!(r.event(&cx, &mut out, &RouteEvent::Input(&ev("mouse_down"))), Handled::No);
    assert_eq!(
        drained(&log),
        ["event:journal", "event:world"],
        "offered deepest-first, and nobody took it"
    );
}

// ── escape ─────────────────────────────────────────────────────────────

#[test]
fn escape_belongs_to_the_deepest_route_and_nothing_above_intercepts() {
    let log = Log::default();
    let mut table = RouteTable::new();
    table.at_root("world").under("journal", "world");
    let routes: Vec<(RouteId, Box<dyn Route<Cx, Act>>)> = vec![
        (
            "world",
            Fake::new("world", &log).escaping(Escape::Pop).boxed(),
        ),
        (
            "journal",
            Fake::new("journal", &log).escaping(Escape::Pop).boxed(),
        ),
    ];
    let mut r = Router::new(table, routes, |_: &Cx| "world").expect("well formed");
    let mut cx = Cx::default();
    r.resolve(&mut cx);
    let _ = drained(&log);

    let mut out = Outbox::new();
    assert_eq!(r.escape(&cx, &mut out), EscapeOutcome::Popped("world"));
    assert_eq!(drained(&log), ["escape:world"]);
}

#[test]
fn escape_falls_to_the_parent_when_a_route_declines_it() {
    let log = Log::default();
    let mut r = game(&log);
    let mut cx = Cx {
        lifecycle: "world",
        stance: Some("journal"),
        ..Cx::default()
    };
    r.resolve(&mut cx);
    let _ = drained(&log);

    // Both fakes default to `Fall`, so nobody claims it — and the host
    // hears about it, which is the only case in which anything above the
    // router sees Escape at all.
    let mut out = Outbox::new();
    assert_eq!(r.escape(&cx, &mut out), EscapeOutcome::Unclaimed);
    assert_eq!(drained(&log), ["escape:journal", "escape:world"]);
}

#[test]
fn a_route_with_unsaved_work_prompts_instead_of_popping() {
    let log = Log::default();
    let mut table = RouteTable::new();
    table.at_root("map-edit");
    let routes: Vec<(RouteId, Box<dyn Route<Cx, Act>>)> = vec![(
        "map-edit",
        Fake::new("map-edit", &log)
            .escaping(Escape::Prompt)
            .boxed(),
    )];
    let mut r = Router::new(table, routes, |_: &Cx| "map-edit").expect("well formed");
    let mut cx = Cx::default();
    r.resolve(&mut cx);

    let mut out = Outbox::new();
    assert_eq!(r.escape(&cx, &mut out), EscapeOutcome::Prompted("map-edit"));
    assert!(
        out.peek().is_empty(),
        "a prompt asks for nothing; the prompt route does the asking"
    );
}

#[test]
fn a_pop_reaches_the_host_as_an_action_rather_than_editing_the_path() {
    // "The path is derived, never pushed" holds even for the one gesture
    // whose whole purpose is to change it: `Pop` asks the route to stop
    // claiming itself, and the next `resolve` is what shortens the path.
    let log = Log::default();
    let mut table = RouteTable::new();
    table.at_root("world").under("journal", "world");
    let routes: Vec<(RouteId, Box<dyn Route<Cx, Act>>)> = vec![
        (
            "world",
            Fake::new("world", &log)
                .with_child(|_| Some("journal"))
                .boxed(),
        ),
        (
            "journal",
            Fake::new("journal", &log).escaping(Escape::Pop).boxed(),
        ),
    ];
    let mut r = Router::new(table, routes, |_: &Cx| "world").expect("well formed");
    let mut cx = Cx::default();
    r.resolve(&mut cx);
    assert_eq!(r.path(), ["world", "journal"]);

    let mut out = Outbox::new();
    assert_eq!(r.escape(&cx, &mut out), EscapeOutcome::Popped("journal"));
    assert_eq!(out.drain(), vec![Act::Left]);
    // The router did not touch the path: the route asked the host, and
    // the *next* resolve is what will shorten it.
    assert_eq!(r.path(), ["world", "journal"]);
}

// ── construction ───────────────────────────────────────────────────────

#[test]
fn a_registered_id_with_no_handler_fails_at_startup() {
    let log = Log::default();
    let mut table = RouteTable::new();
    table.at_root("title").under("settings", "title");
    let routes: Vec<(RouteId, Box<dyn Route<Cx, Act>>)> =
        vec![("title", Fake::new("title", &log).boxed())];
    let err = Router::new(table, routes, |_: &Cx| "title")
        .err()
        .expect("a route with no handler must not build");
    assert!(
        matches!(err, TableError::UnknownParent { child: "settings", .. }),
        "unexpected error: {err}"
    );
}

#[test]
fn a_malformed_table_never_produces_a_router() {
    let log = Log::default();
    let mut table = RouteTable::new();
    table.at_root("title").under("settings", "puase");
    let routes: Vec<(RouteId, Box<dyn Route<Cx, Act>>)> = vec![
        ("title", Fake::new("title", &log).boxed()),
        ("settings", Fake::new("settings", &log).boxed()),
    ];
    assert_eq!(
        Router::new(table, routes, |_: &Cx| "title").err(),
        Some(TableError::UnknownParent {
            child: "settings",
            parent: "puase"
        })
    );
}

#[test]
fn an_outbox_action_survives_the_frame_that_asked_for_it() {
    let log = Log::default();
    let mut r = game(&log);
    let mut cx = Cx {
        lifecycle: "world",
        ..Cx::default()
    };
    r.resolve(&mut cx);

    let mut out = Outbox::new();
    out.push(Act::Saved);
    r.update(&cx, &mut out, 1.0 / 60.0);
    assert_eq!(out.drain(), vec![Act::Saved]);
}

// ── a claim is not allowed to go stale ─────────────────────────────────

/// A route that owns its claim, which is the shape every real in-session
/// route has: pause is raised by the *world*, held in the world's own
/// field, and read back by `resolve_child`.
struct Claimer {
    claim: Option<RouteId>,
    /// Whether `leave` clears it. `false` is the bug this test exists for.
    tidy: bool,
}

impl Route<Cx, Act> for Claimer {
    fn resolve_child(&self, _cx: &Cx) -> Option<RouteId> {
        self.claim
    }

    fn event(&mut self, _cx: &Cx, _out: &mut Outbox<Act>, _ev: &RouteEvent) -> Handled {
        Handled::No
    }

    fn escape(&mut self, _cx: &Cx, _out: &mut Outbox<Act>) -> Escape {
        self.claim = Some("pause");
        Escape::Ignore
    }

    fn child_popped(&mut self, _child: RouteId) {
        self.claim = None;
    }

    fn leave(&mut self, _cx: &mut Cx) {
        if self.tidy {
            self.claim = None;
        }
    }
}

fn claiming_game(log: &Log, tidy: bool) -> Router<Cx, Act> {
    let mut table = RouteTable::new();
    table.at_root("title").at_root("world").under("pause", "world");
    let routes: Vec<(RouteId, Box<dyn Route<Cx, Act>>)> = vec![
        ("title", Fake::new("title", log).boxed()),
        ("world", Box::new(Claimer { claim: None, tidy })),
        ("pause", Fake::new("pause", log).boxed()),
    ];
    Router::new(table, routes, |cx: &Cx| cx.lifecycle).expect("well formed")
}

/// Leave a session from the pause overlay, then start another one.
///
/// The world is claiming pause when it goes. If it still is when it comes
/// back, the new session opens with pause already up — which is exactly
/// what celia did, and it reads as the overlay refusing to close rather
/// than as stale state.
#[test]
fn a_claim_cleared_on_leave_does_not_survive_leaving_the_path() {
    let log = Log::default();
    let mut r = claiming_game(&log, true);
    let mut cx = Cx {
        lifecycle: "world",
        ..Cx::default()
    };
    r.resolve(&mut cx);
    let mut out = Outbox::new();
    r.escape(&cx, &mut out);
    r.resolve(&mut cx);
    assert_eq!(r.path(), ["world", "pause"]);

    cx.lifecycle = "title";
    r.resolve(&mut cx);
    assert_eq!(r.path(), ["title"]);

    cx.lifecycle = "world";
    r.resolve(&mut cx);
    assert_eq!(r.path(), ["world"], "the claim did not survive");
}

/// The same sequence against a route that forgets, so the test above is
/// known to be testing something.
#[test]
fn a_claim_not_cleared_on_leave_comes_back_stale() {
    let log = Log::default();
    let mut r = claiming_game(&log, false);
    let mut cx = Cx {
        lifecycle: "world",
        ..Cx::default()
    };
    r.resolve(&mut cx);
    let mut out = Outbox::new();
    r.escape(&cx, &mut out);
    r.resolve(&mut cx);
    cx.lifecycle = "title";
    r.resolve(&mut cx);
    cx.lifecycle = "world";
    r.resolve(&mut cx);
    assert_eq!(
        r.path(),
        ["world", "pause"],
        "this is the defect; `leave` clearing the claim is what prevents it"
    );
}

// ── the startup check ──────────────────────────────────────────────────
//
// `Chrome::validate_raises` and `Chrome::validate_against` were written
// with tests and wired to nothing. `Chrome::validate` is where they are
// called from; these are the two failures their own doc comments cite.

fn chrome(source: &str, handlers: &[&str]) -> ogham::route::Chrome {
    let mut config = ogham::runtime::config::RuntimeConfig::new();
    for name in handlers {
        config = config.with_event_handler(*name, |_| Ok(ogham::runtime::value::Value::Void));
    }
    ogham::route::Chrome::new(ogham::Ogham::from_source(source, config).expect("from_source"))
}

/// celia's Back button: a `back()` in the document's `events {}` block
/// with no matching handler on the host. It drew, it clicked, and it
/// reached nobody, for months, and nothing anywhere said so.
#[test]
fn a_declared_raise_with_no_handler_is_named() {
    let mut c = chrome(
        r#"events { menu(string), back() };
           screen "title" { view Flex { mouse_down: fn () { event("back") } } };
           let main = fn () { outlet() };"#,
        &["menu"],
    );
    let report = c.validate(&["title"]).expect("the drift is reported");
    assert!(report.contains("back"), "{report}");
    assert!(report.contains("no handler registered"), "{report}");
}

/// The other end of the same wire: a route the table registers that the
/// document draws no `screen` for, and a `screen` nobody routes to.
#[test]
fn a_screen_and_a_route_id_that_disagree_are_named() {
    let mut c = chrome(
        r#"screen "title" { view Flex {} };
           screen "credits" { view Flex {} };
           let main = fn () { outlet() };"#,
        &[],
    );
    let report = c.validate(&["title", "settings"]).expect("the drift is reported");
    assert!(report.contains("credits"), "{report}");
    assert!(report.contains("settings"), "{report}");
}

#[test]
fn a_document_that_agrees_with_its_host_reports_nothing() {
    let mut c = chrome(
        r#"events { back() };
           screen "title" { view Flex { mouse_down: fn () { event("back") } } };
           let main = fn () { outlet() };"#,
        &["back"],
    );
    assert_eq!(c.validate(&["title"]), None);
    assert_eq!(c.validation(), None);
}
