// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

pub use failure_ext::{Error, Fail, Result};

#[derive(Debug, Fail)]
pub enum ErrorKind {
    #[fail(display = "invalid manifest description: {}", _0)]
    InvalidManifestDescription(String),
    #[fail(display = "invalid path map: {}", _0)]
    InvalidPathMap(String),
    #[fail(display = "invalid directory hash map: {}", _0)]
    InvalidDirectoryHashes(String),
}
