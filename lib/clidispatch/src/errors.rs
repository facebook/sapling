// Copyright 2019 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.
use failure::Fail;
use std::io;

/// Finely-grained enumerations of errors that may happen during command dispatching.
#[derive(Debug, Fail)]
pub enum DispatchError {
    #[fail(display = "command alias expansion failed")]
    AliasExpansionFailed,
    #[fail(display = "command handler failed")]
    CommandFailed,
    #[fail(display = "config value was incorrectly written")]
    ConfigIssue,
    #[fail(display = "early parse failed to continue")]
    EarlyParseFailed,
    #[fail(display = "--help flag not supported in Rust")]
    HelpFlagNotSupported,
    #[fail(display = "IO error occurred while dispatching command")]
    IODispatchError,
    #[fail(
        display = "hg {}: invalid arguments\n(use 'hg {} -h' to get help)",
        command_name, command_name
    )]
    InvalidArguments { command_name: String },
    #[fail(display = "command line arguments were not UTF-8")]
    InvalidCommandLineArguments,
    #[fail(display = "invalid config encoding")]
    InvalidConfigEncoding,
    #[fail(display = "no command name was found")]
    NoCommandFound,
    #[fail(display = "parse failed to continue")]
    ParseFailed,
    #[fail(display = "--profile flag not supported in Rust")]
    ProfileFlagNotSupported,
    #[fail(display = "Programmer error occured:  {}", root_cause)]
    ProgrammingError { root_cause: String },
    #[fail(display = "abort: repository {} not found!", path)]
    RepoNotFound { path: String },
    #[fail(display = "abort: no repository found in '{}' (.hg not found)!", cwd)]
    RepoRequired { cwd: String },
    #[fail(
        display = "abort: .hg/sharedpath points to nonexistent directory {}!",
        path
    )]
    SharedPathNotReal { path: String },
}

/// Coarsely-grained enumerations of the types of errors that may happen during command dispatch.
/// Some `DispatchError` variants may immediately end the program, typically in situations where
/// there is no chance for the Python code to have a better chance at handling the command.
///
/// HighLevelError::SupportedError is the type that signals the execution *will not* fallback to the
/// Python codepath.  An example of this would be `DispatchError::InvalidArguments` which will not
/// change regardless of whether Rust or Python returns the error message.
///
/// HighLevelError::UnsupportedError is the type that signals the execution *will* fallback to the
/// Python codepath.  An example of this would be `DispatchError::HelpFlagNotSupported`.  There is
/// no help command in Rust that could possibly be called with a `--help` flag, meaning that the
/// Rust will have to fallback to the Python code to satisfy the user's request.
#[derive(Debug, Fail)]
pub(crate) enum HighLevelError {
    #[fail(display = "{}", cause)]
    SupportedError { cause: DispatchError },
    #[fail(display = "{}", cause)]
    UnsupportedError { cause: DispatchError },
}

impl From<io::Error> for DispatchError {
    fn from(_err: io::Error) -> Self {
        DispatchError::IODispatchError {}
    }
}

impl From<DispatchError> for HighLevelError {
    fn from(err: DispatchError) -> Self {
        match err {
            DispatchError::AliasExpansionFailed
            | DispatchError::ConfigIssue
            | DispatchError::EarlyParseFailed
            | DispatchError::HelpFlagNotSupported
            | DispatchError::IODispatchError
            | DispatchError::InvalidCommandLineArguments
            | DispatchError::InvalidConfigEncoding
            | DispatchError::NoCommandFound
            | DispatchError::ParseFailed
            | DispatchError::ProgrammingError { .. } => {
                HighLevelError::UnsupportedError { cause: err }
            }
            DispatchError::CommandFailed
            | DispatchError::InvalidArguments { .. }
            | DispatchError::ProfileFlagNotSupported
            | DispatchError::RepoNotFound { .. }
            | DispatchError::RepoRequired { .. }
            | DispatchError::SharedPathNotReal { .. } => {
                HighLevelError::SupportedError { cause: err }
            }
        }
    }
}
