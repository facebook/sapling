/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use mononoke_types::DerivableType;
use mononoke_types::RepositoryId;
use sql::mysql;
use sql::mysql_async::FromValueError;
use sql::mysql_async::Value;
use sql::mysql_async::prelude::FromValue;

/// SQL (de)serialization wrapper for [`DerivableType`].
///
/// The `enabled_derived_data_types.derived_data_type` column stores the
/// canonical name string for a derived data type (e.g. `git_delta_manifests_v3`)
/// as produced by [`DerivableType::name`] and parsed back by
/// [`DerivableType::from_name`]. Using the canonical name (rather than the strum
/// `Display`/`FromStr` variant name) keeps the DB value in sync with what the
/// rest of Mononoke uses to identify a type. Parsing on read validates the value.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, mysql::OptTryFromRowField)]
pub struct SqlDerivableType(pub DerivableType);

impl From<SqlDerivableType> for Value {
    fn from(ddt: SqlDerivableType) -> Self {
        Value::Bytes(ddt.0.name().into())
    }
}

impl FromValue for SqlDerivableType {
    type Intermediate = SqlDerivableType;
}

impl TryFrom<Value> for SqlDerivableType {
    type Error = FromValueError;
    fn try_from(v: Value) -> Result<Self, FromValueError> {
        match v {
            Value::Bytes(bytes) => match String::from_utf8(bytes) {
                Ok(s) => DerivableType::from_name(&s)
                    .map(SqlDerivableType)
                    .map_err(|_| FromValueError(Value::Bytes(s.into()))),
                Err(from_utf8_error) => {
                    Err(FromValueError(Value::Bytes(from_utf8_error.into_bytes())))
                }
            },
            v => Err(FromValueError(v)),
        }
    }
}

/// A single row of the `enabled_derived_data_types` table: the presence of an
/// entry means `derived_data_type` is enabled for `repo_id`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EnabledDerivedDataTypeEntry {
    pub repo_id: RepositoryId,
    pub derived_data_type: DerivableType,
    /// The async-requests campaign (`root_request_id`) that enabled this type,
    /// or `None` for a manual poke.
    pub root_request_id: Option<u64>,
}
