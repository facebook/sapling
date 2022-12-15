/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![feature(error_generic_member_access)]
#![feature(provide_any)]

use std::sync::Arc;

use anyhow::Error;
pub use bookmarks::BookmarkName;
use mononoke_repos::MononokeRepos;
use mononoke_types::RepositoryId;

use crate::repo::RepoContextBuilder;

pub mod changeset;
pub mod changeset_path;
pub mod changeset_path_diff;
pub mod errors;
pub mod file;
pub mod path;
pub mod repo;
pub mod sparse_profile;
pub mod specifiers;
pub mod tree;
mod xrepo;

#[cfg(test)]
mod test;

// Re-export types that are useful for clients.
pub use blame::CompatBlame;
pub use context::CoreContext;
pub use context::LoggingContainer;
pub use context::SessionContainer;

pub use crate::changeset::ChangesetContext;
pub use crate::changeset::ChangesetDiffItem;
pub use crate::changeset::ChangesetFileOrdering;
pub use crate::changeset::ChangesetHistoryOptions;
pub use crate::changeset::Generation;
pub use crate::changeset_path::ChangesetPathContentContext;
pub use crate::changeset_path::ChangesetPathHistoryOptions;
pub use crate::changeset_path::PathEntry;
pub use crate::changeset_path_diff::ChangesetPathDiffContext;
pub use crate::changeset_path_diff::CopyInfo;
pub use crate::changeset_path_diff::FileContentType;
pub use crate::changeset_path_diff::FileGeneratedStatus;
pub use crate::changeset_path_diff::MetadataDiff;
pub use crate::changeset_path_diff::MetadataDiffFileInfo;
pub use crate::changeset_path_diff::MetadataDiffLinesCount;
pub use crate::changeset_path_diff::UnifiedDiff;
pub use crate::changeset_path_diff::UnifiedDiffMode;
pub use crate::errors::MononokeError;
pub use crate::file::headerless_unified_diff;
pub use crate::file::FileContext;
pub use crate::file::FileId;
pub use crate::file::FileMetadata;
pub use crate::file::FileType;
pub use crate::file::HeaderlessUnifiedDiff;
pub use crate::path::MononokePath;
pub use crate::repo::create_changeset::CreateChange;
pub use crate::repo::create_changeset::CreateChangeFile;
pub use crate::repo::create_changeset::CreateCopyInfo;
pub use crate::repo::land_stack::PushrebaseOutcome;
pub use crate::repo::BookmarkFreshness;
pub use crate::repo::BookmarkInfo;
pub use crate::repo::Repo;
pub use crate::repo::RepoContext;
pub use crate::specifiers::ChangesetId;
pub use crate::specifiers::ChangesetIdPrefix;
pub use crate::specifiers::ChangesetPrefixSpecifier;
pub use crate::specifiers::ChangesetSpecifier;
pub use crate::specifiers::ChangesetSpecifierPrefixResolution;
pub use crate::specifiers::Globalrev;
pub use crate::specifiers::HgChangesetId;
pub use crate::specifiers::HgChangesetIdPrefix;
pub use crate::tree::TreeContext;
pub use crate::tree::TreeEntry;
pub use crate::tree::TreeId;
pub use crate::tree::TreeSummary;
pub use crate::xrepo::CandidateSelectionHintArgs;

/// An instance of Mononoke, which may manage multiple repositories.
pub struct Mononoke {
    // Collection of instantiated repos currently being served.
    pub repos: Arc<MononokeRepos<Repo>>,
    // The collective list of all enabled repos that exist
    // in the current tier (e.g. prod, backup, etc.)
    pub repo_names_in_tier: Vec<String>,
}

impl Mononoke {
    /// Create a MononokeAPI instance for MononokeRepos
    ///
    /// Takes extra argument containing list of all aviailable repos
    /// (used to power APIs listing repos; TODO: change that arg to MonononokeConfigs)
    pub fn new(
        repos: Arc<MononokeRepos<Repo>>,
        repo_names_in_tier: Vec<String>,
    ) -> Result<Self, Error> {
        Ok(Self {
            repos,
            repo_names_in_tier,
        })
    }

    /// Start a request on a repository by name.
    // Method is async and fallible as in the future this may involve
    // instantiating the repo lazily.
    pub async fn repo(
        &self,
        ctx: CoreContext,
        name: impl AsRef<str>,
    ) -> Result<Option<RepoContextBuilder>, MononokeError> {
        match self.repos.get_by_name(name.as_ref()) {
            None => Ok(None),
            Some(repo) => Ok(Some(
                RepoContextBuilder::new(ctx, repo, self.repos.as_ref()).await?,
            )),
        }
    }

