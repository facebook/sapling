/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use cachelib::VolatileLruCachePool;
use changeset_fetcher::ArcChangesetFetcher;
use fbinit::FacebookInit;
use memcache::KeyGen;
use memcache::MemcacheClient;
use mononoke_types::RepositoryId;
use phases::ArcPhases;
use sql::Connection;
use sql_construct::SqlConstruct;
use sql_construct::SqlConstructFromMetadataDatabaseConfig;
use sql_ext::SqlConnections;

use crate::sql_phases::HeadsFetcher;
use crate::sql_phases::SqlPhases;
use crate::sql_store::Caches;
use crate::sql_store::SqlPhasesStore;

// Memcache constants, should be changed when we want to invalidate memcache
// entries
const MC_CODEVER: u32 = 0;
const MC_SITEVER: u32 = 0;

/// Builder that can be used to produce SqlPhasesStore object.  Primarily
/// intended to be used by Repo factories.
#[derive(Clone)]
pub struct SqlPhasesBuilder {
    write_connection: Connection,
    read_connection: Connection,
    read_master_connection: Connection,
    caches: Arc<Caches>,
}

impl SqlPhasesBuilder {
    pub fn enable_caching(&mut self, fb: FacebookInit, cache_pool: VolatileLruCachePool) {
        let caches = Caches {
            memcache: MemcacheClient::new(fb)
                .expect("Memcache initialization failed")
                .into(),
            keygen: Self::key_gen(),
            cache_pool: cache_pool.into(),
        };
        self.caches = Arc::new(caches);
    }

    pub fn build(
        self,
        repo_id: RepositoryId,
        changeset_fetcher: ArcChangesetFetcher,
        heads_fetcher: HeadsFetcher,
    ) -> ArcPhases {
        let phases_store = self.phases_store();
        let phases = SqlPhases::new(phases_store, repo_id, changeset_fetcher, heads_fetcher);
        Arc::new(phases)
    }

    fn key_gen() -> KeyGen {
        let key_prefix = "scm.mononoke.phases";
        KeyGen::new(key_prefix, MC_CODEVER, MC_SITEVER)
    }

    fn phases_store(self) -> SqlPhasesStore {
        SqlPhasesStore {
            write_connection: self.write_connection,
            read_connection: self.read_connection,
            read_master_connection: self.read_master_connection,
            caches: self.caches,
        }
    }
}

impl SqlConstruct for SqlPhasesBuilder {
    const LABEL: &'static str = "phases";

    const CREATION_QUERY: &'static str = include_str!("../schemas/sqlite-phases.sql");

    fn from_sql_connections(connections: SqlConnections) -> Self {
        let caches = Arc::new(Caches::new_mock(Self::key_gen()));
        Self {
            write_connection: connections.write_connection,
            read_connection: connections.read_connection,
            read_master_connection: connections.read_master_connection,
            caches,
        }
    }
}

impl SqlConstructFromMetadataDatabaseConfig for SqlPhasesBuilder {}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Error;
    use context::CoreContext;
    use maplit::hashset;
    use mononoke_types_mocks::changesetid::*;
    use phases::Phase;

    #[fbinit::test]
    async fn add_get_phase_sql_test(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo_id = RepositoryId::new(0);
        let phases_builder = SqlPhasesBuilder::with_sqlite_in_memory()?;
        let phases_store = phases_builder.phases_store();

        phases_store
            .add_public_raw(&ctx, repo_id, vec![ONES_CSID])
            .await?;

        assert_eq!(
            phases_store
                .get_single_raw(&ctx, repo_id, ONES_CSID)
                .await?,
            Some(Phase::Public),
            "sql: get phase for the existing changeset"
        );

        assert_eq!(
            phases_store
                .get_single_raw(&ctx, repo_id, TWOS_CSID)
                .await?,
            None,
            "sql: get phase for non existing changeset"
        );

        assert_eq!(
            phases_store
                .get_public_raw(&ctx, repo_id, &[ONES_CSID, TWOS_CSID])
                .await?,
            hashset! {ONES_CSID},
            "sql: get phase for non existing changeset and existing changeset"
        );

        Ok(())
    }
}
