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

use typed_hash::ChangesetId;

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
