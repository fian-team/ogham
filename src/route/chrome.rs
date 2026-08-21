//! One ogham instance, one projection, one reload-failure surface.
//!
//! Three games hand-rolled this wrapper — `untold_lore::chrome::Chrome`,
//! `regency::chrome::Chrome`, `celia::chrome::Chrome` — and the three
//! converged on the same shape except where they diverged by accident:
//! each handles a `.ogh` that stops compiling differently, so the same
//! mistake fails three ways depending on which game you were in. This
//! crate owns it, which means a route bringing another crate's document
//! (`Route::own_ui`) fails the same way as the shared one.
//!
//! What it adds over a bare `Ogham`: the route path and each active
//! route's state slice go in as *scoped* host state, so a screen block
//! reads its own fields and the root scope and nothing else.

use std::collections::HashMap;

use crate::runtime::value::Value;
use crate::{Ogham, ReloadRefused};

use crate::route::RouteId;

/// A mounted ogham document plus the projection that feeds it.
pub struct Chrome {
    ui: Ogham,
    /// The last values pushed, per key, so a frame that changes nothing
    /// pushes nothing. Per-key rather than whole-struct, which is what
    /// makes per-route projection cheaper than the union-struct compare
    /// it replaces rather than merely tidier.
    last: HashMap<String, Value>,
    /// Set when the document failed to reload, cleared when it compiles
    /// again. Reported by the host once rather than per frame — the whole
    /// reason this is here and not in three places.
    error: Option<String>,
    /// What [`validate`](Self::validate) found, if it has been run and
    /// found anything. Kept rather than merely printed, so a test can
    /// assert on it — a startup check nothing can fail is a check that
    /// gets ignored.
    validation: Option<String>,
    /// The ids the mount-time [`validate`](Self::validate) ran against,
    /// kept so a hot reload can re-run the same check against the same
    /// table. `None` until a host validates — a reload must not invent a
    /// check the mount never asked for.
    expected: Option<Vec<RouteId>>,
    /// How many times the mounted document has been replaced by a hot
    /// reload. A host that caches anything read *out* of the document —
    /// the names its selection binds, say — compares this to know its
    /// cache died with the runtime it described.
    reloads: u64,
    /// What the contract check refused, if it has refused something
    /// (`APPLICATION.md` §4.1). Distinct from [`error`](Self::error) on
    /// purpose: a refused *edit* leaves the running document perfectly
    /// healthy, and telling a host its document is broken when it is not is
    /// how a game ends up showing a fallback it did not need.
    refusal: Option<String>,
}

impl Chrome {
    pub fn new(ui: Ogham) -> Self {
        Self {
            ui,
            last: HashMap::new(),
            error: None,
            validation: None,
            expected: None,
            reloads: 0,
            refusal: None,
        }
    }

    /// A chrome standing in for a document that could not be loaded.
    ///
    /// The blank fallback *compiles*, so without this the error is
    /// invisible: `error()` reports frame failures, and a document that
    /// never loaded never fails a frame. A game asking "did my document
    /// load?" would be told yes.
    pub fn failed(ui: Ogham, why: String) -> Self {
        eprintln!("route: the document could not be loaded: {why}");
        Self {
            ui,
            last: HashMap::new(),
            error: Some(why),
            validation: None,
            expected: None,
            reloads: 0,
            refusal: None,
        }
    }

    pub fn ui(&self) -> &Ogham {
        &self.ui
    }

    pub fn ui_mut(&mut self) -> &mut Ogham {
        &mut self.ui
    }

    /// How many hot reloads this document has taken.
    ///
    /// A reload replaces the runtime, so anything a host read *out* of the
    /// document — the names its selection binds, the screens it declares —
    /// died with it. Comparing this is how the host knows to read again;
    /// the alternative is re-reading the module schema every frame, which
    /// clones it.
    pub fn reloads(&self) -> u64 {
        self.reloads
    }

    /// The reload error, if the document is currently broken.
    ///
    /// A `.ogh` that stops compiling must *say so*. The failure mode this
    /// exists to prevent is a blank screen that reads as a hang, which is
    /// what two of the three hand-rolled wrappers produced.
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// Check the document's ids against a route table's, once at load.
    ///
    /// A registered id with no `screen` block, or a block nobody routes
    /// to, is an error naming both (axiom 11). `untold_lore`'s
    /// `host_state` comment listed 13 modes against 16 real ones; that
    /// drift was silent, and this is what makes it impossible.
    pub fn validate_against(&mut self, ids: &[RouteId]) -> Result<(), String> {
        let Some(schema) = self.ui.module_schema() else {
            // A document that will not compile has a real error waiting
            // with real diagnostics; do not bury it under a second one.
            return Ok(());
        };
        schema.validate_screens(ids)
    }