    /// Start a request on a repository by id.
    // Method is async and fallible as in the future this may involve
    // instantiating the repo lazily.
    pub async fn repo_by_id(
        &self,
        ctx: CoreContext,
        repo_id: RepositoryId,
    ) -> Result<Option<RepoContextBuilder>, MononokeError> {
        match self.repos.get_by_id(repo_id.id()) {
            None => Ok(None),
            Some(repo) => Ok(Some(
                RepoContextBuilder::new(ctx, repo, self.repos.as_ref()).await?,
            )),
        }
    }

    /// Return the raw underlying repo corresponding to the provided
    /// repo name.
    pub fn raw_repo(&self, name: impl AsRef<str>) -> Option<Arc<Repo>> {
        self.repos.get_by_name(name.as_ref())
    }

    /// Return the raw underlying repo corresponding to the provided
    /// repo id.
    pub fn raw_repo_by_id(&self, id: i32) -> Option<Arc<Repo>> {
        self.repos.get_by_id(id)
    }

    /// Get all known repository ids
    pub fn known_repo_ids(&self) -> Vec<RepositoryId> {
        self.repos.iter_ids().map(RepositoryId::new).collect()
    }

    /// Returns an `Iterator` over all repo names.
    pub fn repo_names(&self) -> impl Iterator<Item = String> {
        self.repos.iter_names()
    }

    pub fn repos(&self) -> impl Iterator<Item = Arc<Repo>> {
        self.repos.iter()
    }

    pub fn repo_name_from_id(&self, repo_id: RepositoryId) -> Option<String> {
        self.repos
            .get_by_id(repo_id.id())
            .map(|repo| repo.name().to_string())
    }

    pub fn repo_id_from_name(&self, name: impl AsRef<str>) -> Option<RepositoryId> {
        self.repos
            .get_by_name(name.as_ref())
            .map(|repo| repo.repoid())
    }

    /// Report configured monitoring stats
    pub async fn report_monitoring_stats(&self, ctx: &CoreContext) -> Result<(), MononokeError> {
        for repo in self.repos.iter() {
            repo.report_monitoring_stats(ctx).await?;
        }

        Ok(())
    }
}

pub mod test_impl {
    use blobrepo::BlobRepo;
    use cloned::cloned;
    use live_commit_sync_config::LiveCommitSyncConfig;
    use metaconfig_types::CommitSyncConfig;
    use synced_commit_mapping::ArcSyncedCommitMapping;

    use super::*;

    impl Mononoke {
        /// Create a Mononoke instance for testing.
        pub async fn new_test(
            ctx: CoreContext,
            repos: impl IntoIterator<Item = (String, BlobRepo)>,
        ) -> Result<Self, Error> {
            use futures::stream::FuturesOrdered;
            use futures::stream::TryStreamExt;
            let repos = repos
                .into_iter()
                .map(move |(name, repo)| {
                    cloned!(ctx);
                    async move {
                        Repo::new_test(ctx.clone(), repo)
                            .await
                            .map(move |repo| (repo.blob_repo().get_repoid().id(), name, repo))
                    }
                })
                .collect::<FuturesOrdered<_>>()
                .try_collect::<Vec<_>>()
                .await?;
            let repo_names_in_tier =
                Vec::from_iter(repos.iter().map(|(_, name, _)| name.to_string()));
            let mononoke_repos = MononokeRepos::new();
            mononoke_repos.populate(repos);
            Ok(Self {
                repos: Arc::new(mononoke_repos),
                repo_names_in_tier,
            })
        }

        pub async fn new_test_xrepo(
            ctx: CoreContext,
            small_repo: (String, BlobRepo),
            large_repo: (String, BlobRepo),
            _commit_sync_config: CommitSyncConfig,
            mapping: ArcSyncedCommitMapping,
            lv_cfg: Arc<dyn LiveCommitSyncConfig>,
        ) -> Result<Self, Error> {
            use futures::stream::FuturesOrdered;
            use futures::stream::TryStreamExt;

            let repos = vec![small_repo, large_repo]
                .into_iter()
                .map({
                    move |(name, repo)| {
                        cloned!(ctx, lv_cfg, mapping);
                        async move {
                            Repo::new_test_xrepo(ctx.clone(), repo, lv_cfg, mapping)
                                .await
                                .map(move |repo| (repo.blob_repo().get_repoid().id(), name, repo))
                        }
                    }
                })
                .collect::<FuturesOrdered<_>>()
                .try_collect::<Vec<_>>()
                .await?;

            let repo_names_in_tier =
                Vec::from_iter(repos.iter().map(|(_, name, _)| name.to_string()));
            let mononoke_repos = MononokeRepos::new();
            mononoke_repos.populate(repos);
            Ok(Self {
                repos: Arc::new(mononoke_repos),
                repo_names_in_tier,
            })
        }
    }
}
