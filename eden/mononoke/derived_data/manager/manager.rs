/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use anyhow::Result;
use cacheblob::LeaseOps;
use changesets::Changesets;
use metaconfig_types::DerivedDataTypesConfig;
use mononoke_types::RepositoryId;
use repo_blobstore::RepoBlobstore;
use scuba_ext::MononokeScubaSampleBuilder;

use crate::lease::DerivedDataLease;

/// Manager for derived data.
///
/// The manager is responsible for ordering derivation of data based
/// on the dependencies between derived data types and the changeset
/// graph.
#[derive(Clone)]
pub struct DerivedDataManager {
    inner: Arc<DerivedDataManagerInner>,
}

#[derive(Clone)]
pub struct DerivedDataManagerInner {
    repo_id: RepositoryId,
    repo_name: String,
    changesets: Arc<dyn Changesets>,
    repo_blobstore: RepoBlobstore,
    lease: DerivedDataLease,
    scuba: MononokeScubaSampleBuilder,
    config: DerivedDataTypesConfig,
}

pub struct DerivedDataManagerBuilder {
    repo_id: RepositoryId,
    repo_name: String,
    changesets: Arc<dyn Changesets>,
    repo_blobstore: RepoBlobstore,
    lease: DerivedDataLease,
    scuba: MononokeScubaSampleBuilder,
    config: DerivedDataTypesConfig,
}

impl DerivedDataManagerBuilder {
    pub fn new(
        repo_id: RepositoryId,
        repo_name: String,
        changesets: Arc<dyn Changesets>,
        repo_blobstore: RepoBlobstore,
        lease: DerivedDataLease,
        scuba: MononokeScubaSampleBuilder,
        config: DerivedDataTypesConfig,
    ) -> Result<Self> {
        Ok(DerivedDataManagerBuilder {
            repo_id,
            repo_name,
            config,
            changesets,
            repo_blobstore,
            lease,
            scuba,
        })
    }

    pub fn build(self) -> DerivedDataManager {
        DerivedDataManager {
            inner: Arc::new(DerivedDataManagerInner {
                repo_id: self.repo_id,
                repo_name: self.repo_name,
                config: self.config,
                changesets: self.changesets,
                repo_blobstore: self.repo_blobstore,
                lease: self.lease,
                scuba: self.scuba,
            }),
        }
    }
}

impl DerivedDataManager {
    // For dangerous-override: allow replacement of lease-ops
    pub fn with_replaced_lease(&self, lease: Arc<dyn LeaseOps>) -> Self {
        Self {
            inner: Arc::new(DerivedDataManagerInner {
                lease: DerivedDataLease::new(lease),
                ..self.inner.as_ref().clone()
            }),
        }
    }

    pub fn changesets(&self) -> &dyn Changesets {
        self.inner.changesets.as_ref()
    }

    pub fn repo_blobstore(&self) -> &RepoBlobstore {
        &self.inner.repo_blobstore
    }

    pub fn lease(&self) -> &DerivedDataLease {
        &self.inner.lease
    }

    pub fn scuba(&self) -> &MononokeScubaSampleBuilder {
        &self.inner.scuba
    }

    pub fn config(&self) -> &DerivedDataTypesConfig {
        &self.inner.config
    }
}
