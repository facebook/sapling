// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use crate::path::MPath;
pub use failure_ext::{Error, Fail, ResultExt};

#[derive(Debug, Fail)]
pub enum ErrorKind {
    #[fail(display = "invalid blake2 input: {}", _0)]
    InvalidBlake2Input(String),
    #[fail(display = "invalid sha256 input: {}", _0)]
    InvalidSha1Input(String),
    #[fail(display = "invalid sha1 input: {}", _0)]
    InvalidSha256Input(String),
    #[fail(display = "invalid path '{}': {}", _0, _1)]
    InvalidPath(String, String),
    #[fail(display = "invalid Mononoke path '{}': {}", _0, _1)]
    InvalidMPath(MPath, String),
    #[fail(display = "error while deserializing blob for '{}'", _0)]
    BlobDeserializeError(String),
    #[fail(display = "invalid Thrift structure '{}': {}", _0, _1)]
    InvalidThrift(String, String),
    #[fail(display = "invalid changeset date: {}", _0)]
    InvalidDateTime(String),
    #[fail(
        display = "not path-conflict-free: changed path '{}' is a prefix of '{}'",
        _0, _1
    )]
    NotPathConflictFree(MPath, MPath),
    #[fail(display = "invalid bonsai changeset: {}", _0)]
    InvalidBonsaiChangeset(String),
}

pub type Result<T> = ::std::result::Result<T, Error>;
