/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use std::fmt::Debug;
use thiserror::Error;

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

#[derive(Debug, Error)]
pub enum ErrorKind {
    #[error("Invalid size: {0:?} was expected, {1:?} was observed")]
    InvalidSize(ExpectedSize, u64),

    #[error("Invalid ContentId: {0:?}")]
    InvalidContentId(InvalidHash<ContentId>),

    #[error("Invalid Sha1: {0:?}")]
    InvalidSha1(InvalidHash<Sha1>),

    #[error("Invalid Sha256: {0:?}")]
    InvalidSha256(InvalidHash<Sha256>),

    #[error("Invalid GitSha1: {0:?}")]
    InvalidGitSha1(InvalidHash<GitSha1>),
}
