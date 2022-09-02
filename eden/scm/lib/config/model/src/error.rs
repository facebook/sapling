/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::ffi::CString;
use std::fmt;
use std::io;
use std::path::PathBuf;
use std::str;

use thiserror::Error;

/// The error type for parsing config files.
#[derive(Error, Debug)]
pub enum Error {
    /// Unable to convert to a type.
    #[error("{0}")]
    Convert(String),

    /// Unable to parse a file due to syntax.
    #[error("{0:?}:\n{1}")]
    ParseFile(PathBuf, String),

    /// Unable to parse a flag due to syntax.
    #[error("malformed --config option: '{0}' (use --config section.name=value)")]
    ParseFlag(String),

    /// Unable to read a file due to IO errors.
    #[error("{0:?}: {1}")]
    Io(PathBuf, #[source] io::Error),

    /// Config file contains invalid UTF-8.
    #[error("{0:?}: {1}")]
    Utf8(PathBuf, #[source] str::Utf8Error),

    #[error("{0:?}: {1}")]
    Utf8Path(CString, #[source] str::Utf8Error),

    #[error(transparent)]
    ParseInt(#[from] std::num::ParseIntError),

    #[error(transparent)]
    ParseFloat(#[from] std::num::ParseFloatError),

    #[error("{0}")]
    General(String),

    #[error("{0}")]
    Other(#[source] anyhow::Error),
}

impl From<String> for Error {
    fn from(s: String) -> Self {
        Self::General(s)
    }
}

#[derive(Error, Debug)]
pub struct Errors(pub Vec<Error>);

impl fmt::Display for Errors {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        for error in self.0.iter() {
            write!(f, "{}\n", error)?;
        }
        Ok(())
    }
}
