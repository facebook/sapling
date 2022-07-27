/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![feature(backtrace)]
#![feature(bool_to_option)]

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Error;
pub use bookmarks::BookmarkName;
use futures::stream;
use futures::StreamExt;
use futures_watchdog::WatchdogExt;
use metaconfig_parser::RepoConfigs;
use mononoke_types::RepositoryId;
use repo_factory::RepoFactory;
use scuba_ext::MononokeScubaSampleBuilder;
use slog::debug;
use slog::info;
use slog::o;

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

pub use crate::changeset::ChangesetContext;
pub use crate::changeset::ChangesetDiffItem;
pub use crate::changeset::ChangesetFileOrdering;
pub use crate::changeset::ChangesetHistoryOptions;
pub use crate::changeset::Generation;
pub use crate::changeset_path::unified_diff;
pub use crate::changeset_path::ChangesetPathContentContext;
pub use crate::changeset_path::ChangesetPathHistoryOptions;
pub use crate::changeset_path::CopyInfo;
pub use crate::changeset_path::PathEntry;
pub use crate::changeset_path::UnifiedDiff;
pub use crate::changeset_path::UnifiedDiffMode;
pub use crate::changeset_path_diff::ChangesetPathDiffContext;
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

// Re-export types that are useful for clients.
pub use blame::CompatBlame;
pub use context::CoreContext;
pub use context::LoggingContainer;
pub use context::SessionContainer;

use regex::Regex;

/// An instance of Mononoke, which may manage multiple repositories.
pub struct Mononoke {
    repos: HashMap<String, Arc<Repo>>,
    repos_by_ids: HashMap<RepositoryId, Arc<Repo>>,
}

impl Mononoke {
    /// Create a Mononoke instance.
    pub async fn new(env: &MononokeApiEnvironment, configs: RepoConfigs) -> Result<Self, Error> {
        let start = Instant::now();

        let repos = stream::iter(configs.repos.into_iter().filter(
            move |&(ref name, ref config)| {
                let is_matching_filter = env
                    .repo_filter
                    .as_ref()
                    .map_or(true, |re| re.is_match(name.as_str()));
                config.enabled && is_matching_filter
            },
        ))
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
        })
        .buffer_unordered(30)
        .collect::<Vec<_>>();

        // There are lots of deep FuturesUnordered here that have caused inefficient polling with
        // Tokio coop in the past.
        let repos_vec = tokio::task::unconstrained(repos)
            .await
            .into_iter()
            .collect::<Result<Vec<_>, _>>()?;

        info!(
            &env.repo_factory.env.logger,
            "All repos initialized. It took: {} seconds",
            start.elapsed().as_secs()
        );

        Self::new_from_repos(repos_vec)
    }

    fn new_from_repos(
        repos_iter: impl IntoIterator<Item = (String, Arc<Repo>)>,
    ) -> Result<Self, Error> {
        let mut repos = HashMap::new();
        let mut repos_by_ids = HashMap::new();
        for (name, repo) in repos_iter {
            if repos.insert(name.clone(), repo.clone()).is_some() {
                return Err(anyhow!("repos with duplicate name '{}' found", name));
            }

            let repo_id = repo.blob_repo().get_repoid();
            if repos_by_ids.insert(repo_id, repo).is_some() {
                return Err(anyhow!("repos with duplicate id '{}' found", repo_id));
            }
        }

        Ok(Self {
            repos,
            repos_by_ids,
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
        match self.repos.get(name.as_ref()) {
            None => Ok(None),
            Some(repo) => Ok(Some(RepoContextBuilder::new(ctx, repo.clone()))),
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
        match self.repos_by_ids.get(&repo_id) {
            None => Ok(None),
            Some(repo) => Ok(Some(RepoContextBuilder::new(ctx, repo.clone()))),
        }
    }

    /// Get all known repository ids
    pub fn known_repo_ids(&self) -> Vec<RepositoryId> {
        self.repos.iter().map(|repo| repo.1.repoid()).collect()
    }

    /// Returns an `Iterator` over all repo names.
    pub fn repo_names(&self) -> impl Iterator<Item = &str> {
        self.repos.keys().map(AsRef::as_ref)
    }

    pub fn repos(&self) -> impl Iterator<Item = &Arc<Repo>> {
        self.repos.values()
    }

    pub fn repo_name_from_id(&self, repo_id: RepositoryId) -> Option<&String> {
        match self.repos_by_ids.get(&repo_id) {
            None => None,
            Some(repo) => Some(repo.name()),
        }
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
    pub warm_bookmarks_cache_derived_data: WarmBookmarksCacheDerivedData,
    pub warm_bookmarks_cache_enabled: bool,
    pub warm_bookmarks_cache_scuba_sample_builder: MononokeScubaSampleBuilder,
    pub skiplist_enabled: bool,
    pub repo_filter: Option<Regex>,
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
    use live_commit_sync_config::LiveCommitSyncConfig;
    use metaconfig_types::CommitSyncConfig;
    use synced_commit_mapping::SyncedCommitMapping;

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
            small_repo: (String, BlobRepo),
            large_repo: (String, BlobRepo),
            _commit_sync_config: CommitSyncConfig,
            mapping: Arc<dyn SyncedCommitMapping>,
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
                                .map(move |repo| (name, Arc::new(repo)))
                        }
                    }
                })
                .collect::<FuturesOrdered<_>>()
                .try_collect::<HashMap<_, _>>()
                .await?;

            let mononoke = Self::new_from_repos(repos)?;
            Ok(mononoke)
        }
    }
}
