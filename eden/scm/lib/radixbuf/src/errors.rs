/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use thiserror::Error;

use crate::key::KeyId;

#[derive(Debug, Error)]
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
