/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use bonsai_hg_mapping::BonsaiHgMapping;
use cacheblob::LeaseOps;
use changesets::Changesets;
use derived_data_manager::DerivedDataManager;
use derived_data_remote::DerivationClient;
use filenodes::Filenodes;
use metaconfig_types::DerivedDataConfig;
use metaconfig_types::DerivedDataTypesConfig;
use mononoke_types::RepositoryId;
use repo_blobstore::RepoBlobstore;
use scuba_ext::MononokeScubaSampleBuilder;
use slog::Logger;

#[facet::facet]
pub struct DerivedDataManagerSet {
    logger: Logger,
    configs: HashMap<String, DerivedDataManager>,
}

impl DerivedDataManagerSet {
    pub fn get_mananger(&self, config_name: impl Into<String>) -> Option<&DerivedDataManager> {
        self.configs.get(&config_name.into())
    }

    pub fn logger(&self) -> &Logger {
        &self.logger
    }
}

#[facet::container]
pub struct DerivedDataServiceRepo {
    #[facet]
    pub manager_set: DerivedDataManagerSet,
}

impl DerivedDataManagerSet {
    pub fn new(
        repo_id: RepositoryId,
        repo_name: String,
        changesets: Arc<dyn Changesets>,
        bonsai_hg_mapping: Arc<dyn BonsaiHgMapping>,
        filenodes: Arc<dyn Filenodes>,
        repo_blobstore: RepoBlobstore,
        lease: Arc<dyn LeaseOps>,
        logger: Logger,
        derived_data_scuba: MononokeScubaSampleBuilder,
        config: DerivedDataConfig,
        derivation_service_client: Option<Arc<dyn DerivationClient>>,
    ) -> Result<Self> {
        let manager = DerivedDataManager::new(
            repo_id,
            repo_name,
            changesets,
            bonsai_hg_mapping,
            filenodes,
            repo_blobstore,
            lease,
            derived_data_scuba,
            String::default(),
            DerivedDataTypesConfig::default(),
            derivation_service_client,
        );
        let configs = config
            .available_configs
            .into_iter()
            .map(|(config_name, config)| {
                (
                    config_name.clone(),
                    manager.with_replaced_config(config_name, config),
                )
            })
            .collect();

        Ok(Self { logger, configs })
    }
}
