// Copyright 2017 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use failure::Fail;

use crate::key::KeyId;

#[derive(Debug, Fail)]
pub enum ErrorKind {
    #[fail(display = "offset {} is out of range", _0)]
    OffsetOverflow(u64),
    #[fail(display = "ambiguous prefix")]
    AmbiguousPrefix,
    #[fail(display = "{:?} cannot be a prefix of {:?}", _0, _1)]
    PrefixConflict(KeyId, KeyId),
    #[fail(display = "{:?} cannot be resolved", _0)]
    InvalidKeyId(KeyId),
    #[fail(display = "{} is not a base16 value", _0)]
    InvalidBase16(u8),
}
