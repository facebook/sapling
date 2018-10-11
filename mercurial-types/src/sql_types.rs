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
use sql::mysql_async::{FromValueError, Value, prelude::{ConvIr, FromValue}};

use {HgChangesetId, HgFileNodeId, HgManifestId, HgNodeHash, RepositoryId};
use errors::*;

type FromValueResult<T> = ::std::result::Result<T, FromValueError>;

#[derive(QueryId, SqlType)]
#[mysql_type = "Blob"]
#[sqlite_type = "Binary"]
pub struct HgChangesetIdSql;

#[derive(QueryId, SqlType)]
#[mysql_type = "Blob"]
#[sqlite_type = "Binary"]
pub struct HgManifestIdSql;

#[derive(QueryId, SqlType)]
#[mysql_type = "Blob"]
#[sqlite_type = "Binary"]
pub struct HgFileNodeIdSql;

impl<DB: Backend> ToSql<HgChangesetIdSql, DB> for HgChangesetId {
    fn to_sql<W: Write>(&self, out: &mut Output<W, DB>) -> serialize::Result {
        out.write_all(self.as_nodehash().0.as_ref())?;
        Ok(IsNull::No)
    }
}

impl<DB: Backend> FromSql<HgChangesetIdSql, DB> for HgChangesetId
where
    *const [u8]: FromSql<Binary, DB>,
{
    fn from_sql(bytes: Option<&DB::RawValue>) -> deserialize::Result<Self> {
        // Using unsafe here saves on a heap allocation. See https://goo.gl/K6hapb.
        let raw_bytes: *const [u8] = FromSql::<Binary, DB>::from_sql(bytes)?;
        let raw_bytes: &[u8] = unsafe { &*raw_bytes };
        let hash = HgNodeHash::from_bytes(raw_bytes).compat()?;
        Ok(Self::new(hash))
    }
}

impl<DB: Backend> ToSql<HgManifestIdSql, DB> for HgManifestId {
    fn to_sql<W: Write>(&self, out: &mut Output<W, DB>) -> serialize::Result {
        out.write_all(self.as_nodehash().0.as_ref())?;
        Ok(IsNull::No)
    }
}

impl<DB: Backend> FromSql<HgManifestIdSql, DB> for HgManifestId
where
    *const [u8]: FromSql<Binary, DB>,
{
    fn from_sql(bytes: Option<&DB::RawValue>) -> deserialize::Result<Self> {
        // Using unsafe here saves on a heap allocation. See https://goo.gl/K6hapb.
        let raw_bytes: *const [u8] = FromSql::<Binary, DB>::from_sql(bytes)?;
        let raw_bytes: &[u8] = unsafe { &*raw_bytes };
        let hash = HgNodeHash::from_bytes(raw_bytes).compat()?;
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
        let hash = HgNodeHash::from_bytes(raw_bytes).compat()?;
        Ok(Self::new(hash))
    }
}

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