    /// Check the document's declared events against the names this host
    /// registered handlers for.
    ///
    /// The other end of the same wire as
    /// [`validate_against`](Self::validate_against): a declared raise with
    /// no handler is a button that draws, clicks and reaches nobody. That
    /// is not hypothetical — celia's Back button was exactly this, a
    /// `back()` in the document's `events {}` block with no matching entry
    /// in the host's list, and nothing anywhere said so.
    pub fn validate_raises(&mut self, registered: &[&str]) -> Result<(), String> {
        let Some(schema) = self.ui.module_schema() else {
            return Ok(());
        };
        schema.validate_events(registered)
    }

    /// Both halves of the startup check, against the host this document
    /// is actually mounted in.
    ///
    /// This is where the two above are *called from*. Written in July,
    /// tested in July, and wired to nothing until this: `validate_raises`
    /// had no callers anywhere in five repositories and `validate_against`
    /// had one, in a test. A guard nobody runs is documentation with a
    /// build cost — and the failure its own doc comment cites, celia's
    /// `back()` with no handler, went on shipping the whole time.
    ///
    /// `ids` is what the host expects this document to draw — the table's
    /// ids less any route that brings a document of its own
    /// ([`Route::brings_own_document`](crate::route::Route::brings_own_document)).
    ///
    /// The registered event names come from the instance itself rather
    /// than from an argument, so there is no list for a caller to forget
    /// to update — the drift this is trying to catch is exactly the drift
    /// a hand-maintained argument would acquire.
    ///
    /// Reports; does not fail. A router whose table has drifted from its
    /// document still stands up, because the alternative is a game that
    /// will not boot over a screen nobody has routed yet, and that is how
    /// a check gets deleted rather than fixed.
    pub fn validate(&mut self, ids: &[RouteId]) -> Option<&str> {
        self.expected = Some(ids.to_vec());
        let registered = self.ui.registered_event_names();
        let registered: Vec<&str> = registered.iter().map(|s| s.as_str()).collect();

        let mut report = String::new();
        if let Err(why) = self.validate_against(ids) {
            report.push_str(&why);
        }
        if let Err(why) = self.validate_raises(&registered) {
            if !report.is_empty() {
                report.push('\n');
            }
            report.push_str(&why);
        }

        self.validation = match report.is_empty() {
            true => None,
            false => {
                eprintln!("route: {report}");
                Some(report)
            }
        };
        self.validation.as_deref()
    }

    /// What the last [`validate`](Self::validate) found. `None` when it
    /// found nothing, or has not run.
    pub fn validation(&self) -> Option<&str> {
        self.validation.as_deref()
    }

    // --- the contract's mechanism (`APPLICATION.md` §4.1) ------------------
    //
    // The *question* — does this document agree with what its scopes
    // publish? — is the `contract` crate's, because asking it needs the
    // structure framework and this crate does not depend on it (§2). What
    // is here is the machinery the answer is delivered through: a gate the
    // reload has to pass, and a refusal held apart from a compile error.
    // `contract::Checked` is the trait that puts a question into the gate.

    /// What the contract check refused. `None` when it refused nothing, or
    /// has not run.
    ///
    /// A refusal is not an [`error`](Self::error): after a refused reload
    /// the running document is intact and still drawing, and the sentence
    /// here is about the file on disk.
    pub fn refusal(&self) -> Option<&str> {
        self.refusal.as_deref()
    }

    /// Record what a contract check refused, or that it refused nothing.
    ///
    /// Reported once rather than per frame — the whole reason a refusal is
    /// remembered here instead of returned and forgotten.
    pub fn refuse(&mut self, why: Option<String>) {
        let Some(why) = why else {
            self.refusal = None;
            return;
        };
        if self.refusal.as_deref() != Some(why.as_str()) {
            eprintln!("route: the document was refused, and the running one stands:\n{why}");
        }
        self.refusal = Some(why);
    }

    /// [`frame`](Self::frame) with the hot reload held to a gate.
    ///
    /// §4.1's last sentence, as a call: a hot-reload refusal rejects the
    /// new document and names the field **without tearing down the running
    /// instance**. The edit is checked before the swap, so a refused one
    /// costs the candidate and nothing else — the running tree keeps its
    /// state and its focus, the projection cache is untouched, and the file
    /// stays watched, so the next save that passes heals the session.
    ///
    /// `gate` is handed the candidate's own module schema and answers with
    /// the sentence that refuses it, or `Ok(())`. It is a closure rather
    /// than a contract check because the contract lives one crate up; what
    /// a caller passes is `contract::refusals`.
    pub fn frame_gated(
        &mut self,
        gate: impl FnOnce(&crate::runtime::schema::ModuleSchema) -> Result<(), String>,
        width: f32,
        height: f32,
        dt: f32,
    ) {
        if self.ui.check_for_changes() {
            match self.ui.reload_if(gate) {
                Ok(()) => {
                    self.reloaded();
                    // The candidate the gate accepted is the running
                    // document now, so whatever it refused before is over.
                    self.refusal = None;
                }
                Err(ReloadRefused::Broken(e)) => self.record_failure(e),
                Err(ReloadRefused::Refused(why)) => self.refuse(Some(why)),
            }
        }
        // The watcher's events were drained above, so this does not reload
        // again — and must not, because that reload would be ungated.
        self.frame(width, height, dt);
    }

