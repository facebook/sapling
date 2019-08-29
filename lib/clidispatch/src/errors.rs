// Copyright 2019 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.
use failure::Fail;
use std::borrow::Cow;

#[derive(Debug, Fail)]
#[fail(display = "cannot decode arguments")]
pub struct NonUTF8Arguments;

pub use cliparser::errors::InvalidArguments;

#[derive(Debug, Fail)]
#[fail(display = "unknown command '{}'\n(use 'hg help' to get help)", _0)]
pub struct UnknownCommand(pub String);

/// Explicitly fallback to Python code path.
///
/// Ideally this does not exist.
#[derive(Debug, Fail)]
#[fail(display = "")]
pub struct FallbackToPython;

#[derive(Debug, Fail)]
#[fail(display = "no repository found in '{}' (.hg not found)!", _0)]
pub struct RepoRequired(pub String);

#[derive(Debug, Fail)]
#[fail(display = "repository {} not found!", _0)]
pub struct RepoNotFound(pub String);

#[derive(Debug, Fail)]
#[fail(display = ".hg/sharedpath points to nonexistent directory {}!", _0)]
pub struct InvalidSharedPath(pub String);

#[derive(Debug, Fail)]
#[fail(
    display = "malformed --config option: '{}' (use --config section.name=value)",
    _0
)]
pub struct MalformedConfigOption(pub String);

#[derive(Debug, Fail)]
#[fail(display = "{}", _0)]
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
