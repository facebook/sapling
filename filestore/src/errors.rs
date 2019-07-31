// Copyright (c) 2019-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use failure_ext::Fail;
use std::fmt::Debug;

use crate::expected_size::ExpectedSize;
use mononoke_types::{
    hash::{GitSha1, Sha1, Sha256},
    ContentId,
};

#[derive(Debug)]
pub struct InvalidHash<T: Debug> {
    pub expected: T,
    pub effective: T,
}

#[derive(Debug, Fail)]
pub enum ErrorKind {
    #[fail(display = "Invalid size: {:?} was expected, {:?} was observed", _0, _1)]
    InvalidSize(ExpectedSize, u64),

    #[fail(display = "Invalid ContentId: {:?}", _0)]
    InvalidContentId(InvalidHash<ContentId>),

    #[fail(display = "Invalid Sha1: {:?}", _0)]
    InvalidSha1(InvalidHash<Sha1>),

    #[fail(display = "Invalid Sha256: {:?}", _0)]
    InvalidSha256(InvalidHash<Sha256>),

    #[fail(display = "Invalid GitSha1: {:?}", _0)]
    InvalidGitSha1(InvalidHash<GitSha1>),
}
