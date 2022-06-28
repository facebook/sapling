/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use sql::mysql_async::from_value_opt;
use sql::mysql_async::prelude::ConvIr;
use sql::mysql_async::prelude::FromValue;
use sql::mysql_async::FromValueError;
use sql::mysql_async::Value;

use mononoke_types::hash::Blake2;

use crate::types::IdDagVersion;
use crate::types::IdMapVersion;

type FromValueResult<T> = Result<T, FromValueError>;

impl From<IdDagVersion> for Value {
    fn from(iddag_version: IdDagVersion) -> Self {
        Value::Bytes(iddag_version.0.as_ref().into())
    }
}

impl ConvIr<IdDagVersion> for Blake2 {
    fn new(v: Value) -> FromValueResult<Self> {
        match v {
            Value::Bytes(bytes) => {
                Blake2::from_bytes(&bytes).map_err(move |_| FromValueError(Value::Bytes(bytes)))
            }
            v => Err(FromValueError(v)),
        }
    }

    fn commit(self) -> IdDagVersion {
        IdDagVersion(self)
    }

    fn rollback(self) -> Value {
        Value::Bytes(self.as_ref().into())
    }
}

impl FromValue for IdDagVersion {
    type Intermediate = Blake2;
}

impl From<IdMapVersion> for Value {
    fn from(idmap_version: IdMapVersion) -> Self {
        Value::UInt(idmap_version.0)
    }
}

impl ConvIr<IdMapVersion> for IdMapVersion {
    fn new(v: Value) -> FromValueResult<Self> {
        Ok(IdMapVersion(from_value_opt(v)?))
    }

    fn commit(self) -> Self {
        self
    }

    fn rollback(self) -> Value {
        self.into()
    }
}

impl FromValue for IdMapVersion {
    type Intermediate = IdMapVersion;
}
