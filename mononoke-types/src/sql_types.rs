// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::io::Write;

use diesel::backend::Backend;
use diesel::deserialize::{self, FromSql};
use diesel::serialize::{self, IsNull, Output, ToSql};
use diesel::sql_types::Binary;
use failure::ResultExt;
use hash::Blake2;
use sql::mysql_async::{FromValueError, Value, prelude::{ConvIr, FromValue}};

use typed_hash::ChangesetId;

type FromValueResult<T> = ::std::result::Result<T, FromValueError>;

#[derive(QueryId, SqlType)]
#[mysql_type = "Blob"]
#[sqlite_type = "Binary"]
pub struct ChangesetIdSql;

impl<DB: Backend> ToSql<ChangesetIdSql, DB> for ChangesetId {
    fn to_sql<W: Write>(&self, out: &mut Output<W, DB>) -> serialize::Result {
        out.write_all(self.as_ref())?;
        Ok(IsNull::No)
    }
}

impl<DB: Backend> FromSql<ChangesetIdSql, DB> for ChangesetId
where
    *const [u8]: FromSql<Binary, DB>,
{
    fn from_sql(bytes: Option<&DB::RawValue>) -> deserialize::Result<Self> {
        let raw_bytes: *const [u8] = FromSql::<Binary, DB>::from_sql(bytes)?;
        let raw_bytes: &[u8] = unsafe { &*raw_bytes };
        Ok(ChangesetId::from_bytes(raw_bytes).compat()?)
    }
}

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
