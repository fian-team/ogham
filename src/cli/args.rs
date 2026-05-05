//! Hand-rolled argument parser for the `ogham` CLI.
//!
//! Kept dependency-free (no clap) because the surface is small —
//! one subcommand, four flags. The trade-off: we don't get
//! auto-generated help. The hand-rolled `usage()` strings stay in
//! sync because they're co-located with the parser.

use std::path::PathBuf;

/// Top-level parsed command.
#[derive(Debug, PartialEq)]
pub enum Command {
    Check(CheckArgs),
}

#[derive(Debug, PartialEq)]
pub struct CheckArgs {
    /// Path to a single `.ogh` file. Mutually exclusive with `all`.
    pub path: Option<PathBuf>,
    /// Walk cwd recursively for every `.ogh` file.
    pub all: bool,
    /// Override the workspace root (otherwise inferred via cargo
    /// metadata from cwd).
    pub workspace: Option<PathBuf>,
    /// Suppress staleness warnings (when manifest mtime is older
    /// than the Rust source it points at).
    pub no_staleness_check: bool,
}

#[derive(Debug)]
pub enum ParseError {
    /// `--help` was passed; the caller should print this text and
    /// exit 0 rather than treating it as an error.
    HelpRequested(String),
    /// Unknown subcommand or missing required argument.
    Usage(String),
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseError::HelpRequested(s) | ParseError::Usage(s) => f.write_str(s),
        }
    }
}

impl std::error::Error for ParseError {}

/// Top-level parse. The first arg is the subcommand name; the
/// remainder are subcommand-specific.
pub fn parse(args: &[String]) -> Result<Command, ParseError> {
    let mut iter = args.iter();
    let Some(first) = iter.next() else {
        return Err(ParseError::Usage(
            "missing subcommand (try `ogham --help`)".into(),
        ));
    };
    match first.as_str() {
        "--help" | "-h" | "help" => Err(ParseError::HelpRequested(usage())),
        "check" => {
            let rest: Vec<String> = iter.cloned().collect();
            parse_check(&rest).map(Command::Check)
        }
        other => Err(ParseError::Usage(format!("unknown subcommand `{other}`"))),
    }
}

/// Parse `check`'s flags. State machine: walks args once, sets
/// fields, errors on conflicts.
pub fn parse_check(args: &[String]) -> Result<CheckArgs, ParseError> {
    let mut path: Option<PathBuf> = None;
    let mut all = false;
    let mut workspace: Option<PathBuf> = None;
    let mut no_staleness_check = false;

    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--help" | "-h" => return Err(ParseError::HelpRequested(check_usage())),
            "--all" => all = true,
            "--no-staleness-check" => no_staleness_check = true,
            "--workspace" => {
                let value = iter.next().ok_or_else(|| {
                    ParseError::Usage("`--workspace` requires a directory argument".into())
                })?;
                workspace = Some(PathBuf::from(value));
            }
            // Any other `--…` is unknown.
            s if s.starts_with("--") => {
                return Err(ParseError::Usage(format!("unknown flag `{s}`")));
            }
            // Positional path; only one allowed.
            other => {
                if path.is_some() {
                    return Err(ParseError::Usage(
                        "more than one positional path given; pass exactly one or use `--all`".into(),
                    ));
                }
                path = Some(PathBuf::from(other));
            }
        }
    }

    if all && path.is_some() {
        return Err(ParseError::Usage(
            "`--all` and a positional path are mutually exclusive".into(),
        ));
    }
    if !all && path.is_none() {
        return Err(ParseError::Usage(
            "`ogham check` requires either a positional `.ogh` path or `--all`".into(),
        ));
    }

    Ok(CheckArgs {
        path,
        all,
        workspace,
        no_staleness_check,
    })
}

pub fn usage() -> String {
    "\
Usage: ogham <SUBCOMMAND> [ARGS...]

Subcommands:
  check    Run schema-diagnostic checks against `.ogh` files

Run `ogham <SUBCOMMAND> --help` for subcommand-specific options."
        .into()
}

