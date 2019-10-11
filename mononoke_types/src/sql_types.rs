/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use crate::datetime::Timestamp;
use crate::hash::Blake2;
use crate::repo::RepositoryId;
use crate::typed_hash::ChangesetId;
use sql::mysql_async::{
    from_value_opt,
    prelude::{ConvIr, FromValue},
    FromValueError, Value,
};

type FromValueResult<T> = ::std::result::Result<T, FromValueError>;

impl From<ChangesetId> for Value {
    fn from(id: ChangesetId) -> Self {
        Value::Bytes(id.as_ref().into())
    }
}

impl ConvIr<ChangesetId> for Blake2 {
    fn new(v: Value) -> FromValueResult<Self> {
        match v {
            Value::Bytes(bytes) => {
                Blake2::from_bytes(&bytes).map_err(move |_| FromValueError(Value::Bytes(bytes)))
            }
            v => Err(FromValueError(v)),
        }
    }

    fn commit(self) -> ChangesetId {
        ChangesetId::new(self)
    }

    fn rollback(self) -> Value {
        Value::Bytes(self.as_ref().into())
    }
}

impl FromValue for ChangesetId {
    type Intermediate = Blake2;
}

impl From<Timestamp> for Value {
    fn from(ts: Timestamp) -> Self {
        Value::Int(ts.timestamp_nanos())
    }
}

impl ConvIr<Timestamp> for Timestamp {
    fn new(v: Value) -> FromValueResult<Self> {
        Ok(Timestamp::from_timestamp_nanos(from_value_opt(v)?))
    }

    fn commit(self) -> Self {
        self
    }

    fn rollback(self) -> Value {
        self.into()
    }
}

impl FromValue for Timestamp {
    type Intermediate = Timestamp;
}

impl From<RepositoryId> for Value {
    fn from(repo_id: RepositoryId) -> Self {
        Value::UInt(repo_id.id() as u64)
    }
}

impl ConvIr<RepositoryId> for RepositoryId {
    fn new(v: Value) -> FromValueResult<Self> {
        Ok(RepositoryId::new(from_value_opt(v)?))
    }

    fn commit(self) -> Self {
        self
    }

    fn rollback(self) -> Value {
        self.into()
    }
}

impl FromValue for RepositoryId {
    type Intermediate = RepositoryId;
}
