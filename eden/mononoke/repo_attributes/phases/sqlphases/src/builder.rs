/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use caching_ext::CacheHandlerFactory;
use commit_graph::ArcCommitGraph;
use memcache::KeyGen;
use metaconfig_types::OssRemoteDatabaseConfig;
use metaconfig_types::OssRemoteMetadataDatabaseConfig;
use metaconfig_types::RemoteDatabaseConfig;
use metaconfig_types::RemoteMetadataDatabaseConfig;
use mononoke_types::RepositoryId;
use phases::ArcPhases;
use rendezvous::MultiRendezVous;
use rendezvous::RendezVousOptions;
use rendezvous::RendezVousStats;
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
    connections: SqlConnections,
    caches: Arc<Caches>,
}

impl SqlPhasesBuilder {
    pub fn enable_caching(&mut self, cache_handler_factory: CacheHandlerFactory) {
        let caches = Caches::new(cache_handler_factory, Self::key_gen());
        self.caches = Arc::new(caches);
    }

    pub fn build(
        self,
        repo_id: RepositoryId,
        commit_graph: ArcCommitGraph,
        heads_fetcher: HeadsFetcher,
        rendezvous_options: RendezVousOptions,
    ) -> ArcPhases {
        let phases_store = self.phases_store(rendezvous_options);
        let phases = SqlPhases::new(phases_store, repo_id, commit_graph, heads_fetcher);
        Arc::new(phases)
    }

    fn key_gen() -> KeyGen {
        let key_prefix = "scm.mononoke.phases";
        KeyGen::new(key_prefix, MC_CODEVER, MC_SITEVER)
    }

    fn phases_store(self, rendezvous_options: RendezVousOptions) -> SqlPhasesStore {
        SqlPhasesStore {
            connections: self.connections,
            caches: self.caches,
            rendezvous: MultiRendezVous::new(
                rendezvous_options,
                RendezVousStats::new("mononoke.phases.sql".into()),
            ),
        }
    }
}

impl SqlConstruct for SqlPhasesBuilder {
    const LABEL: &'static str = "phases";

    const CREATION_QUERY: &'static str = include_str!("../schemas/sqlite-phases.sql");

    fn from_sql_connections(connections: SqlConnections) -> Self {
        let caches = Arc::new(Caches::new(CacheHandlerFactory::Noop, Self::key_gen()));
        Self {
            connections,
            caches,
        }
    }
}

impl SqlConstructFromMetadataDatabaseConfig for SqlPhasesBuilder {
    fn remote_database_config(
        remote: &RemoteMetadataDatabaseConfig,
    ) -> Option<&RemoteDatabaseConfig> {
        Some(&remote.production)
    }
    fn oss_remote_database_config(
        remote: &OssRemoteMetadataDatabaseConfig,
    ) -> Option<&OssRemoteDatabaseConfig> {
        Some(&remote.production)
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use anyhow::Error;
    use context::CoreContext;
    use context::PerfCounterType;
    use fbinit::FacebookInit;
    use maplit::hashset;
    use mononoke_macros::mononoke;
    use mononoke_types_mocks::changesetid::*;
    use phases::Phase;

    use super::*;

    #[mononoke::fbinit_test]
    async fn add_get_phase_sql_test(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo_id = RepositoryId::new(0);
        let phases_builder = SqlPhasesBuilder::with_sqlite_in_memory()?;
        let phases_store = phases_builder.phases_store(RendezVousOptions::for_test());

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

    #[mononoke::fbinit_test]
    async fn concurrent_phase_reads_share_a_query(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo_id = RepositoryId::new(0);
        let store = SqlPhasesBuilder::with_sqlite_in_memory()?.phases_store(RendezVousOptions {
            free_connections: 0,
            max_delay: Duration::from_secs(60),
            max_threshold: 3,
        });
        store
            .add_public_raw(&ctx, repo_id, vec![ONES_CSID, TWOS_CSID])
            .await?;
        let cloned_store = store.clone();

        // The three distinct keys trigger dispatch, including overlapping single and bulk reads.
        let (single, bulk, missing) = futures::try_join!(
            store.get_single_raw(&ctx, repo_id, ONES_CSID),
            cloned_store.get_public_raw(&ctx, repo_id, &[ONES_CSID, TWOS_CSID]),
            store.get_single_raw(&ctx, repo_id, THREES_CSID),
        )?;

        assert_eq!(single, Some(Phase::Public));
        assert_eq!(bulk, hashset! {ONES_CSID, TWOS_CSID});
        assert_eq!(missing, None);
        assert_eq!(
            ctx.perf_counters()
                .get_counter(PerfCounterType::SqlReadsReplica),
            1,
        );
        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn concurrent_phase_reads_keep_repositories_separate(
        fb: FacebookInit,
    ) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo_a = RepositoryId::new(0);
        let repo_b = RepositoryId::new(1);
        let store = SqlPhasesBuilder::with_sqlite_in_memory()?.phases_store(RendezVousOptions {
            free_connections: 0,
            max_delay: Duration::from_secs(60),
            max_threshold: 2,
        });
        store.add_public_raw(&ctx, repo_a, vec![ONES_CSID]).await?;
        store.add_public_raw(&ctx, repo_b, vec![TWOS_CSID]).await?;

        let (a_one, b_one, a_two, b_two) = futures::try_join!(
            store.get_single_raw(&ctx, repo_a, ONES_CSID),
            store.get_single_raw(&ctx, repo_b, ONES_CSID),
            store.get_single_raw(&ctx, repo_a, TWOS_CSID),
            store.get_single_raw(&ctx, repo_b, TWOS_CSID),
        )?;

        assert_eq!((a_one, a_two), (Some(Phase::Public), None));
        assert_eq!((b_one, b_two), (None, Some(Phase::Public)));
        assert_eq!(
            ctx.perf_counters()
                .get_counter(PerfCounterType::SqlReadsReplica),
            2,
        );
        Ok(())
    }
}
