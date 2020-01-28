/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use crate::caching::{Caches, CachingPhases};
use crate::{HeadsFetcher, Phases, SqlPhases, SqlPhasesStore};
use changeset_fetcher::ChangesetFetcher;
use fbinit::FacebookInit;
use memcache::{KeyGen, MemcacheClient};
use mononoke_types::RepositoryId;
use std::sync::Arc;

// Memcache constants, should be changed when we want to invalidate memcache
// entries
const MC_CODEVER: u32 = 0;
const MC_SITEVER: u32 = 0;

/// Factory that can be used to produce Phases object
/// Primarily intended to be used by BlobRepo
#[derive(Clone)]
pub struct SqlPhasesFactory {
    phases_store: Arc<SqlPhasesStore>,
    repo_id: RepositoryId,
    caches: Option<Arc<Caches>>,
}

impl SqlPhasesFactory {
    pub fn new_no_caching(phases_store: Arc<SqlPhasesStore>, repo_id: RepositoryId) -> Self {
        Self {
            phases_store,
            repo_id,
            caches: None,
        }
    }

    pub fn new_with_caching(
        fb: FacebookInit,
        phases_store: Arc<SqlPhasesStore>,
        repo_id: RepositoryId,
    ) -> Self {
        let key_prefix = "scm.mononoke.phases";
        let caches = Caches {
            memcache: MemcacheClient::new(fb),
            keygen: KeyGen::new(key_prefix, MC_CODEVER, MC_SITEVER),
        };

        Self {
            phases_store,
            repo_id,
            caches: Some(Arc::new(caches)),
        }
    }

    pub fn get_phases(
        &self,
        changeset_fetcher: Arc<dyn ChangesetFetcher>,
        heads_fetcher: HeadsFetcher,
    ) -> Arc<dyn Phases> {
        let phases = SqlPhases {
            phases_store: self.phases_store.clone(),
            changeset_fetcher,
            heads_fetcher,
            repo_id: self.repo_id.clone(),
        };
        match &self.caches {
            Some(caches) => Arc::new(CachingPhases::new(phases, caches.clone())),
            None => Arc::new(phases),
        }
    }
}
