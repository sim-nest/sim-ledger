//! Command line runner for yearly ledger sets.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

use std::io::Write;

mod args;
mod commands;
mod error;

#[cfg(test)]
mod tests;

/// Run the `ledger` command with caller-provided streams.
///
/// The returned integer is a process exit status: `0` for success, `1` for a
/// runtime error, and `2` for usage errors.
pub fn run<I, S>(args: I, out: &mut dyn Write, err: &mut dyn Write) -> i32
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let args = args.into_iter().map(Into::into).collect::<Vec<_>>();
    let command = match args::parse(args) {
        Ok(command) => command,
        Err(parse) => {
            let _ = writeln!(err, "usage error: {parse}");
            let _ = writeln!(err);
            let _ = writeln!(err, "{}", args::USAGE);
            return 2;
        }
    };

    match commands::execute(command, out) {
        Ok(()) => 0,
        Err(error) => {
            let _ = writeln!(err, "error: {error}");
            1
        }
    }
}
