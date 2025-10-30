mod files;
mod formatter;

use std::path::PathBuf;
use std::process;

use anyhow::{Context, Result, anyhow};
use clap::Parser;
use formatter::{FileChange, Formatter};

#[derive(Parser, Debug)]
#[command(
    name = "sqlx-inline-fmt",
    about = "Format inline sqlx query strings in Rust using an external SQL formatter",
    version
)]
struct Args {
    /// Files or directories to scan. If omitted, defaults to ./src and ./tests
    inputs: Vec<PathBuf>,

    /// Only report files that would change; do not write
    #[arg(long)]
    check: bool,

    /// Print only on error
    #[arg(short, long)]
    quiet: bool,

    /// Print per-file status for unchanged files
    #[arg(short, long)]
    verbose: bool,

    /// Formatter command to run
    #[arg(long = "formatter", value_name = "COMMAND")]
    formatter: String,

    /// Ignore non-zero exit codes from the formatter command
    #[arg(long)]
    ignore_exit_code: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Mode {
    Check,
    Write,
}

impl Mode {
    fn is_check(self) -> bool {
        matches!(self, Self::Check)
    }
}

impl From<bool> for Mode {
    fn from(check: bool) -> Self {
        if check { Self::Check } else { Self::Write }
    }
}

fn parse_formatter_command(s: &str) -> Result<Vec<String>> {
    let parts =
        shlex::split(s).ok_or_else(|| anyhow!("failed to parse formatter command `{s}`"))?;

    anyhow::ensure!(!parts.is_empty(), "formatter command cannot be empty");

    Ok(parts)
}

fn main() -> Result<()> {
    let args = Args::parse();
    let formatter_command = parse_formatter_command(&args.formatter)?;

    let files = files::gather_files(&args.inputs).context("gathering rust files")?;
    if files.is_empty() {
        if !args.quiet {
            eprintln!("No .rs files found");
        }

        return Ok(());
    }

    let mut formatter = Formatter::new(formatter_command, args.ignore_exit_code)?;
    let mode: Mode = args.check.into();
    let mut any_changes = false;

    for path in files {
        match formatter.format_path(&path, mode) {
            Ok(change) => {
                any_changes |= change.did_change();
                if !args.quiet {
                    match change {
                        FileChange::Unchanged if args.verbose => {
                            println!("{}: ok", path.display());
                        }
                        FileChange::Unchanged => {}
                        FileChange::WouldChange => {
                            println!("{}: would change", path.display());
                        }
                        FileChange::Changed => {
                            println!("{}: changed", path.display());
                        }
                    }
                }
            }
            Err(err) => {
                eprintln!("{}: {err:#}", path.display());
                process::exit(3);
            }
        }
    }

    if formatter.had_errors() {
        process::exit(2);
    }

    if mode.is_check() && any_changes {
        process::exit(1);
    }

    Ok(())
}
