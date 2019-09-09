// Copyright (c) 2019-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::future::Future;
use std::pin::Pin;

use chrono::{DateTime, FixedOffset};
use cloned::cloned;
use futures_preview::compat::Future01CompatExt;
use futures_preview::future::{FutureExt, Shared};
use mononoke_types::BonsaiChangeset;
use reachabilityindex::ReachabilityIndex;

use crate::errors::MononokeError;
use crate::repo::RepoContext;
use crate::specifiers::{ChangesetId, HgChangesetId};

#[derive(Clone)]
pub struct ChangesetContext {
    repo_ctx: RepoContext,
    id: ChangesetId,
    bonsai_changeset:
        Shared<Pin<Box<dyn Future<Output = Result<BonsaiChangeset, MononokeError>> + Send>>>,
}

impl ChangesetContext {
    /// Construct a new `MononokeChangeset`.  The changeset must exist
    /// in the repo.
    pub(crate) fn new(repo_ctx: RepoContext, id: ChangesetId) -> Self {
        let bonsai_changeset = {
            cloned!(repo_ctx);
            async move {
                repo_ctx
                    .repo
                    .blob_repo
                    .get_bonsai_changeset(repo_ctx.ctx.clone(), id)
                    .compat()
                    .await
                    .map_err(MononokeError::from)
            }
        };
        let bonsai_changeset = bonsai_changeset.boxed().shared();
        Self {
            repo_ctx,
            id,
            bonsai_changeset,
        }
    }

    /// The canonical bonsai changeset ID for the changeset.
    pub fn id(&self) -> ChangesetId {
        self.id
    }

    /// The Mercurial ID for the changeset.
    pub async fn hg_id(&self) -> Result<Option<HgChangesetId>, MononokeError> {
        let mapping = self
            .repo_ctx
            .repo
            .blob_repo
            .get_hg_bonsai_mapping(self.repo_ctx.ctx.clone(), self.id)
            .compat()
            .await?;
        Ok(mapping.iter().next().map(|(hg_cs_id, _)| *hg_cs_id))
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
            .repo_ctx
            .repo
            .skiplist_index
            .query_reachability(
                self.repo_ctx.ctx.clone(),
                self.repo_ctx.repo.blob_repo.get_changeset_fetcher(),
                other_commit,
                self.id,
            )
            .compat()
            .await?;
        Ok(is_ancestor_of)
    }
}
