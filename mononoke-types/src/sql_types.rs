// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use hash::Blake2;
use sql::mysql_async::{FromValueError, Value, prelude::{ConvIr, FromValue}};

use typed_hash::ChangesetId;

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
