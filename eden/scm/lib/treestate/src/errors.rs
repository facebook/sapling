/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Errors.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum ErrorKind {
    #[error("the provided store file is not a valid store file")]
    NotAStoreFile,
    #[error("tree version not supported: {0}")]
    UnsupportedTreeVersion(u32),
    #[error("store file version not supported: {0}")]
    UnsupportedVersion(u32),
    #[error("invalid store id: {0}")]
    InvalidStoreId(u64),
    #[error("store is read-only")]
    ReadOnlyStore,
    #[error("treedirstate is corrupt")]
    CorruptTree,
    #[error("callback error: {0}")]
    CallbackError(String),
}
