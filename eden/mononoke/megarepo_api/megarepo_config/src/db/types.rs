/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use bookmarks::BookmarkName;
use megarepo_configs::Source;
use megarepo_configs::SyncConfigVersion;
use mononoke_types::RepositoryId;
use sql::mysql;
use sql::mysql_async::from_value_opt;
use sql::mysql_async::prelude::ConvIr;
use sql::mysql_async::prelude::FromValue;
use sql::mysql_async::FromValueError;
use sql::mysql_async::Value;

#[derive(Clone, Copy, Debug, Eq, PartialEq, mysql::OptTryFromRowField)]
pub struct RowId(pub u64);

impl From<RowId> for Value {
    fn from(id: RowId) -> Self {
        Value::UInt(id.0)
    }
}

impl std::fmt::Display for RowId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
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

impl FromValue for RowId {
    type Intermediate = RowId;
}

#[derive(Clone, Debug)]
pub struct MegarepoSyncConfigEntry {
    #[allow(dead_code)]
    pub id: RowId,
    pub repo_id: RepositoryId,
    pub bookmark: BookmarkName,
    pub version: SyncConfigVersion,
    pub sources: Vec<Source>,
}
