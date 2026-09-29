//! **A `for` that ends a block is the block's value** — the collecting
//! form, like every other trailing expression. A helper whose body ended
//! in a bare `for` returned one element where the same loop bound to a
//! `let` returned them all, and nothing said so: the widget drew one row
//! of five. Each case here is paired with a form that already worked, so
//! the two have to agree.

use ogham::runtime::config::RuntimeConfig;
use ogham::widget::WidgetRef;
use ogham::Ogham;

fn find(node: &WidgetRef, key: &str) -> Option<WidgetRef> {
    let children = {
        let g = node.lock().unwrap();
        if g.key() == Some(key) {
            return Some(node.clone());
        }
        g.get_children()
    };
    children.iter().find_map(|c| find(c, key))
}

/// How many children each keyed box ends up with, given helpers and the
/// call each box makes.
fn counts(helpers: &str, calls: &[&str]) -> Vec<usize> {
    let boxes: String = calls
        .iter()
        .enumerate()
        .map(|(i, call)| format!("Flex {{ key: \"b{i}\", children: {call} }},\n"))
        .collect();
    let src = format!(
        "{helpers}\nlet main = fn () {{ Flex {{ style: {{ direction: \"row\" }}, children: [\n{boxes}] }} }};"
    );
    let mut o = Ogham::from_source(&src, RuntimeConfig::default()).expect("from_source");
    for _ in 0..2 {
        o.frame(800.0, 600.0, 1.0 / 60.0).expect("frame");
    }
    (0..calls.len())
        .map(|i| {
            find(&o.get_ui().root, &format!("b{i}"))
                .expect("the box")
                .lock()
                .unwrap()
                .get_children()
                .len()
        })
        .collect()
}

#[test]
fn a_helper_ending_in_a_for_returns_every_element() {
    let got = counts(
        r#"
let tail = fn () { for (i in 0..5) { Flex {} } };
let bound = fn () { let out = for (i in 0..5) { Flex {} }; out };
let param = fn (n: int) { for (i in 0..n) { Flex {} } };
let after_lets = fn () { let n = 4; let m = n + 1; for (i in 0..m) { Flex {} } };
"#,
        &["tail()", "bound()", "param(5)", "after_lets()"],
    );
    assert_eq!(got, [5, 5, 5, 5]);
}

/// Only the loop that *ends* the block is its value: one followed by
/// another statement is still a loop, and the block's value is what ends
/// it.
#[test]
fn a_for_that_is_not_last_is_still_a_loop() {
    let got = counts(
        r#"
let not_last = fn () { for (i in 0..5) { Flex {} } [Flex {}, Flex {}] };
"#,
        &["not_last()"],
    );
    assert_eq!(got, [2]);
}

/// **A loop's body is not where its function ends.** A body whose last
/// line is an expression — `n++` here, `event(…)` in a handler — compiled
/// as a return out of the *enclosing function* on the first iteration, so
/// the loop ran once. The counter proves every iteration ran: five bumps,
/// five rows; the old reading returned `0` from the first `n++`.
#[test]
fn a_loop_body_ending_in_an_expression_runs_every_iteration() {
    let got = counts(
        r#"
let counted = fn () { let n = 0; for (i in 0..5) { n++ } for (j in 0..n) { Flex {} } };
"#,
        &["counted()"],
    );
    assert_eq!(got, [5]);
}

/// **A `return` somebody wrote still leaves the function from a loop** —
/// only the parser's implicit one is the iteration's value. The search
/// stops at the first match, so the two readings disagree: dropped, the
/// loop would run on and the function would return the trailing array.
#[test]
fn a_written_return_in_a_loop_leaves_the_function() {
    let got = counts(
        r#"
let first = fn () { for (i in 0..5) { return [Flex {}, Flex {}, Flex {}]; } [Flex {}] };
"#,
        &["first()"],
    );
    assert_eq!(got, [3]);
}
