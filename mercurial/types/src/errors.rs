/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

pub use failure::Fail;
pub use failure_ext::{Error, ResultExt};

#[derive(Debug, Fail)]
pub enum ErrorKind {
    #[fail(display = "invalid sha-1 input: {}", _0)]
    InvalidSha1Input(String),
    #[fail(display = "invalid fragment list: {}", _0)]
    InvalidFragmentList(String),
    #[fail(display = "invalid Thrift structure '{}': {}", _0, _1)]
    InvalidThrift(String, String),
    #[fail(display = "error while deserializing blob for '{}'", _0)]
    BlobDeserializeError(String),
    #[fail(display = "imposssible to parse unknown rev flags")]
    UnknownRevFlags,
}

pub type Result<T> = ::std::result::Result<T, Error>;
