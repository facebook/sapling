/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![feature(backtrace)]
#![feature(bool_to_option)]
#![deny(warnings)]

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use anyhow::{anyhow, Context, Error};
pub use bookmarks::BookmarkName;
use futures::future;
use futures_watchdog::WatchdogExt;
use mononoke_types::RepositoryId;
use repo_factory::RepoFactory;
use slog::{debug, info, o};
pub use warm_bookmarks_cache::BookmarkUpdateDelay;

use metaconfig_parser::RepoConfigs;

pub mod changeset;
pub mod changeset_path;
pub mod changeset_path_diff;
pub mod errors;
pub mod file;
pub mod path;
pub mod repo;
pub mod repo_write;
pub mod specifiers;
pub mod tree;
mod xrepo;

#[cfg(test)]
mod test;

pub use crate::changeset::{
    ChangesetContext, ChangesetDiffItem, ChangesetHistoryOptions, Generation,
};
pub use crate::changeset_path::{
    unified_diff, ChangesetPathContentContext, ChangesetPathHistoryOptions, CopyInfo, PathEntry,
    UnifiedDiff, UnifiedDiffMode,
};
pub use crate::changeset_path_diff::ChangesetPathDiffContext;
pub use crate::errors::MononokeError;
pub use crate::file::{
    headerless_unified_diff, FileContext, FileId, FileMetadata, FileType, HeaderlessUnifiedDiff,
};
pub use crate::path::MononokePath;
pub use crate::repo::{BookmarkFreshness, Repo, RepoContext};
pub use crate::repo_write::create_changeset::{CreateChange, CreateChangeFile, CreateCopyInfo};
pub use crate::repo_write::land_stack::PushrebaseOutcome;
pub use crate::repo_write::RepoWriteContext;
pub use crate::specifiers::{
    ChangesetId, ChangesetIdPrefix, ChangesetPrefixSpecifier, ChangesetSpecifier,
    ChangesetSpecifierPrefixResolution, Globalrev, HgChangesetId, HgChangesetIdPrefix,
};
pub use crate::tree::{TreeContext, TreeEntry, TreeId, TreeSummary};
pub use crate::xrepo::CandidateSelectionHintArgs;

// Re-export types that are useful for clients.
pub use blame::CompatBlame;
pub use context::{CoreContext, LoggingContainer, SessionContainer};

/// An instance of Mononoke, which may manage multiple repositories.
pub struct Mononoke {
    repos: HashMap<String, Arc<Repo>>,
    repos_by_ids: HashMap<RepositoryId, Arc<Repo>>,
}

impl Mononoke {
    /// Create a Mononoke instance.
    pub async fn new(env: &MononokeApiEnvironment, configs: RepoConfigs) -> Result<Self, Error> {
        let repos = future::try_join_all(
            configs
                .repos
                .into_iter()
                .filter(move |&(_, ref config)| config.enabled)
                .map({
                    move |(name, config)| async move {
                        let logger = &env.repo_factory.env.logger;
                        info!(logger, "Initializing repo: {}", &name);

                        let repo = Repo::new(env, name.clone(), config)
                            .watched(logger.new(o!("repo" => name.clone())))
                            .await
                            .with_context(|| format!("could not initialize repo '{}'", &name))?;
                        debug!(logger, "Initialized {}", &name);
                        Ok::<_, Error>((name, Arc::new(repo)))
                    }
                }),
        );

        // There are lots of deep FuturesUnordered here that have caused inefficient polling with
        // Tokio coop in the past.
        let repos_vec = tokio::task::unconstrained(repos).await?;

        Self::new_from_repos(repos_vec)
    }

    fn new_from_repos(
        repos_iter: impl IntoIterator<Item = (String, Arc<Repo>)>,
    ) -> Result<Self, Error> {
        let mut repos = HashMap::new();
        let mut repos_by_ids = HashMap::new();
        for (name, repo) in repos_iter {
            if !repos.insert(name.clone(), repo.clone()).is_none() {
                return Err(anyhow!("repos with duplicate name '{}' found", name));
            }

            let repo_id = repo.blob_repo().get_repoid();
            if !repos_by_ids.insert(repo_id, repo).is_none() {
                return Err(anyhow!("repos with duplicate id '{}' found", repo_id));
            }
        }

        Ok(Self {
            repos,
            repos_by_ids,
        })
    }

    /// Start a request on a repository.
    pub async fn repo(
        &self,
        ctx: CoreContext,
        name: impl AsRef<str>,
    ) -> Result<Option<RepoContext>, MononokeError> {
        match self.repos.get(name.as_ref()) {
            None => Ok(None),
            Some(repo) => Ok(Some(RepoContext::new(ctx, repo.clone()).await?)),
        }
    }

    pub async fn repo_by_id(
        &self,
        ctx: CoreContext,
        repo_id: RepositoryId,
    ) -> Result<Option<RepoContext>, MononokeError> {
        match self.repos_by_ids.get(&repo_id) {
            None => Ok(None),
            Some(repo) => Ok(Some(RepoContext::new(ctx, repo.clone()).await?)),
        }
    }

