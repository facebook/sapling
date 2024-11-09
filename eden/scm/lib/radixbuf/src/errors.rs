/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use thiserror::Error;

use crate::key::KeyId;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ErrorKind {
    #[error("offset {0} is out of range")]
    OffsetOverflow(u64),
    #[error("ambiguous prefix")]
    AmbiguousPrefix,
    #[error("{0:?} cannot be a prefix of {1:?}")]
    PrefixConflict(KeyId, KeyId),
    #[error("{0:?} cannot be resolved")]
    InvalidKeyId(KeyId),
    #[error("{0} is not a base16 value")]
    InvalidBase16(u8),
}
