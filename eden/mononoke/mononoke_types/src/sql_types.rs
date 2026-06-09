/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use sql::mysql_async::FromValueError;
use sql::mysql_async::Value;
use sql::mysql_async::from_value_opt;
use sql::mysql_async::prelude::FromValue;
use sql::sql_common::mysql::OptionalTryFromRowField;
use sql::sql_common::mysql::RowField;
use sql::sql_common::mysql::ValueError;
use sql::sql_common::mysql::opt_try_from_rowfield;

use crate::NonRootMPath;
use crate::datetime::Timestamp;
use crate::globalrev::Globalrev;
use crate::hash::Blake2;
use crate::hash::GitSha1;
use crate::repo::RepositoryId;
use crate::svnrev::Svnrev;
use crate::typed_hash::ChangesetId;

type FromValueResult<T> = Result<T, FromValueError>;

impl From<ChangesetId> for Value {
    fn from(id: ChangesetId) -> Self {
        Value::Bytes(id.as_ref().into())
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

impl TryFrom<Value> for Timestamp {
    type Error = FromValueError;

    fn try_from(v: Value) -> FromValueResult<Self> {
        Ok(Timestamp::from_timestamp_nanos(from_value_opt(v)?))
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

impl TryFrom<Value> for RepositoryId {
    type Error = FromValueError;

    fn try_from(v: Value) -> FromValueResult<Self> {
        Ok(RepositoryId::new(from_value_opt(v)?))
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

impl TryFrom<Value> for GitSha1 {
    type Error = FromValueError;

    fn try_from(v: Value) -> FromValueResult<Self> {
        match v {
            Value::Bytes(bytes) => {
                GitSha1::from_bytes(&bytes).map_err(move |_| FromValueError(Value::Bytes(bytes)))
            }
            v => Err(FromValueError(v)),
        }
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

impl TryFrom<Value> for Globalrev {
    type Error = FromValueError;

    fn try_from(v: Value) -> FromValueResult<Self> {
        Ok(Globalrev::new(from_value_opt(v)?))
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

impl TryFrom<Value> for Svnrev {
    type Error = FromValueError;

    fn try_from(v: Value) -> FromValueResult<Self> {
        Ok(Svnrev::new(from_value_opt(v)?))
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

impl TryFrom<Value> for Blake2 {
    type Error = FromValueError;

    fn try_from(v: Value) -> FromValueResult<Self> {
        match v {
            Value::Bytes(bytes) => {
                Blake2::from_bytes(&bytes).map_err(move |_| FromValueError(Value::Bytes(bytes)))
            }
            v => Err(FromValueError(v)),
        }
    }
}

impl FromValue for Blake2 {
    type Intermediate = Blake2;
}

impl From<NonRootMPath> for Value {
    fn from(path: NonRootMPath) -> Self {
        Value::Bytes(path.to_vec())
    }
}

impl TryFrom<Value> for NonRootMPath {
    type Error = FromValueError;

    fn try_from(v: Value) -> FromValueResult<Self> {
        match v {
            Value::Bytes(bytes) => {
                NonRootMPath::new(&bytes).map_err(move |_| FromValueError(Value::Bytes(bytes)))
            }
            v => Err(FromValueError(v)),
        }
    }
}

impl FromValue for NonRootMPath {
    type Intermediate = NonRootMPath;
}

impl OptionalTryFromRowField for NonRootMPath {
    fn try_from_opt(field: RowField) -> Result<Option<Self>, ValueError> {
        opt_try_from_rowfield(field)
    }
}
