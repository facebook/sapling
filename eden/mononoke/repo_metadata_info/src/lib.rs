/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod sql;

use anyhow::Result;
use async_trait::async_trait;
use mononoke_types::ChangesetId;
use mononoke_types::RepositoryId;
use mononoke_types::Timestamp;

pub use crate::sql::SqlRepoMetadataInfo;
pub use crate::sql::SqlRepoMetadataInfoBuilder;

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct RepoMetadataInfoEntry {
    pub changeset_id: ChangesetId,
    pub bookmark_name: String,
    pub last_updated_timestamp: Timestamp,
}

impl RepoMetadataInfoEntry {
    pub fn new(
        changeset_id: ChangesetId,
        bookmark_name: String,
        last_updated_timestamp: Timestamp,
    ) -> Self {
        RepoMetadataInfoEntry {
            changeset_id,
            bookmark_name,
            last_updated_timestamp,
        }
    }
}

#[facet::facet]
#[async_trait]
/// Facet trait representing repo metadata info for a repo
pub trait RepoMetadataInfo: Send + Sync {
    /// The repository for which this entry has been created
    fn repo_id(&self) -> RepositoryId;

    /// Fetch all the metadata info entries for the given repo
    async fn get_all_entries(&self) -> Result<Vec<RepoMetadataInfoEntry>>;

    /// Fetch the repo metadata entries corresponding to the input bookmark name
    /// for the given repo
    async fn get_entry(&self, bookmark_name: String) -> Result<Option<RepoMetadataInfoEntry>>;

    /// Add new or update existing repo metadata entries for the given repo
    async fn add_or_update_entries(&self, entries: Vec<RepoMetadataInfoEntry>) -> Result<()>;
}
