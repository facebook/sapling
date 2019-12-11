/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

#![feature(backtrace)]
#![deny(warnings)]

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Error;
use blobrepo::BlobRepo;
use blobrepo_factory::{Caching, ReadOnlyStorage};
use cloned::cloned;
use fbinit::FacebookInit;
use fsnodes::RootFsnodeMapping;
use futures_preview::future;
use futures_preview::future::try_join_all;
use skiplist::SkiplistIndex;
use slog::{debug, info, o, Logger};
use synced_commit_mapping::SyncedCommitMapping;
use unodes::RootUnodeManifestMapping;
use warm_bookmarks_cache::WarmBookmarksCache;

use metaconfig_parser::RepoConfigs;
use metaconfig_types::SourceControlServiceMonitoring;

use crate::repo::Repo;

pub mod changeset;
pub mod changeset_path;
pub mod changeset_path_diff;
pub mod errors;
pub mod file;
pub mod legacy;
pub mod path;
pub mod repo;
pub mod specifiers;
pub mod tree;

#[cfg(test)]
mod test;

pub use crate::legacy::get_content_by_path;

pub use crate::changeset::ChangesetContext;
pub use crate::changeset_path::{
    unified_diff, ChangesetPathContext, CopyInfo, PathEntry, UnifiedDiff,
};
pub use crate::changeset_path_diff::ChangesetPathDiffContext;
pub use crate::errors::MononokeError;
pub use crate::file::{FileContext, FileId, FileMetadata, FileType};
pub use crate::path::MononokePath;
pub use crate::repo::RepoContext;
pub use crate::specifiers::{ChangesetId, ChangesetSpecifier, HgChangesetId};
pub use crate::tree::{TreeContext, TreeEntry, TreeId, TreeSummary};

// Re-export types that are useful for clients.
pub use context::{CoreContext, LoggingContainer, SessionContainer};

/// An instance of Mononoke, which may manage multiple repositories.
pub struct Mononoke {
    repos: HashMap<String, Arc<Repo>>,
}

impl Mononoke {
    /// Create a Mononoke instance.
    pub async fn new(
        fb: FacebookInit,
        logger: Logger,
        configs: RepoConfigs,
        myrouter_port: Option<u16>,
        with_cachelib: Caching,
        readonly_storage: ReadOnlyStorage,
    ) -> Result<Self, Error> {
        let common_config = configs.common;
        let repos = future::join_all(
            configs
                .repos
                .into_iter()
                .filter(move |&(_, ref config)| config.enabled)
                .map(move |(name, config)| {
                    cloned!(logger, common_config);
                    async move {
                        info!(logger, "Initializing repo: {}", &name);
                        let repo = Repo::new(
                            fb,
                            logger.new(o!("repo" => name.clone())),
                            name.clone(),
                            config,
                            common_config,
                            myrouter_port,
                            with_cachelib,
                            readonly_storage,
                        )
                        .await
                        .expect("failed to initialize repo");
                        debug!(logger, "Initialized {}", &name);
                        (name, Arc::new(repo))
                    }
                }),
        )
        .await
        .into_iter()
        .collect();
        Ok(Self { repos })
    }

    /// Create a Mononoke instance for testing.
    #[cfg(test)]
    async fn new_test(
        ctx: CoreContext,
        repos: impl IntoIterator<Item = (String, BlobRepo)>,
    ) -> Result<Self, Error> {
        use futures_util::stream::FuturesOrdered;
        use futures_util::try_stream::TryStreamExt;
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

    /// Temporary function to create directly from parts.
    pub fn new_from_parts(
        repos: impl IntoIterator<
            Item = (
                String,
                BlobRepo,
                Arc<SkiplistIndex>,
                Arc<RootUnodeManifestMapping>,
                Arc<WarmBookmarksCache>,
                Arc<dyn SyncedCommitMapping>,
                Option<SourceControlServiceMonitoring>,
            ),
        >,
    ) -> Self {
        Self {
            repos: repos
                .into_iter()
                .map(
                    |(
                        name,
                        blob_repo,
                        skiplist_index,
                        unodes_derived_mapping,
                        warm_bookmarks_cache,
                        synced_commit_mapping,
                        monitoring_config,
                    )| {
                        let fsnodes_derived_mapping =
                            Arc::new(RootFsnodeMapping::new(blob_repo.get_blobstore()));
                        (
                            name.clone(),
                            Arc::new(Repo::new_from_parts(
                                name,
                                blob_repo,
                                skiplist_index,
                                fsnodes_derived_mapping,
                                unodes_derived_mapping,
                                warm_bookmarks_cache,
                                synced_commit_mapping,
                                monitoring_config,
                            )),
                        )
                    },
                )
                .collect(),
        }
    }

    /// Start a request on a repository.
    pub fn repo(
        &self,
        ctx: CoreContext,
        name: impl AsRef<str>,
    ) -> Result<Option<RepoContext>, MononokeError> {
        let name = name.as_ref();
        let repo = self
            .repos
            .get(name)
            .map(move |repo| RepoContext::new(ctx, repo.clone()));
        Ok(repo)
    }

    /// Returns an `Iterator` over all repo names.
    pub fn repo_names(&self) -> impl Iterator<Item = &str> {
        self.repos.keys().map(AsRef::as_ref)
    }

    /// Report configured monitoring stats
    pub async fn report_monitoring_stats(&self, ctx: CoreContext) -> Result<(), MononokeError> {
        let reporting_futs: Vec<_> = self
            .repos
            .iter()
            .map(|(_, repo)| RepoContext::new(ctx.clone(), repo.clone()))
            .map(|repo| async move { repo.report_monitoring_stats().await })
            .collect();
        try_join_all(reporting_futs).await.map(|_| ())
    }
}
