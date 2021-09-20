/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Repo derived data
//!
//! Stores configuration and state for data derivation.

use std::sync::Arc;

use anyhow::Result;
use cacheblob::LeaseOps;
use changesets::Changesets;
use derived_data_manager::DerivedDataManager;
use metaconfig_types::DerivedDataConfig;
use mononoke_types::RepositoryId;
use repo_blobstore::RepoBlobstore;
use scuba_ext::MononokeScubaSampleBuilder;

/// Repository derived data management.
#[facet::facet]
pub struct RepoDerivedData {
    /// Configuration for derived data.
    config: DerivedDataConfig,

    /// Derived data manager for the enabled types on this repo.
    manager: DerivedDataManager,
}

impl RepoDerivedData {
    /// Construct a new RepoDerivedData.
    pub fn new(
        repo_id: RepositoryId,
        repo_name: String,
        changesets: Arc<dyn Changesets>,
        repo_blobstore: RepoBlobstore,
        lease: Arc<dyn LeaseOps>,
        scuba: MononokeScubaSampleBuilder,
        config: DerivedDataConfig,
    ) -> Result<RepoDerivedData> {
        let manager = DerivedDataManager::new(
            repo_id,
            repo_name,
            changesets,
            repo_blobstore,
            lease,
            scuba,
            config.enabled.clone(),
        );
        Ok(RepoDerivedData { config, manager })
    }

    // For dangerous-override: allow replacement of lease-ops
    pub fn with_replaced_lease(&self, lease: Arc<dyn LeaseOps>) -> Self {
        Self {
            config: self.config.clone(),
            manager: self.manager.with_replaced_lease(lease),
        }
    }

    // For dangerous-override: allow replacement of blobstore
    pub fn with_replaced_blobstore(&self, repo_blobstore: RepoBlobstore) -> Self {
        Self {
            config: self.config.clone(),
            manager: self.manager.with_replaced_blobstore(repo_blobstore),
        }
    }

    /// Current derived data configuration for this repo.
    pub fn config(&self) -> &DerivedDataConfig {
        &self.config
    }

    /// Derived data lease for this repo.
    pub fn lease(&self) -> &Arc<dyn LeaseOps> {
        self.manager.lease().lease_ops()
    }

    pub fn manager(&self) -> &DerivedDataManager {
        &self.manager
    }
}