    /// Push the active path. The document renders these screens in order,
    /// so a deeper route draws over a shallower one.
    pub fn set_path(&mut self, path: &[RouteId]) {
        self.ui.with_runtime_mut(|rt| rt.set_route_path(path));
    }

    /// Push one route's state slice into its own scope.
    ///
    /// Only changed keys reach the runtime, so an idle frame costs a
    /// compare and nothing else.
    pub fn project_route(&mut self, id: RouteId, fields: Option<HashMap<String, Value>>) {
        let Some(fields) = fields else {
            return;
        };
        self.project_fields(id, fields);
    }

    /// [`project_route`](Self::project_route) with the walk already run —
    /// what a host uses when it must end its borrow of the route before it
    /// can borrow the chrome.
    pub fn project_fields(&mut self, id: RouteId, fields: HashMap<String, Value>) {
        for (name, value) in fields {
            let key = format!("{id}::{name}");
            if self.last.get(&key) == Some(&value) {
                continue;
            }
            self.last.insert(key.clone(), value.clone());
            self.ui
                .with_runtime_mut(|rt| rt.inject_host_state(key, value));
        }
    }

    /// Push a root-scope value — chrome-global and session-global keys,
    /// the measured remainder that does not partition per route
    /// (`ROUTING.md` §5).
    pub fn project_root(&mut self, name: &str, value: Value) {
        if self.last.get(name) == Some(&value) {
            return;
        }
        self.last.insert(name.to_string(), value.clone());
        self.ui
            .with_runtime_mut(|rt| rt.inject_host_state(name.to_string(), value));
    }

    /// Advance the document a frame, recording a reload failure rather
    /// than dropping it.
    pub fn frame(&mut self, width: f32, height: f32, dt: f32) {
        match self.ui.frame(width, height, dt) {
            // A document that never loaded keeps its error: a blank
            // fallback frames perfectly and would otherwise clear it.
            // (A *reload* clears it inside `reloaded` — the fallback has
            // no watcher, so it can never take that path.)
            Ok(report) => {
                if report.reloaded {
                    self.reloaded();
                }
            }
            Err(e) => self.record_failure(e),
        }
    }

    /// Reload the mounted document now, as [`frame`](Self::frame) does
    /// when a watched file changes — for a host that drives the reload
    /// itself. Same duties either way: on success the projection cache
    /// drops and the startup check re-runs; on failure the running
    /// document stands and [`error`](Self::error) says why.
    pub fn reload(&mut self) {
        match self.ui.reload() {
            Ok(()) => self.reloaded(),
            Err(e) => self.record_failure(e),
        }
    }

    /// A hot reload just replaced the runtime. Two duties.
    ///
    /// The projection cache dies with the runtime it described
    /// ([`forget_projection`](Self::forget_projection)), so the next
    /// projection pushes every key into the fresh document instead of
    /// assuming it still holds them. And the startup check re-runs
    /// against the same ids the mount validated, because a hot edit can
    /// introduce exactly the drift the check exists to catch — a check
    /// that runs once per process is a check every hot session escapes,
    /// and the drift would sit silent until the next restart.
    fn reloaded(&mut self) {
        self.reloads += 1;
        // The document compiled again, which is the reload error's
        // documented end of life.
        self.error = None;
        self.forget_projection();
        if let Some(ids) = self.expected.clone() {
            self.validate(&ids);
        }
    }

    /// A reload that did not take: the running document stands, the
    /// error is kept, and it is printed once rather than per frame.
    fn record_failure(&mut self, e: crate::runtime::error::RuntimeError) {
        let msg = format!("{e:?}");
        if self.error.as_deref() != Some(msg.as_str()) {
            eprintln!("route: the mounted document failed: {msg}");
        }
        self.error = Some(msg);
    }

    /// Drop the projection cache.
    ///
    /// A hot reload replaces the runtime, so every key has to be pushed
    /// again — a cache that survives it would leave the fresh document
    /// holding the config's initial values for everything that happened
    /// not to change this frame.
    pub fn forget_projection(&mut self) {
        self.last.clear();
    }
}
