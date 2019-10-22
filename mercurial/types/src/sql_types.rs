/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

// Definitions for interfacing with SQL data stores

use sql::mysql_async::{
    from_value_opt,
    prelude::{ConvIr, FromValue},
    FromValueError, Value,
};

use crate::{Globalrev, HgChangesetId, HgFileNodeId, HgNodeHash};

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

impl From<Globalrev> for Value {
    fn from(globalrev: Globalrev) -> Self {
        Value::UInt(globalrev.id())
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

impl ConvIr<Globalrev> for Globalrev {
    fn new(v: Value) -> FromValueResult<Self> {
        Ok(Globalrev::new(from_value_opt(v)?))
    }

    fn commit(self) -> Self {
        self
    }

    fn rollback(self) -> Value {
        self.into()
    }
}

impl FromValue for HgFileNodeId {
    type Intermediate = HgNodeHash;
}

impl FromValue for HgChangesetId {
    type Intermediate = HgNodeHash;
}

impl FromValue for Globalrev {
    type Intermediate = Globalrev;
}
