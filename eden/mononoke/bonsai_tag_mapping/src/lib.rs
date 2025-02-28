/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod sql;

use anyhow::Result;
use async_trait::async_trait;
use mononoke_types::hash::GitSha1;
use mononoke_types::ChangesetId;
use mononoke_types::RepositoryId;

pub use crate::sql::SqlBonsaiTagMapping;
pub use crate::sql::SqlBonsaiTagMappingBuilder;

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct BonsaiTagMappingEntry {
    pub changeset_id: ChangesetId,
    pub tag_name: String,
    pub tag_hash: GitSha1,
    pub target_is_tag: bool,
}

impl BonsaiTagMappingEntry {
    pub fn new(
        changeset_id: ChangesetId,
        tag_name: String,
        tag_hash: GitSha1,
        target_is_tag: bool,
    ) -> Self {
        BonsaiTagMappingEntry {
            changeset_id,
            tag_name,
            tag_hash,
            target_is_tag,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Freshness {
    /// The mapping is guaranteed to be up to date
    Latest,
    /// The mapping may be stale and is subject to replication lag
    MaybeStale,
}

#[facet::facet]
#[async_trait]
/// Facet trait representing Bonsai Changeset to Git Tag mapping
pub trait BonsaiTagMapping: Send + Sync {
    /// The repository for which this mapping has been created
    fn repo_id(&self) -> RepositoryId;

    /// Fetch all the tag mapping entries for the given repo
    async fn get_all_entries(&self) -> Result<Vec<BonsaiTagMappingEntry>>;

    /// Fetch the tag mapping entries corresponding to the input changeset ids
    /// for the given repo
    async fn get_entries_by_changesets(
        &self,
        changeset_id: Vec<ChangesetId>,
    ) -> Result<Vec<BonsaiTagMappingEntry>>;

    /// Fetch the tag mapping entry corresponding to the tag name in the
    /// given repo, if one exists
    async fn get_entry_by_tag_name(
        &self,
        tag_name: String,
        freshness: Freshness,
    ) -> Result<Option<BonsaiTagMappingEntry>>;

    /// Fetch the tag mapping entries corresponding to the input changeset id
    /// for the given repo. Note that there can potentially be multiple
    /// tags that map to the same changeset
    async fn get_entries_by_changeset(
        &self,
        changeset_id: ChangesetId,
    ) -> Result<Option<Vec<BonsaiTagMappingEntry>>> {
        let entries = self.get_entries_by_changesets(vec![changeset_id]).await?;
        Ok((!entries.is_empty()).then_some(entries))
    }

    /// Fetch the tag mapping entries corresponding to the input tag hashes
    async fn get_entries_by_tag_hashes(
        &self,
        tag_hashes: Vec<GitSha1>,
    ) -> Result<Vec<BonsaiTagMappingEntry>>;

    /// Add new tag name to bonsai changeset mappings
    async fn add_or_update_mappings(&self, entries: Vec<BonsaiTagMappingEntry>) -> Result<()>;

    /// Delete existing bonsai tag mappings based on the input tag names
    async fn delete_mappings_by_name(&self, tag_names: Vec<String>) -> Result<()>;
}
