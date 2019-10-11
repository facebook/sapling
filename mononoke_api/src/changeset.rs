/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use std::convert::TryFrom;
use std::future::Future;
use std::pin::Pin;

use chrono::{DateTime, FixedOffset};
use cloned::cloned;
use context::CoreContext;
use derived_data::BonsaiDerived;
use fsnodes::RootFsnodeId;
use futures_preview::compat::Future01CompatExt;
use futures_preview::future::{FutureExt, Shared};
use mononoke_types::{BonsaiChangeset, MPath};
use reachabilityindex::ReachabilityIndex;
use unodes::RootUnodeManifestId;

use crate::changeset_path::ChangesetPathContext;
use crate::errors::MononokeError;
use crate::repo::RepoContext;
use crate::specifiers::{ChangesetId, HgChangesetId};

#[derive(Clone)]
pub struct ChangesetContext {
    repo: RepoContext,
    id: ChangesetId,
    bonsai_changeset:
        Shared<Pin<Box<dyn Future<Output = Result<BonsaiChangeset, MononokeError>> + Send>>>,
    root_fsnode_id:
        Shared<Pin<Box<dyn Future<Output = Result<RootFsnodeId, MononokeError>> + Send>>>,
    root_unode_manifest_id:
        Shared<Pin<Box<dyn Future<Output = Result<RootUnodeManifestId, MononokeError>> + Send>>>,
}

/// A context object representing a query to a particular commit in a repo.
impl ChangesetContext {
    /// Construct a new `MononokeChangeset`.  The changeset must exist
    /// in the repo.
    pub(crate) fn new(repo: RepoContext, id: ChangesetId) -> Self {
        let bonsai_changeset = {
            cloned!(repo);
            async move {
                repo.blob_repo()
                    .get_bonsai_changeset(repo.ctx().clone(), id)
                    .compat()
                    .await
                    .map_err(MononokeError::from)
            }
        };
        let bonsai_changeset = bonsai_changeset.boxed().shared();
        let root_fsnode_id = {
            cloned!(repo);
            async move {
                RootFsnodeId::derive(
                    repo.ctx().clone(),
                    repo.blob_repo().clone(),
                    repo.fsnodes_derived_mapping().clone(),
                    id,
                )
                .compat()
                .await
                .map_err(MononokeError::from)
            }
        };
        let root_fsnode_id = root_fsnode_id.boxed().shared();
        let root_unode_manifest_id = {
            cloned!(repo);
            async move {
                RootUnodeManifestId::derive(
                    repo.ctx().clone(),
                    repo.blob_repo().clone(),
                    repo.unodes_derived_mapping().clone(),
                    id,
                )
                .compat()
                .await
                .map_err(MononokeError::from)
            }
        };
        let root_unode_manifest_id = root_unode_manifest_id.boxed().shared();
        Self {
            repo,
            id,
            bonsai_changeset,
            root_fsnode_id,
            root_unode_manifest_id,
        }
    }

    /// The context for this query.
    pub(crate) fn ctx(&self) -> &CoreContext {
        &self.repo.ctx()
    }

    /// The `RepoContext` for this query.
    pub(crate) fn repo(&self) -> &RepoContext {
        &self.repo
    }

    /// The canonical bonsai changeset ID for the changeset.
    pub fn id(&self) -> ChangesetId {
        self.id
    }

    /// The Mercurial ID for the changeset.
    pub async fn hg_id(&self) -> Result<Option<HgChangesetId>, MononokeError> {
        let mapping = self
            .repo()
            .blob_repo()
            .get_hg_bonsai_mapping(self.ctx().clone(), self.id)
            .compat()
            .await?;
        Ok(mapping.iter().next().map(|(hg_cs_id, _)| *hg_cs_id))
    }

    pub(crate) async fn root_fsnode_id(&self) -> Result<RootFsnodeId, MononokeError> {
        self.root_fsnode_id.clone().await
    }

    pub(crate) async fn root_unode_manifest_id(
        &self,
    ) -> Result<RootUnodeManifestId, MononokeError> {
        self.root_unode_manifest_id.clone().await
    }

    /// Query the root directory in the repository at this changeset revision.
    pub fn root(&self) -> ChangesetPathContext {
        ChangesetPathContext::new(self.clone(), None)
    }

    /// Query a path within the respository. This could be a file or a
    /// directory.
    pub fn path(&self, path: impl AsRef<str>) -> Result<ChangesetPathContext, MononokeError> {
        let path = path.as_ref();
        let mpath = if path.is_empty() {
            None
        } else {
            Some(MPath::try_from(path)?)
        };
        Ok(ChangesetPathContext::new(self.clone(), mpath))
    }

    /// Get the `BonsaiChangeset` information for this changeset.
    async fn bonsai_changeset(&self) -> Result<BonsaiChangeset, MononokeError> {
        self.bonsai_changeset.clone().await
    }

    /// The IDs of the parents of the changeset.
    pub async fn parents(&self) -> Result<Vec<ChangesetId>, MononokeError> {
        Ok(self.bonsai_changeset().await?.parents().collect())
    }

    /// The author of the changeset.
    pub async fn author(&self) -> Result<String, MononokeError> {
        Ok(self.bonsai_changeset().await?.author().to_string())
    }

    /// The date the changeset was authored.
    pub async fn author_date(&self) -> Result<DateTime<FixedOffset>, MononokeError> {
        Ok(self
            .bonsai_changeset()
            .await?
            .author_date()
            .as_chrono()
            .clone())
    }

    /// The committer of the changeset.  May be `None` if the committer
    /// is not tracked.
    pub async fn committer(&self) -> Result<Option<String>, MononokeError> {
        Ok(self
            .bonsai_changeset()
            .await?
            .committer()
            .map(|s| s.to_string()))
    }

    /// The date the changeset was committed.  May be `None` if the
    /// committer is not tracked.
    pub async fn committer_date(&self) -> Result<Option<DateTime<FixedOffset>>, MononokeError> {
        Ok(self
            .bonsai_changeset()
            .await?
            .committer_date()
            .map(|d| d.as_chrono().clone()))
    }

    /// The commit message.
    pub async fn message(&self) -> Result<String, MononokeError> {
        Ok(self.bonsai_changeset().await?.message().to_string())
    }

    /// All commit extras as (name, value) pairs.
    pub async fn extras(&self) -> Result<Vec<(String, Vec<u8>)>, MononokeError> {
        Ok(self
            .bonsai_changeset()
            .await?
            .extra()
            .map(|(name, value)| (name.to_string(), Vec::from(value)))
            .collect())
    }

    /// Returns `true` if this commit is an ancestor of `other_commit`.
    pub async fn is_ancestor_of(&self, other_commit: ChangesetId) -> Result<bool, MononokeError> {
        let is_ancestor_of = self
            .repo()
            .skiplist_index()
            .query_reachability(
                self.ctx().clone(),
                self.repo().blob_repo().get_changeset_fetcher(),
                other_commit,
                self.id,
            )
            .compat()
            .await?;
        Ok(is_ancestor_of)
    }
}
