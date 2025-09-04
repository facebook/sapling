/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::str::FromStr;

use anyhow::Result;
use mononoke_types::RepositoryId;
use sql::mysql;
use sql::mysql_async::FromValueError;
use sql::mysql_async::Value;
use sql::mysql_async::from_value_opt;
use sql::mysql_async::prelude::ConvIr;
use sql::mysql_async::prelude::FromValue;
use strum::Display as StrumDisplay;
use strum::EnumString;

#[derive(Clone, Copy, Debug, Eq, PartialEq, mysql::OptTryFromRowField)]
pub struct RowId(pub u64);

#[derive(Clone, Debug, Eq, PartialEq, Hash, mysql::OptTryFromRowField)]
pub struct RepositoryName(pub String);

#[derive(
    Clone,
    Copy,
    Debug,
    EnumString,
    Eq,
    PartialEq,
    StrumDisplay,
    mysql::OptTryFromRowField
)]
#[strum(serialize_all = "snake_case")]
pub enum GitSourceOfTruth {
    Mononoke,
    Metagit,
    Locked,
    Reserved,
}

impl From<RowId> for Value {
    fn from(id: RowId) -> Self {
        Value::UInt(id.0)
    }
}

impl From<RepositoryName> for Value {
    fn from(repo_name: RepositoryName) -> Self {
        Value::Bytes(repo_name.0.into())
    }
}

impl From<GitSourceOfTruth> for Value {
    fn from(source_of_truth: GitSourceOfTruth) -> Self {
        Value::Bytes(source_of_truth.to_string().into())
    }
}

impl std::fmt::Display for RowId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::fmt::Display for RepositoryName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromValue for RowId {
    type Intermediate = RowId;
}

impl FromValue for RepositoryName {
    type Intermediate = RepositoryName;
}

impl FromValue for GitSourceOfTruth {
    type Intermediate = GitSourceOfTruth;
}

impl ConvIr<RowId> for RowId {
    fn new(v: Value) -> Result<Self, FromValueError> {
        Ok(RowId(from_value_opt(v)?))
    }
    fn commit(self) -> Self {
        self
    }
    fn rollback(self) -> Value {
        self.into()
    }
}

impl ConvIr<RepositoryName> for RepositoryName {
    fn new(v: Value) -> Result<Self, FromValueError> {
        match v {
            Value::Bytes(bytes) => match String::from_utf8(bytes) {
                Ok(s) => Ok(RepositoryName(s)),
                Err(from_utf8_error) => {
                    Err(FromValueError(Value::Bytes(from_utf8_error.into_bytes())))
                }
            },
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

impl ConvIr<GitSourceOfTruth> for GitSourceOfTruth {
    fn new(v: Value) -> Result<Self, FromValueError> {
        match v {
            Value::Bytes(bytes) => match String::from_utf8(bytes) {
                Ok(s) => Ok(GitSourceOfTruth::from_str(&s)
                    .map_err(|_| FromValueError(Value::Bytes(s.into())))?),
                Err(from_utf8_error) => {
                    Err(FromValueError(Value::Bytes(from_utf8_error.into_bytes())))
                }
            },
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GitSourceOfTruthConfigEntry {
    pub id: RowId,
    pub repo_id: RepositoryId,
    pub repo_name: RepositoryName,
    pub source_of_truth: GitSourceOfTruth,
}
