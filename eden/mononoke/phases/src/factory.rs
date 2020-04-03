/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::caching::Caches;
use crate::{sql_store::SqlPhasesStore, HeadsFetcher, Phases, SqlPhases};
use cachelib::VolatileLruCachePool;
use changeset_fetcher::ChangesetFetcher;
use fbinit::FacebookInit;
use memcache::{KeyGen, MemcacheClient};
use mononoke_types::RepositoryId;
use sql::Connection;
use sql_construct::{SqlConstruct, SqlConstructFromMetadataDatabaseConfig};
use sql_ext::SqlConnections;
use std::sync::Arc;

// Memcache constants, should be changed when we want to invalidate memcache
// entries
const MC_CODEVER: u32 = 0;
const MC_SITEVER: u32 = 0;

/// Factory that can be used to produce SqlPhasesStore object
/// Primarily intended to be used by BlobRepo
#[derive(Clone)]
pub struct SqlPhasesFactory {
    write_connection: Connection,
    read_connection: Connection,
    read_master_connection: Connection,
    caches: Arc<Caches>,
}

impl SqlPhasesFactory {
    pub fn enable_caching(&mut self, fb: FacebookInit, cache_pool: VolatileLruCachePool) {
        let caches = Caches {
            memcache: MemcacheClient::new(fb)
                .expect("Memcache initialization failed")
                .into(),
            keygen: Self::get_key_gen(),
            cache_pool: cache_pool.into(),
        };
        self.caches = Arc::new(caches);
    }

    pub fn get_phases(
        &self,
        repo_id: RepositoryId,
        changeset_fetcher: Arc<dyn ChangesetFetcher>,
        heads_fetcher: HeadsFetcher,
    ) -> Arc<dyn Phases> {
        let phases_store = self.get_phases_store();
        let phases = SqlPhases {
            phases_store,
            changeset_fetcher,
            heads_fetcher,
            repo_id,
        };
        Arc::new(phases)
    }

    fn get_key_gen() -> KeyGen {
        let key_prefix = "scm.mononoke.phases";
        KeyGen::new(key_prefix, MC_CODEVER, MC_SITEVER)
    }

    fn get_phases_store(&self) -> SqlPhasesStore {
        SqlPhasesStore {
            write_connection: self.write_connection.clone(),
            read_connection: self.read_connection.clone(),
            read_master_connection: self.read_master_connection.clone(),
            caches: self.caches.clone(),
        }
    }
}

impl SqlConstruct for SqlPhasesFactory {
    const LABEL: &'static str = "phases";

    const CREATION_QUERY: &'static str = include_str!("../schemas/sqlite-phases.sql");

    fn from_sql_connections(connections: SqlConnections) -> Self {
        let caches = Arc::new(Caches::new_mock(Self::get_key_gen()));
        Self {
            write_connection: connections.write_connection,
            read_connection: connections.read_connection,
            read_master_connection: connections.read_master_connection,
            caches,
        }
    }
}

impl SqlConstructFromMetadataDatabaseConfig for SqlPhasesFactory {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Phase;
    use anyhow::Error;
    use context::CoreContext;
    use futures::compat::Future01CompatExt;
    use maplit::hashset;
    use mononoke_types_mocks::changesetid::*;

    #[fbinit::compat_test]
    async fn add_get_phase_sql_test(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo_id = RepositoryId::new(0);
        let phases_factory = SqlPhasesFactory::with_sqlite_in_memory()?;
        let phases = phases_factory.get_phases_store();

        phases
            .add_public_raw(ctx, repo_id, vec![ONES_CSID])
            .compat()
            .await?;

        assert_eq!(
            phases.get_single_raw(repo_id, ONES_CSID).compat().await?,
            Some(Phase::Public),
            "sql: get phase for the existing changeset"
        );

        assert_eq!(
            phases.get_single_raw(repo_id, TWOS_CSID).compat().await?,
            None,
            "sql: get phase for non existing changeset"
        );

        assert_eq!(
            phases
                .get_public_raw(repo_id, &[ONES_CSID, TWOS_CSID])
                .compat()
                .await?,
            hashset! {ONES_CSID},
            "sql: get phase for non existing changeset and existing changeset"
        );

        Ok(())
    }
}
