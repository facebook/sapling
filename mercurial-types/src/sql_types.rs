// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

// Definitions for interfacing with SQL data stores

use sql::mysql_async::{FromValueError, Value, prelude::{ConvIr, FromValue}};

use {HgChangesetId, HgFileNodeId, HgNodeHash, RepositoryId};

type FromValueResult<T> = ::std::result::Result<T, FromValueError>;

impl From<HgFileNodeId> for Value {
    fn from(id: HgFileNodeId) -> Self {
        Value::Bytes(id.as_nodehash().0.as_ref().into())
    }
}

impl From<HgChangesetId> for Value {
    fn from(id: HgChangesetId) -> Self {
        Value::Bytes(id.as_nodehash().0.as_ref().into())
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

impl From<RepositoryId> for Value {
    fn from(repo_id: RepositoryId) -> Self {
        Value::UInt(repo_id.id() as u64)
    }
}

impl ConvIr<RepositoryId> for RepositoryId {
    fn new(v: Value) -> FromValueResult<Self> {
        match v {
            Value::UInt(id) => Ok(RepositoryId::new(id as i32)),
            v => Err(FromValueError(v)),
        }
    }

    fn commit(self) -> Self {
        self
    }

    fn rollback(self) -> Value {
        Value::UInt(self.id() as u64)
    }
}

impl FromValue for RepositoryId {
    type Intermediate = RepositoryId;
}
