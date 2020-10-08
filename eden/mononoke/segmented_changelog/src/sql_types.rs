/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use sql::mysql_async::{
    from_value_opt,
    prelude::{ConvIr, FromValue},
    FromValueError, Value,
};

use crate::types::{IdDagVersion, IdMapVersion};

type FromValueResult<T> = Result<T, FromValueError>;

impl From<IdDagVersion> for Value {
    fn from(iddag_version: IdDagVersion) -> Self {
        Value::UInt(iddag_version.0)
    }
}

impl ConvIr<IdDagVersion> for IdDagVersion {
    fn new(v: Value) -> FromValueResult<Self> {
        Ok(IdDagVersion(from_value_opt(v)?))
    }

    fn commit(self) -> Self {
        self
    }

    fn rollback(self) -> Value {
        self.into()
    }
}

impl FromValue for IdDagVersion {
    type Intermediate = IdDagVersion;
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
