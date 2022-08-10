/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use blobrepo::BlobRepo;
use bonsai_hg_mapping::BonsaiHgMapping;
use bookmarks::ArcBookmarkUpdateLog;
use bookmarks::ArcBookmarks;
use facet::facet;
use metaconfig_types::BackupRepoConfig;
use metaconfig_types::CommonCommitSyncConfig;
use metaconfig_types::RepoClientKnobs;
use mutable_counters::ArcMutableCounters;
use repo_blobstore::RepoBlobstore;
use scuba_ext::MononokeScubaSampleBuilder;
use slog::Logger;
use sql_ext::SqlConnections;
use synced_commit_mapping::SyncedCommitMapping;

#[derive(Clone)]
#[facet]
/// The core push-redirection mode for a repo. This enum allows
/// to express if push-redirection is enabled for a repo-pair or
/// if its disabled.
pub enum PushRedirectorMode {
    Enabled(Arc<PushRedirectorBase>),
    Disabled,
}

#[derive(Clone)]
/// The base struct for a repo with push-redirection enabled
pub struct PushRedirectorBase {
    pub common_commit_sync_config: CommonCommitSyncConfig,
    pub synced_commit_mapping: Arc<dyn SyncedCommitMapping>,
    pub target_repo_dbs: Arc<TargetRepoDbs>,
}

#[derive(Clone)]
#[facet]
pub struct TargetRepoDbs {
    pub connections: SqlConnections,
    pub bookmarks: ArcBookmarks,
    pub bookmark_update_log: ArcBookmarkUpdateLog,
    pub counters: ArcMutableCounters,
}

#[derive(Clone)]
#[facet]
/// The base struct for serving wireproto traffic for a repo
pub struct RepoHandlerBase {
    pub logger: Logger,
    pub scuba: MononokeScubaSampleBuilder,
    pub maybe_push_redirector_base: Option<Arc<PushRedirectorBase>>,
    pub repo_client_knobs: RepoClientKnobs,
    pub backup_repo_config: Option<BackupRepoConfig>,
}

#[facet::container]
#[derive(Clone)]
/// The source repo for a given backup repo
pub struct BackupSourceRepo {
    #[facet]
    pub bonsai_hg_mapping: dyn BonsaiHgMapping,
    #[facet]
    pub repo_blobstore: RepoBlobstore,
}

impl BackupSourceRepo {
    pub fn from_blob_repo(repo: &BlobRepo) -> Self {
        Self {
            bonsai_hg_mapping: Arc::clone(repo.bonsai_hg_mapping()),
            repo_blobstore: Arc::new(repo.get_blobstore()),
        }
    }
}
