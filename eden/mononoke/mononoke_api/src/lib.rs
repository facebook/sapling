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

use anyhow::{Context, Error};
use blobrepo_factory::{BlobstoreOptions, Caching, ReadOnlyStorage};
pub use bookmarks::BookmarkName;
use cached_config::ConfigStore;
use cloned::cloned;
use fbinit::FacebookInit;
use futures::future;
use slog::{debug, info, Logger};
use sql_ext::facebook::MysqlOptions;
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
    unified_diff, ChangesetPathContext, ChangesetPathHistoryOptions, CopyInfo, PathEntry,
    UnifiedDiff, UnifiedDiffMode,
};
pub use crate::changeset_path_diff::ChangesetPathDiffContext;
pub use crate::errors::MononokeError;
pub use crate::file::{
    headerless_unified_diff, FileContext, FileId, FileMetadata, FileType, HeaderlessUnifiedDiff,
};
pub use crate::path::MononokePath;
pub use crate::repo::{BookmarkFreshness, Repo, RepoContext};
pub use crate::repo_write::create_changeset::{CreateChange, CreateCopyInfo};
pub use crate::repo_write::land_stack::PushrebaseOutcome;
pub use crate::repo_write::RepoWriteContext;
pub use crate::specifiers::{
    ChangesetId, ChangesetIdPrefix, ChangesetPrefixSpecifier, ChangesetSpecifier,
    ChangesetSpecifierPrefixResolution, Globalrev, HgChangesetId, HgChangesetIdPrefix,
};
pub use crate::tree::{TreeContext, TreeEntry, TreeId, TreeSummary};
pub use crate::xrepo::CandidateSelectionHintArgs;

// Re-export types that are useful for clients.
pub use context::{CoreContext, LoggingContainer, SessionContainer};

/// An instance of Mononoke, which may manage multiple repositories.
pub struct Mononoke {
    repos: HashMap<String, Arc<Repo>>,
}

impl Mononoke {
    /// Create a Mononoke instance.
    pub async fn new(env: &MononokeEnvironment<'_>, configs: RepoConfigs) -> Result<Self, Error> {
        let common_config = configs.common;
        let repos = future::try_join_all(
            configs
                .repos
                .into_iter()
                .filter(move |&(_, ref config)| config.enabled)
                .map({
                    move |(name, config)| {
                        cloned!(common_config);
                        async move {
                            info!(&env.logger, "Initializing repo: {}", &name);
                            let repo = Repo::new(env, name.clone(), config, common_config)
                                .await
                                .with_context(|| {
                                    format!("could not initialize repo '{}'", &name)
                                })?;
                            debug!(&env.logger, "Initialized {}", &name);
                            Ok::<_, Error>((name, Arc::new(repo)))
                        }
                    }
                }),
        )
        .await?
        .into_iter()
        .collect();
        Ok(Self { repos })
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

pub struct MononokeEnvironment<'a> {
    pub fb: FacebookInit,
    pub logger: Logger,
    pub mysql_options: MysqlOptions,
    pub caching: Caching,
    pub readonly_storage: ReadOnlyStorage,
    pub blobstore_options: BlobstoreOptions,
    pub config_store: &'a ConfigStore,
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

#[cfg(test)]
mod test_impl {
    use super::*;
    use blobrepo::BlobRepo;
    use live_commit_sync_config::{
        LiveCommitSyncConfig, TestLiveCommitSyncConfig, TestLiveCommitSyncConfigSource,
    };
    use metaconfig_types::CommitSyncConfig;
    use synced_commit_mapping::SyncedCommitMapping;

    impl Mononoke {
        /// Create a Mononoke instance for testing.
        pub(crate) async fn new_test(
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
                .try_collect()
                .await?;

            Ok(Self { repos })
        }

        pub(crate) async fn new_test_xrepo(
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
                .try_collect()
                .await?;

            Ok((Self { repos }, lv_cfg_src))
        }
    }
}
