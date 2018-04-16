// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

// Definitions for interfacing with SQL data stores using the diesel library.

use std::io::Write;

use diesel::backend::Backend;
use diesel::deserialize::{self, FromSql};
use diesel::serialize::{self, IsNull, Output, ToSql};
use diesel::sql_types::{Binary, Integer};

use {DChangesetId, DManifestId, DNodeHash, HgFileNodeId, RepositoryId};
use errors::*;

#[derive(QueryId, SqlType)]
#[mysql_type = "Blob"]
#[sqlite_type = "Binary"]
pub struct DChangesetIdSql;

#[derive(QueryId, SqlType)]
#[mysql_type = "Blob"]
#[sqlite_type = "Binary"]
pub struct DManifestIdSql;

#[derive(QueryId, SqlType)]
#[mysql_type = "Blob"]
#[sqlite_type = "Binary"]
pub struct HgFileNodeIdSql;

impl<DB: Backend> ToSql<DChangesetIdSql, DB> for DChangesetId {
    fn to_sql<W: Write>(&self, out: &mut Output<W, DB>) -> serialize::Result {
        out.write_all(self.as_nodehash().0.as_ref())?;
        Ok(IsNull::No)
    }
}

impl<DB: Backend> FromSql<DChangesetIdSql, DB> for DChangesetId
where
    *const [u8]: FromSql<Binary, DB>,
{
    fn from_sql(bytes: Option<&DB::RawValue>) -> deserialize::Result<Self> {
        // Using unsafe here saves on a heap allocation. See https://goo.gl/K6hapb.
        let raw_bytes: *const [u8] = FromSql::<Binary, DB>::from_sql(bytes)?;
        let raw_bytes: &[u8] = unsafe { &*raw_bytes };
        let hash = DNodeHash::from_bytes(raw_bytes).compat()?;
        Ok(Self::new(hash))
    }
}

impl<DB: Backend> ToSql<DManifestIdSql, DB> for DManifestId {
    fn to_sql<W: Write>(&self, out: &mut Output<W, DB>) -> serialize::Result {
        out.write_all(self.as_nodehash().0.as_ref())?;
        Ok(IsNull::No)
    }
}

impl<DB: Backend> FromSql<DManifestIdSql, DB> for DManifestId
where
    *const [u8]: FromSql<Binary, DB>,
{
    fn from_sql(bytes: Option<&DB::RawValue>) -> deserialize::Result<Self> {
        // Using unsafe here saves on a heap allocation. See https://goo.gl/K6hapb.
        let raw_bytes: *const [u8] = FromSql::<Binary, DB>::from_sql(bytes)?;
        let raw_bytes: &[u8] = unsafe { &*raw_bytes };
        let hash = DNodeHash::from_bytes(raw_bytes).compat()?;
        Ok(Self::new(hash))
    }
}

impl<DB: Backend> ToSql<HgFileNodeIdSql, DB> for HgFileNodeId {
    fn to_sql<W: Write>(&self, out: &mut Output<W, DB>) -> serialize::Result {
        out.write_all(self.as_nodehash().0.as_ref())?;
        Ok(IsNull::No)
    }
}

impl<DB: Backend> FromSql<HgFileNodeIdSql, DB> for HgFileNodeId
where
    *const [u8]: FromSql<Binary, DB>,
{
    fn from_sql(bytes: Option<&DB::RawValue>) -> deserialize::Result<Self> {
        // Using unsafe here saves on a heap allocation. See https://goo.gl/K6hapb.
        let raw_bytes: *const [u8] = FromSql::<Binary, DB>::from_sql(bytes)?;
        let raw_bytes: &[u8] = unsafe { &*raw_bytes };
        let hash = DNodeHash::from_bytes(raw_bytes).compat()?;
        Ok(Self::new(hash))
    }
}

impl<DB: Backend> ToSql<Integer, DB> for RepositoryId
where
    i32: ToSql<Integer, DB>,
{
    fn to_sql<W: Write>(&self, out: &mut Output<W, DB>) -> serialize::Result {
        self.id().to_sql(out)
    }
}

impl<DB: Backend> FromSql<Integer, DB> for RepositoryId
where
    i32: FromSql<Integer, DB>,
{
    fn from_sql(bytes: Option<&DB::RawValue>) -> deserialize::Result<Self> {
        let val = FromSql::<Integer, DB>::from_sql(bytes)?;
        Ok(RepositoryId::new(val))
    }
}
