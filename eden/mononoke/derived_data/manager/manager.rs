/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use cacheblob::LeaseOps;
use changesets::Changesets;
use metaconfig_types::DerivedDataTypesConfig;
use mononoke_types::RepositoryId;
use repo_blobstore::RepoBlobstore;
use scuba_ext::MononokeScubaSampleBuilder;

use crate::lease::DerivedDataLease;

pub mod derive;
pub mod logging;
pub mod util;

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

impl DerivedDataManager {
    pub fn new(
        repo_id: RepositoryId,
        repo_name: String,
        changesets: Arc<dyn Changesets>,
        repo_blobstore: RepoBlobstore,
        lease: Arc<dyn LeaseOps>,
        scuba: MononokeScubaSampleBuilder,
        config: DerivedDataTypesConfig,
    ) -> Self {
        let lease = DerivedDataLease::new(lease);
        DerivedDataManager {
            inner: Arc::new(DerivedDataManagerInner {
                repo_id,
                repo_name,
                config,
                changesets,
                repo_blobstore,
                lease,
                scuba,
            }),
        }
    }

    // For dangerous-override: allow replacement of lease-ops
    pub fn with_replaced_lease(&self, lease: Arc<dyn LeaseOps>) -> Self {
        Self {
            inner: Arc::new(DerivedDataManagerInner {
                lease: DerivedDataLease::new(lease),
                ..self.inner.as_ref().clone()
            }),
        }
    }

    // For dangerous-override: allow replacement of blobstore
    pub fn with_replaced_blobstore(&self, repo_blobstore: RepoBlobstore) -> Self {
        Self {
            inner: Arc::new(DerivedDataManagerInner {
                repo_blobstore,
                ..self.inner.as_ref().clone()
            }),
        }
    }

    pub fn repo_id(&self) -> RepositoryId {
        self.inner.repo_id
    }

    pub fn repo_name(&self) -> &str {
        self.inner.repo_name.as_str()
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
