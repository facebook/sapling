/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Errors.

use failure::Fail;

#[derive(Debug, Fail)]
pub enum ErrorKind {
    #[fail(display = "the provided store file is not a valid store file")]
    NotAStoreFile,
    #[fail(display = "tree version not supported: {}", _0)]
    UnsupportedTreeVersion(u32),
    #[fail(display = "store file version not supported: {}", _0)]
    UnsupportedVersion(u32),
    #[fail(display = "invalid store id: {}", _0)]
    InvalidStoreId(u64),
    #[fail(display = "store is read-only")]
    ReadOnlyStore,
    #[fail(display = "treedirstate is corrupt")]
    CorruptTree,
    #[fail(display = "callback error: {}", _0)]
    CallbackError(String),
}
