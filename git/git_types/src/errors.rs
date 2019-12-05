/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
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
