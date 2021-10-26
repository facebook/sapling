/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use context::{CoreContext, SessionClass};
use scuba_ext::MononokeScubaSampleBuilder;

use crate::derivable::BonsaiDerivable;
use crate::error::DerivationError;

use super::DerivedDataManager;

impl DerivedDataManager {
    /// Returns the passed-in `CoreContext` with the session class modified to
    /// the one that should be used for derivation.
    pub(super) fn set_derivation_session_class(&self, mut ctx: CoreContext) -> CoreContext {
        if tunables::tunables()
            .get_by_repo_derived_data_use_background_session_class(self.repo_name())
            .unwrap_or(false)
        {
            ctx.session_mut()
                .override_session_class(SessionClass::BackgroundUnlessTooSlow);
        }
        ctx
    }

    /// Construct a scuba sample builder for logging derivation of this
    /// changeset to the derived data scuba table.
    pub(super) fn derived_data_scuba<Derivable>(&self) -> MononokeScubaSampleBuilder
    where
        Derivable: BonsaiDerivable,
    {
        let mut scuba = self.inner.scuba.clone();
        scuba.add("derived_data", Derivable::NAME);
        scuba
    }

    pub(super) fn check_enabled<Derivable>(&self) -> Result<(), DerivationError>
    where
        Derivable: BonsaiDerivable,
    {
        if self.config().types.contains(Derivable::NAME) {
            Ok(())
        } else {
            Err(DerivationError::Disabled(
                Derivable::NAME,
                self.repo_id(),
                self.repo_name().to_string(),
            ))
        }
    }

    pub(super) fn max_parallel_derivations(&self) -> usize {
        let buffer_size = tunables::tunables().get_derived_data_parallel_derivation_buffer();
        if buffer_size > 0 {
            buffer_size
                .try_into()
                .expect("buffer size should convert to usize")
        } else {
            10
        }
    }
}

pub mod derived_data_service {
    use std::sync::Arc;

    use super::DerivedDataManager;
    use anyhow::Result;
    use bonsai_hg_mapping::BonsaiHgMapping;
    use cacheblob::LeaseOps;
    use changesets::Changesets;
    use filenodes::Filenodes;
    use metaconfig_types::DerivedDataConfig;
    use mononoke_types::RepositoryId;
    use repo_blobstore::RepoBlobstore;
    use scuba_ext::MononokeScubaSampleBuilder;

    #[facet::facet]
    pub struct DerivedDataManagerSet {
        prod: DerivedDataManager,
        backfilling: DerivedDataManager,
    }

    impl DerivedDataManagerSet {
        pub fn get_prod(&self) -> &DerivedDataManager {
            return &self.prod;
        }

        pub fn get_backfilling(&self) -> &DerivedDataManager {
            return &self.backfilling;
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
            scuba: MononokeScubaSampleBuilder,
            config: DerivedDataConfig,
        ) -> Result<Self> {
            let prod = DerivedDataManager::new(
                repo_id,
                repo_name,
                changesets,
                bonsai_hg_mapping,
                filenodes,
                repo_blobstore,
                lease,
                scuba,
                config.enabled.clone(),
            );
            let backfilling = prod.with_replaced_config(config.backfilling.clone());
            Ok(Self { prod, backfilling })
        }
    }
}
