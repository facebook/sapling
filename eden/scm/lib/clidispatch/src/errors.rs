/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::borrow::Cow;
use thiserror::Error;

#[derive(Debug, Error)]
#[error("cannot decode arguments")]
pub struct NonUTF8Arguments;

pub use cliparser::errors::InvalidArguments;

#[derive(Debug, Error)]
#[error("unknown command '{0}'\n(use 'hg help' to get help)")]
pub struct UnknownCommand(pub String);

/// Explicitly fallback to Python code path.
///
/// Ideally this does not exist.
#[derive(Debug, Error)]
#[error("")]
pub struct FallbackToPython;

#[derive(Debug, Error)]
#[error("'{0}' is not inside a repository, but this command requires a repository!\n(use 'cd' to go to a directory inside a repository and try again)")]
pub struct RepoRequired(pub String);

#[derive(Debug, Error)]
#[error("repository {0} not found!")]
pub struct RepoNotFound(pub String);

#[derive(Debug, Error)]
#[error(".hg/sharedpath points to nonexistent directory {0}!")]
pub struct InvalidSharedPath(pub String);

#[derive(Debug, Error)]
#[error("malformed --config option: '{0}' (use --config section.name=value)")]
pub struct MalformedConfigOption(pub String);

#[derive(Debug, Error)]
#[error("{0}")]
pub struct Abort(pub Cow<'static, str>);

/// Print an error suitable for end-user consumption.
///
/// This function adds `hg:` or `abort:` to error messages.
pub fn print_error(err: &failure::Error, io: &mut crate::io::IO) {
    use cliparser::parser::ParseError;
    if err.downcast_ref::<configparser::Error>().is_some() {
        let _ = io.write_err(format!("hg: parse error: {}\n", err));
    } else if let Some(ParseError::AmbiguousCommand {
        command_name: _,
        possibilities,
    }) = err.downcast_ref::<ParseError>()
    {
        let _ = io.write_err(format!("hg: {}:\n", err));
        for possibility in possibilities {
            // UX: Colorize the output once `io` can output colors.
            let _ = io.write_err(format!("     {}\n", possibility));
        }
    } else {
        let _ = io.write_err(format!("abort: {}\n", err));
    }
}
