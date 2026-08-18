//! `ogham-vocab` — read `.ogh` files and say what the language does not
//! recognise in them.
//!
//! The static half of [`ogham::widget::vocabulary`]: no host, no window,
//! no host state, so it runs over a whole repository's UI in a second and
//! names file, line and column. What it cannot see is a style map reached
//! through a `let`; that is the builder's half, which a host turns on
//! with `RuntimeConfig::with_strict_vocabulary`.
//!
//! ```text
//! ogham-vocab data/ui/*.ogh
//! ```
//!
//! Exits non-zero when it found something, so a repository can put it in
//! CI before it is ready to put strictness in its own tests.

use std::process::ExitCode;

fn main() -> ExitCode {
    let files: Vec<String> = std::env::args().skip(1).collect();
    if files.is_empty() {
        eprintln!("usage: ogham-vocab <file.ogh> [file.ogh ...]");
        return ExitCode::from(2);
    }

    let mut total = 0usize;
    for path in &files {
        let source = match std::fs::read_to_string(path) {
            Ok(source) => source,
            Err(err) => {
                eprintln!("{path}: {err}");
                return ExitCode::from(2);
            }
        };
        for violation in ogham::widget::vocabulary::scan_source(path, &source) {
            println!("{violation}");
            total += 1;
        }
    }

    match total {
        0 => {
            println!("{} file(s): nothing unrecognised", files.len());
            ExitCode::SUCCESS
        }
        n => {
            println!("\n{n} unrecognised key(s) and value(s) in {} file(s)", files.len());
            ExitCode::FAILURE
        }
    }
}
