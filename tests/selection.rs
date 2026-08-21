//! WP-3.2: `select` — the consumer's half of the contract, as the
//! *language* holds it (`docs/internal/APPLICATION.md` §4.1, §4.6).
//!
//! **§4.6, top-level binding**, is the load-bearing property and it is
//! constructed here rather than asserted: `tests/documents/manor_hud.ogh`
//! is regency's in-manor HUD family, checked in verbatim — five helpers,
//! twenty unqualified reads of `hud`. The two arms of
//! [`the_manor_screen_family_compiles_against_a_selection_unedited`] put
//! the *same bytes* under a `host_state {}` block and under a `select`,
//! and both compile. That is the migration property in the only form that
//! can be checked — if the selected arm needed one edit, the two arms
//! could not share the file.
//!
//! What a selection is checked *against* is the contract crate's, and its
//! tests are `contract/tests/selection.rs`: this file has no store in it,
//! because §4.6 is answered by the compiler alone.

use ogham::parser::Parser;
use ogham::runtime::compiler::Compiler;
use ogham::runtime::config::RuntimeConfig;
use ogham::runtime::value::Value;
use ogham::scanner::Scanner;
use ogham::Ogham;

/// regency's palette, verbatim.
const STATIONERY: &str = include_str!("documents/manor_stationery.ogh");

/// regency's manor HUD family, verbatim. Read by both arms, unedited.
const HUD: &str = include_str!("documents/manor_hud.ogh");

/// The record block those helpers are written against today — regency's
/// `Pip` and `Hud`, verbatim from `client.ogh` lines 72-98.
///
/// Twenty-four fields of shape the document restates and the Rust builder
/// restates, held together by a hand-written conformance test. It is what
/// the selection below replaces with one line.
const DECLARED: &str = r#"
record Pip { filled: bool };

record Hud {
  clock: string,
  act: string,
  mode_hint: string,
  threat_caption: string,
  threat: array<Pip>,
  threat_alarm: bool,
  threat_round: bool,
  name: string,
  status: string,
  status_alarm: bool,
  purse_label: string,
  purse: string,
  purse_lit: bool,
  impropriety: string,
  impropriety_alarm: bool,
  light_text: string,
  light_frac: float,
  vitality: array<Pip>,
  composure: array<Pip>,
  condition_shown: bool,
  prompt: string,
};

host_state { hud: Hud };
"#;

/// The same contract, selected. One line, and the shapes stay where they
/// were declared — in the Rust type the manor node provides.
const SELECTED: &str = "\nselect manor { hud };\n";

const MAIN: &str = r#"
let main = fn () {
  Flex { style: {}, children: [ hud_evening(), hud_you() ] }
};
"#;

/// Parse and compile — which is where strict-mode identifier resolution
/// runs, and so where "compiles against a selection" is answered.
fn compiles(source: &str) -> Result<(), String> {
    let module = Parser::new(Scanner::new(source.to_string()).scan())
        .parse()
        .map_err(|e| format!("{e:?}"))?;
    Compiler::compile_module(&module)
        .map(|_| ())
        .map_err(|e| format!("{e:?}"))
}

/// The diagnostic a document that will not compile produced.
fn refused(source: &str) -> String {
    match compiles(source) {
        Ok(_) => panic!("this document was expected not to compile"),
        Err(why) => why,
    }
}

// ── §4.6: the binding ──────────────────────────────────────────────────

/// **The acceptance test.** regency's `manor_screen` family compiles
/// against a selection with no body edits.
///
/// Both arms are the same three constants in the same order, and the one
/// that differs is the *declaration*: twenty-four restated field shapes, or
/// `select manor { hud };`. The helpers in between are one file, read
/// twice.
#[test]
fn the_manor_screen_family_compiles_against_a_selection_unedited() {
    let declared = format!("{STATIONERY}{DECLARED}{HUD}{MAIN}");
    let selected = format!("{STATIONERY}{SELECTED}{HUD}{MAIN}");

    compiles(&declared).expect("the family compiles as it is written today");
    compiles(&selected).expect("and against a selection, with the same bodies");

    // The bodies really are the same bytes: everything after the
    // declaration is identical, character for character.
    assert_eq!(
        declared.split_once(HUD).map(|(_, rest)| rest),
        selected.split_once(HUD).map(|(_, rest)| rest),
    );
}

/// The binding is not a loosening. A selected document is still strict —
/// a name it does not select is an unknown identifier at compile time,
/// with the same diagnostic a declared one gets.
///
/// This is the half that would be easy to get wrong and impossible to
/// notice: a `select` that merely turned strict mode *off* would compile
/// every arm of the test above and give a modder nothing.
#[test]
fn a_selected_document_is_still_strict_about_every_other_name() {
    let selected = format!("{STATIONERY}{SELECTED}{HUD}{MAIN}");
    let typo = selected.replace("hud.clock", "hud.clock + hudd");
    let why = refused(&typo);
    assert!(why.contains("hudd"), "{why}");
    assert!(why.contains("did you mean `hud`"), "{why}");
}

/// A name may be bound once. Two selections that both bind `heading` would
/// leave every read of it reaching whichever one the lookup happened to
/// find, so the document is refused at parse time and the name is said.
#[test]
fn a_name_two_selections_both_bind_is_refused_naming_it() {
    let why = refused(
        "select foyer { heading };\nselect table { heading };\nlet main = fn () { heading };",
    );
    assert!(why.contains("heading"), "{why}");

    let why = refused(
        "host_state { heading: string };\nselect table { heading };\nlet main = fn () { heading };",
    );
    assert!(why.contains("host_state"), "{why}");
}

/// A selection names fields. Writing a shape beside one is the inverted
/// contract coming back, so it is refused *naming what to write instead* —
/// because the first thing an author migrating a `host_state {}` block will
/// do is paste it in and change the keyword.
#[test]
fn a_selection_that_restates_a_shape_says_so() {
    let why = refused("select manor { hud: Hud };\nlet main = fn () { hud };");
    assert!(why.contains("selected, not declared"), "{why}");
}

/// `select` is contextual, not a keyword. Every document that already uses
/// the word — as a helper, a field, a local — goes on compiling, which is
/// the measured rule `screen` established (`LANGUAGE.md`).
#[test]
fn select_is_still_an_ordinary_name() {
    compiles(
        r#"
        let select = fn (width: int, children: int) { Flex { style: { width: width } } };
        let main = fn () { select(1, 2) };
        "#,
    )
    .expect("celia's `screen`-shaped layout helper, under the other name");

    compiles(
        r#"
        host_state { select: string };
        let main = fn () { Text { text: select } };
        "#,
    )
    .expect("a host-state field called `select`");
}

/// A selected name is read exactly as a declared one was: the same
/// host-state key, pushed the same way.
///
/// This is why §4.6's binding costs no bytecode and no host change — the
/// projection a game already writes goes on working, which is the other
/// half of "the migration touches zero helper bodies".
#[test]
fn a_selected_name_reads_the_host_state_key_of_the_same_name() {
    let ui = Ogham::from_source(
        "select manor { hud };\nlet main = fn () { Text { text: hud } };",
        RuntimeConfig::new().with_host_state(
            [("hud".to_string(), Value::String(String::new()))]
                .into_iter()
                .collect(),
        ),
    )
    .expect("a document with one selected name");
    ui.with_runtime_mut(|rt| {
        rt.inject_host_state("hud".to_string(), Value::String("9:40".to_string()))
    });
    assert_eq!(
        ui.with_runtime_mut(|rt| rt.get_host_state("hud")),
        Some(Value::String("9:40".to_string()))
    );
}
