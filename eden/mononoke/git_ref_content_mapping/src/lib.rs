/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod cache;
mod sql;

use anyhow::Result;
use async_trait::async_trait;
use context::CoreContext;
use mononoke_types::RepositoryId;
use mononoke_types::hash::GitSha1;

pub use crate::cache::CachedGitRefContentMapping;
pub use crate::sql::SqlGitRefContentMapping;
pub use crate::sql::SqlGitRefContentMappingBuilder;

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct GitRefContentMappingEntry {
    pub ref_name: String,
    pub git_hash: GitSha1,
    pub is_tree: bool,
}

impl GitRefContentMappingEntry {
    pub fn new(ref_name: String, git_hash: GitSha1, is_tree: bool) -> Self {
        GitRefContentMappingEntry {
            ref_name,
            git_hash,
            is_tree,
        }
    }
}

#[facet::facet]
#[async_trait]
/// Facet trait representing Git ref to Git tree or Git blob mapping
pub trait GitRefContentMapping: Send + Sync {
    /// The repository for which this mapping has been created
    fn repo_id(&self) -> RepositoryId;

    /// Fetch all the ref content mapping entries for the given repo
    async fn get_all_entries(&self, ctx: &CoreContext) -> Result<Vec<GitRefContentMappingEntry>>;

    /// Fetch the git ref content mapping entry corresponding to the ref name in the
    /// given repo, if one exists
    async fn get_entry_by_ref_name(
        &self,
        ctx: &CoreContext,
        ref_name: String,
    ) -> Result<Option<GitRefContentMappingEntry>>;

    /// Add new git ref to content mapping entries or update existing ones
    async fn add_or_update_mappings(
        &self,
        ctx: &CoreContext,
        entries: Vec<GitRefContentMappingEntry>,
    ) -> Result<()>;

    /// Delete existing git ref content mappings based on the input ref names
    async fn delete_mappings_by_name(
        &self,
        ctx: &CoreContext,
        ref_names: Vec<String>,
    ) -> Result<()>;
}
