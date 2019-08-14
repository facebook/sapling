// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

// Definitions for interfacing with SQL data stores

use sql::mysql_async::{
    prelude::{ConvIr, FromValue},
    FromValueError, Value,
};

use crate::{HgChangesetId, HgFileNodeId, HgNodeHash};

type FromValueResult<T> = ::std::result::Result<T, FromValueError>;

impl From<HgFileNodeId> for Value {
    fn from(id: HgFileNodeId) -> Self {
        Value::Bytes(id.into_nodehash().0.as_ref().into())
    }
}

impl From<HgChangesetId> for Value {
    fn from(id: HgChangesetId) -> Self {
        Value::Bytes(id.into_nodehash().0.as_ref().into())
    }
}

pub trait FromNodeHash {
    fn from_nodehash(hash: HgNodeHash) -> Self;
}

impl FromNodeHash for HgFileNodeId {
    fn from_nodehash(hash: HgNodeHash) -> Self {
        HgFileNodeId::new(hash)
    }
}

impl FromNodeHash for HgChangesetId {
    fn from_nodehash(hash: HgNodeHash) -> Self {
        HgChangesetId::new(hash)
    }
}

impl<T: FromNodeHash> ConvIr<T> for HgNodeHash {
    fn new(v: Value) -> FromValueResult<Self> {
        match v {
            Value::Bytes(bytes) => {
                HgNodeHash::from_bytes(&bytes).map_err(move |_| FromValueError(Value::Bytes(bytes)))
            }
            v => Err(FromValueError(v)),
        }
    }

    fn commit(self) -> T {
        T::from_nodehash(self)
    }

    fn rollback(self) -> Value {
        Value::Bytes(self.0.as_ref().into())
    }
}

impl FromValue for HgFileNodeId {
    type Intermediate = HgNodeHash;
}

impl FromValue for HgChangesetId {
    type Intermediate = HgNodeHash;
}
