/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Repo Cross Repo.
//!
//! Manages cross-repo interactions for this repo

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use cacheblob::LeaseOps;
use live_commit_sync_config::LiveCommitSyncConfig;
use mononoke_types::NonRootMPath;
use mononoke_types::RepositoryId;
use synced_commit_mapping::ArcSyncedCommitMapping;

/// Repository identity information.
#[facet::facet]
pub struct RepoCrossRepo {
    /// The mapping of synced commits.
    synced_commit_mapping: ArcSyncedCommitMapping,

    /// The commit sync config that is currently live.
    live_commit_sync_config: Arc<dyn LiveCommitSyncConfig>,

    /// Lease for cross-repo syncing, to prevent multiple servers stampeding
    /// to sync the same commits.
    sync_lease: Arc<dyn LeaseOps>,

    /// Ids of submodule dependencies of this repo
    submodule_dep_ids: Arc<HashMap<NonRootMPath, RepositoryId>>,
}

impl RepoCrossRepo {
    /// Construct a new RepoCrossRepo.
    pub async fn new(
        synced_commit_mapping: ArcSyncedCommitMapping,
        live_commit_sync_config: Arc<dyn LiveCommitSyncConfig>,
        sync_lease: Arc<dyn LeaseOps>,
        repo_id: RepositoryId,
    ) -> Result<RepoCrossRepo> {
        let source_repo_sync_configs = live_commit_sync_config
            .get_all_commit_sync_config_versions(repo_id)
            .await?;

        let submodule_dep_ids = source_repo_sync_configs
            .into_values()
            .filter_map(|mut cfg| {
                cfg.small_repos
                    .remove(&repo_id)
                    .map(|small_repo_cfg| small_repo_cfg.submodule_config.submodule_dependencies)
            })
            .flatten()
            .collect::<HashMap<_, _>>();

        Ok(RepoCrossRepo {
            synced_commit_mapping,
            live_commit_sync_config,
            sync_lease,
            submodule_dep_ids: Arc::new(submodule_dep_ids),
        })
    }

    pub fn synced_commit_mapping(&self) -> &ArcSyncedCommitMapping {
        &self.synced_commit_mapping
    }

    pub fn live_commit_sync_config(&self) -> &Arc<dyn LiveCommitSyncConfig> {
        &self.live_commit_sync_config
    }

    pub fn sync_lease(&self) -> &Arc<dyn LeaseOps> {
        &self.sync_lease
    }

    pub fn submodule_dep_ids(&self) -> &Arc<HashMap<NonRootMPath, RepositoryId>> {
        &self.submodule_dep_ids
    }
}
