/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use filestore::FetchKey;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ErrorKind {
    #[error("Could not locate content: {0:?}")]
    ContentMissing(FetchKey),
    #[error("Tree Derivation Failed")]
    TreeDerivationFailed,
    #[error("Invalid Thrift")]
    InvalidThrift,
}
