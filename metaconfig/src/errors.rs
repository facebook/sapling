// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! Definition of errors used in this crate by the error_chain crate

pub use failure::{Error, Result};

/// Types of errors we can raise
#[derive(Debug, Fail)]
pub enum ErrorKind {
    /// The given bookmark does not exist in the repo
    #[fail(display = "bookmark not found: {}", _0)]
    BookmarkNotFound(String),
    /// The structure of metaconfig repo is invalid
    #[fail(display = "invalid file structure: {}", _0)]
    InvalidFileStructure(String),
}
