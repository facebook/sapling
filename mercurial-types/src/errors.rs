// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

pub use failure::{Error, ResultExt};

use MPath;

#[derive(Debug, Fail)]
pub enum ErrorKind {
    #[fail(display = "invalid sha-1 input: {}", _0)] InvalidSha1Input(String),
    #[fail(display = "invalid path '{}': {}", _0, _1)] InvalidPath(String, String),
    #[fail(display = "invalid Mercurial path '{}': {}", _0, _1)] InvalidMPath(MPath, String),
    #[fail(display = "invalid fragment list: {}", _0)] InvalidFragmentList(String),
}

pub type Result<T> = ::std::result::Result<T, Error>;
