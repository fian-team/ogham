//! `ogham` CLI entry point. Dispatches subcommands.
//!
//! See [`ogham::cli`] for the implementation. This file is the
//! `[[bin]]` entry; everything load-bearing lives in the lib so it
//! can be unit-tested.

use std::process::ExitCode;

use ogham::cli::{args, check};

fn main() -> ExitCode {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    let parsed = match args::parse(&raw) {
        Ok(parsed) => parsed,
        Err(args::ParseError::HelpRequested(text)) => {
            println!("{text}");
            return ExitCode::SUCCESS;
        }
        Err(err) => {
            eprintln!("ogham: {err}\n");
            eprintln!("{}", args::usage());
            return ExitCode::from(2);
        }
    };
    match parsed {
        args::Command::Check(check_args) => match check::run(check_args) {
            Ok(code) => ExitCode::from(code as u8),
            Err(err) => {
                eprintln!("ogham: {err}");
                ExitCode::from(2)
            }
        },
    }
}
