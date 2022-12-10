/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use anyhow::Context;
use anyhow::Result;
use bonsai_hg_mapping::BonsaiHgMapping;
use cacheblob::LeaseOps;
use changesets::Changesets;
use context::CoreContext;
use derived_data_remote::DerivationClient;
use filenodes::Filenodes;
use metaconfig_types::DerivedDataTypesConfig;
use mononoke_types::ChangesetId;
use mononoke_types::RepositoryId;
use repo_blobstore::RepoBlobstore;
use scuba_ext::MononokeScubaSampleBuilder;

use crate::lease::DerivedDataLease;

pub mod bubble;
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
    bonsai_hg_mapping: Option<Arc<dyn BonsaiHgMapping>>,
    filenodes: Option<Arc<dyn Filenodes>>,
    repo_blobstore: RepoBlobstore,
    lease: DerivedDataLease,
    scuba: MononokeScubaSampleBuilder,
    config_name: String,
    config: DerivedDataTypesConfig,
    /// If a (primary) manager has a secondary manager, that means some of the
    /// changesets should be derived using the primary manager, and some the secondary,
    /// in that order. For example, bubble managers are secondary, as all the data in
    /// the persistent blobstore must be derived BEFORE deriving data in the bubble.
    secondary: Option<SecondaryManagerData>,
    /// If this client is set, then derivation will be done remotely on derived data service
    derivation_service_client: Option<Arc<dyn DerivationClient>>,
}

pub struct DerivationAssignment {
    /// Changesets that should be derived by the primary manager
    pub primary: Vec<ChangesetId>,
    /// Changesets that should be derived by the secondary manager, after the first
    /// part of derivation is done.
    pub secondary: Vec<ChangesetId>,
}

#[async_trait::async_trait]
pub trait DerivationAssigner: Send + Sync {
    /// How to split derivation between primary and secondary managers. If not possible
    /// to split, this function should error.
    async fn assign(&self, ctx: &CoreContext, cs: Vec<ChangesetId>)
    -> Result<DerivationAssignment>;
}

#[derive(Clone)]
pub(crate) struct SecondaryManagerData {
    manager: DerivedDataManager,
    assigner: Arc<dyn DerivationAssigner>,
}

impl DerivedDataManager {
    pub fn new(
        repo_id: RepositoryId,
        repo_name: String,
        changesets: Arc<dyn Changesets>,
        bonsai_hg_mapping: Arc<dyn BonsaiHgMapping>,
        filenodes: Arc<dyn Filenodes>,
        repo_blobstore: RepoBlobstore,
        lease: Arc<dyn LeaseOps>,
        scuba: MononokeScubaSampleBuilder,
        config_name: String,
        config: DerivedDataTypesConfig,
        derivation_service_client: Option<Arc<dyn DerivationClient>>,
    ) -> Self {
        let lease = DerivedDataLease::new(lease);
        DerivedDataManager {
            inner: Arc::new(DerivedDataManagerInner {
                repo_id,
                repo_name,
                config_name,
                config,
                changesets,
                bonsai_hg_mapping: Some(bonsai_hg_mapping),
                filenodes: Some(filenodes),
                repo_blobstore,
                lease,
                scuba,
                secondary: None,
                derivation_service_client,
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

    // For dangerous-override: allow replacement of changesets
    pub fn with_replaced_changesets(&self, changesets: Arc<dyn Changesets>) -> Self {
        Self {
            inner: Arc::new(DerivedDataManagerInner {
                changesets,
                ..self.inner.as_ref().clone()
            }),
        }
    }

    // For dangerous-override: allow replacement of bonsai-hg-mapping
    pub fn with_replaced_bonsai_hg_mapping(
        &self,
        bonsai_hg_mapping: Arc<dyn BonsaiHgMapping>,
    ) -> Self {
        Self {
            inner: Arc::new(DerivedDataManagerInner {
                bonsai_hg_mapping: Some(bonsai_hg_mapping),
                ..self.inner.as_ref().clone()
            }),
        }
    }

    // For dangerous-override: allow replacement of filenodes
    pub fn with_replaced_filenodes(&self, filenodes: Arc<dyn Filenodes>) -> Self {
        Self {
            inner: Arc::new(DerivedDataManagerInner {
                filenodes: Some(filenodes),
                ..self.inner.as_ref().clone()
            }),
        }
    }

    pub fn with_replaced_config(
        &self,
        config_name: String,
        config: DerivedDataTypesConfig,
    ) -> Self {
        Self {
            inner: Arc::new(DerivedDataManagerInner {
                config_name,
                config,
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

    pub fn changesets_arc(&self) -> Arc<dyn Changesets> {
        self.inner.changesets.clone()
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

    pub fn config_name(&self) -> String {
        self.inner.config_name.clone()
    }

    pub fn bonsai_hg_mapping(&self) -> Result<&dyn BonsaiHgMapping> {
        self.inner
            .bonsai_hg_mapping
            .as_deref()
            .context("Missing BonsaiHgMapping")
    }

    pub fn filenodes(&self) -> Result<&dyn Filenodes> {
        self.inner.filenodes.as_deref().context("Missing filenodes")
    }

    pub fn derivation_service_client(&self) -> Option<&dyn DerivationClient> {
        self.inner.derivation_service_client.as_deref()
    }
}