    /// Get all known repository ids
    pub fn known_repo_ids(&self) -> Vec<RepositoryId> {
        self.repos.iter().map(|repo| repo.1.repoid()).collect()
    }

    /// Start a request on a repository bypassing the ACL check.
    ///
    /// Should be only used for internal usecases where we don't have external user with
    /// identity.
    pub async fn repo_bypass_acl_check(
        &self,
        ctx: CoreContext,
        name: impl AsRef<str>,
    ) -> Result<Option<RepoContext>, MononokeError> {
        match self.repos.get(name.as_ref()) {
            None => Ok(None),
            Some(repo) => Ok(Some(
                RepoContext::new_bypass_acl_check(ctx, repo.clone()).await?,
            )),
        }
    }

    /// Start a request on a repository bypassing the ACL check.
    ///
    /// Should be only used for internal usecases where we don't have external user with
    /// identity.
    pub async fn repo_by_id_bypass_acl_check(
        &self,
        ctx: CoreContext,
        repo_id: RepositoryId,
    ) -> Result<Option<RepoContext>, MononokeError> {
        match self.repos_by_ids.get(&repo_id) {
            None => Ok(None),
            Some(repo) => Ok(Some(
                RepoContext::new_bypass_acl_check(ctx, repo.clone()).await?,
            )),
        }
    }

    /// Returns an `Iterator` over all repo names.
    pub fn repo_names(&self) -> impl Iterator<Item = &str> {
        self.repos.keys().map(AsRef::as_ref)
    }

    pub fn repos(&self) -> impl Iterator<Item = &Arc<Repo>> {
        self.repos.values()
    }

    /// Report configured monitoring stats
    pub async fn report_monitoring_stats(&self, ctx: &CoreContext) -> Result<(), MononokeError> {
        for (_, repo) in self.repos.iter() {
            repo.report_monitoring_stats(ctx).await?;
        }

        Ok(())
    }
}

pub struct MononokeApiEnvironment {
    pub repo_factory: RepoFactory,
    pub disabled_hooks: HashMap<String, HashSet<String>>,
    pub warm_bookmarks_cache_derived_data: WarmBookmarksCacheDerivedData,
    pub warm_bookmarks_cache_delay: BookmarkUpdateDelay,
}

#[derive(Copy, Clone, Debug)]
pub enum WarmBookmarksCacheDerivedData {
    HgOnly,
    AllKinds,
    None,
}

pub mod test_impl {
    use super::*;
    use blobrepo::BlobRepo;
    use cloned::cloned;
    use live_commit_sync_config::{
        LiveCommitSyncConfig, TestLiveCommitSyncConfig, TestLiveCommitSyncConfigSource,
    };
    use metaconfig_types::CommitSyncConfig;
    use synced_commit_mapping::SyncedCommitMapping;

    impl Mononoke {
        /// Create a Mononoke instance for testing.
        pub async fn new_test(
            ctx: CoreContext,
            repos: impl IntoIterator<Item = (String, BlobRepo)>,
        ) -> Result<Self, Error> {
            use futures::stream::{FuturesOrdered, TryStreamExt};

            let repos = repos
                .into_iter()
                .map(move |(name, repo)| {
                    cloned!(ctx);
                    async move {
                        Repo::new_test(ctx.clone(), repo)
                            .await
                            .map(move |repo| (name, Arc::new(repo)))
                    }
                })
                .collect::<FuturesOrdered<_>>()
                .try_collect::<HashMap<_, _>>()
                .await?;

            Self::new_from_repos(repos)
        }

        pub async fn new_test_xrepo(
            ctx: CoreContext,
            repos: impl IntoIterator<
                Item = (
                    String,
                    BlobRepo,
                    CommitSyncConfig,
                    Arc<dyn SyncedCommitMapping>,
                ),
            >,
        ) -> Result<(Self, TestLiveCommitSyncConfigSource), Error> {
            use futures::stream::{FuturesOrdered, TryStreamExt};
            let (lv_cfg, lv_cfg_src) = TestLiveCommitSyncConfig::new_with_source();
            let lv_cfg: Arc<dyn LiveCommitSyncConfig> = Arc::new(lv_cfg);

            let repos = repos
                .into_iter()
                .map({
                    cloned!(lv_cfg_src);
                    move |(name, repo, commit_sync_config, synced_commit_maping)| {
                        cloned!(ctx, lv_cfg, lv_cfg_src);
                        async move {
                            lv_cfg_src.add_config(commit_sync_config.clone());
                            lv_cfg_src.add_current_version(commit_sync_config.version_name);
                            Repo::new_test_xrepo(ctx.clone(), repo, lv_cfg, synced_commit_maping)
                                .await
                                .map(move |repo| (name, Arc::new(repo)))
                        }
                    }
                })
                .collect::<FuturesOrdered<_>>()
                .try_collect::<HashMap<_, _>>()
                .await?;

            let mononoke = Self::new_from_repos(repos)?;
            Ok((mononoke, lv_cfg_src))
        }
    }
}
