/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Repo Cross Repo.
//!
//! Manages cross-repo interactions for this repo

use std::sync::Arc;

use cacheblob::LeaseOps;
use live_commit_sync_config::LiveCommitSyncConfig;
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
}

impl RepoCrossRepo {
    /// Construct a new RepoCrossRepo.
    pub fn new(
        synced_commit_mapping: ArcSyncedCommitMapping,
        live_commit_sync_config: Arc<dyn LiveCommitSyncConfig>,
        sync_lease: Arc<dyn LeaseOps>,
    ) -> RepoCrossRepo {
        RepoCrossRepo {
            synced_commit_mapping,
            live_commit_sync_config,
            sync_lease,
        }
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
}
