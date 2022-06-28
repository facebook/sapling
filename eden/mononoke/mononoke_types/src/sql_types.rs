/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::datetime::Timestamp;
use crate::globalrev::Globalrev;
use crate::hash::Blake2;
use crate::hash::GitSha1;
use crate::repo::RepositoryId;
use crate::svnrev::Svnrev;
use crate::typed_hash::ChangesetId;
use sql::mysql_async::from_value_opt;
use sql::mysql_async::prelude::ConvIr;
use sql::mysql_async::prelude::FromValue;
use sql::mysql_async::FromValueError;
use sql::mysql_async::Value;
use sql::sql_common::mysql::opt_try_from_rowfield;
use sql::sql_common::mysql::OptionalTryFromRowField;
use sql::sql_common::mysql::RowField;
use sql::sql_common::mysql::ValueError;

type FromValueResult<T> = Result<T, FromValueError>;

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

impl From<GitSha1> for Value {
    fn from(sha1: GitSha1) -> Self {
        Value::Bytes(sha1.as_ref().into())
    }
}

impl ConvIr<GitSha1> for GitSha1 {
    fn new(v: Value) -> FromValueResult<Self> {
        match v {
            Value::Bytes(bytes) => {
                GitSha1::from_bytes(&bytes).map_err(move |_| FromValueError(Value::Bytes(bytes)))
            }
            v => Err(FromValueError(v)),
        }
    }

    fn commit(self) -> Self {
        self
    }

    fn rollback(self) -> Value {
        self.into()
    }
}

impl FromValue for GitSha1 {
    type Intermediate = GitSha1;
}

impl OptionalTryFromRowField for GitSha1 {
    fn try_from_opt(field: RowField) -> Result<Option<Self>, ValueError> {
        opt_try_from_rowfield(field)
    }
}

impl From<Globalrev> for Value {
    fn from(globalrev: Globalrev) -> Self {
        Value::UInt(globalrev.id())
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

impl FromValue for Globalrev {
    type Intermediate = Globalrev;
}

impl From<Svnrev> for Value {
    fn from(svnrev: Svnrev) -> Self {
        Value::UInt(svnrev.id())
    }
}

impl ConvIr<Svnrev> for Svnrev {
    fn new(v: Value) -> FromValueResult<Self> {
        Ok(Svnrev::new(from_value_opt(v)?))
    }

    fn commit(self) -> Self {
        self
    }

    fn rollback(self) -> Value {
        self.into()
    }
}

impl FromValue for Svnrev {
    type Intermediate = Svnrev;
}

impl From<Blake2> for Value {
    fn from(id: Blake2) -> Self {
        Value::Bytes(id.as_ref().into())
    }
}

impl ConvIr<Blake2> for Blake2 {
    fn new(v: Value) -> FromValueResult<Self> {
        match v {
            Value::Bytes(bytes) => {
                Blake2::from_bytes(&bytes).map_err(move |_| FromValueError(Value::Bytes(bytes)))
            }
            v => Err(FromValueError(v)),
        }
    }

    fn commit(self) -> Self {
        self
    }

    fn rollback(self) -> Value {
        self.into()
    }
}

impl FromValue for Blake2 {
    type Intermediate = Blake2;
}