pub fn check_usage() -> String {
    "\
Usage: ogham check <PATH> [OPTIONS]
       ogham check --all   [OPTIONS]

Run schema-diagnostic checks against one or more `.ogh` files,
matching them against Rust-side binding manifests emitted by
`#[derive(OghamState)]` / `#[derive(OghamMsg)]` at compile time.

Options:
  --all                    Walk cwd recursively; check every `.ogh`
  --workspace <DIR>        Override the workspace root (default: cwd)
  --no-staleness-check     Suppress warnings about stale manifests
  -h, --help               Show this message

Exit codes:
  0   No ERROR diagnostics (warnings/infos OK)
  1   At least one ERROR diagnostic
  2   Usage error or IO failure"
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(args: &[&str]) -> Vec<String> {
        args.iter().map(|a| (*a).to_string()).collect()
    }

    #[test]
    fn no_args_is_usage_error() {
        assert!(matches!(parse(&[]), Err(ParseError::Usage(_))));
    }

    #[test]
    fn unknown_subcommand_is_usage_error() {
        let err = parse(&s(&["frob"]));
        assert!(matches!(err, Err(ParseError::Usage(_))));
    }

    #[test]
    fn top_level_help_is_help_requested() {
        assert!(matches!(parse(&s(&["--help"])), Err(ParseError::HelpRequested(_))));
        assert!(matches!(parse(&s(&["-h"])), Err(ParseError::HelpRequested(_))));
        assert!(matches!(parse(&s(&["help"])), Err(ParseError::HelpRequested(_))));
    }

    #[test]
    fn check_with_path_only() {
        let cmd = parse(&s(&["check", "data/ui.ogh"])).unwrap();
        let Command::Check(args) = cmd;
        assert_eq!(args.path.unwrap(), PathBuf::from("data/ui.ogh"));
        assert!(!args.all);
        assert_eq!(args.workspace, None);
        assert!(!args.no_staleness_check);
    }

    #[test]
    fn check_with_all_flag() {
        let cmd = parse(&s(&["check", "--all"])).unwrap();
        let Command::Check(args) = cmd;
        assert!(args.all);
        assert!(args.path.is_none());
    }

    #[test]
    fn check_with_workspace() {
        let cmd = parse(&s(&["check", "x.ogh", "--workspace", "/path/to/ws"])).unwrap();
        let Command::Check(args) = cmd;
        assert_eq!(args.workspace.unwrap(), PathBuf::from("/path/to/ws"));
    }

    #[test]
    fn check_with_no_staleness_check() {
        let cmd = parse(&s(&["check", "x.ogh", "--no-staleness-check"])).unwrap();
        let Command::Check(args) = cmd;
        assert!(args.no_staleness_check);
    }

    #[test]
    fn check_all_and_path_is_usage_error() {
        let err = parse(&s(&["check", "x.ogh", "--all"]));
        assert!(matches!(err, Err(ParseError::Usage(_))));
    }

    #[test]
    fn check_neither_all_nor_path_is_usage_error() {
        let err = parse(&s(&["check"]));
        assert!(matches!(err, Err(ParseError::Usage(_))));
    }

    #[test]
    fn check_two_positionals_is_usage_error() {
        let err = parse(&s(&["check", "a.ogh", "b.ogh"]));
        assert!(matches!(err, Err(ParseError::Usage(_))));
    }

    #[test]
    fn check_unknown_flag_is_usage_error() {
        let err = parse(&s(&["check", "x.ogh", "--frob"]));
        assert!(matches!(err, Err(ParseError::Usage(_))));
    }

    #[test]
    fn check_workspace_without_value_is_usage_error() {
        let err = parse(&s(&["check", "x.ogh", "--workspace"]));
        assert!(matches!(err, Err(ParseError::Usage(_))));
    }

    #[test]
    fn check_help_is_help_requested() {
        assert!(matches!(
            parse(&s(&["check", "--help"])),
            Err(ParseError::HelpRequested(_))
        ));
    }

    #[test]
    fn check_path_after_flags() {
        // --workspace consumes its value; subsequent positional is
        // the path.
        let cmd = parse(&s(&["check", "--workspace", "/ws", "x.ogh"])).unwrap();
        let Command::Check(args) = cmd;
        assert_eq!(args.path.unwrap(), PathBuf::from("x.ogh"));
        assert_eq!(args.workspace.unwrap(), PathBuf::from("/ws"));
    }
}
