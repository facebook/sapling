/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{format_err, Context, Error, Result};
use async_trait::async_trait;
use sql::{queries, Connection};
use sql_ext::{
    replication::{ReplicaLagMonitor, WaitForReplicationConfig},
    SqlConnections,
};

use stats::prelude::*;

use context::{CoreContext, PerfCounterType};
use mononoke_types::{ChangesetId, RepositoryId};

use crate::idmap::IdMap;
use crate::types::IdMapVersion;
use crate::DagId;

define_stats! {
    prefix = "mononoke.segmented_changelog.idmap";
    insert: timeseries(Sum),
    find_changeset_id: timeseries(Sum),
    find_dag_id: timeseries(Sum),
    get_last_entry: timeseries(Sum),
}

const INSERT_MAX: usize = 1_000;

pub struct SqlIdMap {
    connections: SqlConnections,
    replica_lag_monitor: Arc<dyn ReplicaLagMonitor>,
    repo_id: RepositoryId,
    version: IdMapVersion,
}

queries! {
    write InsertIdMapEntry(
        values: (repo_id: RepositoryId, version: IdMapVersion, dag_id: u64, cs_id: ChangesetId)
    ) {
        insert_or_ignore,
        "
        {insert_or_ignore} INTO segmented_changelog_idmap (repo_id, version, vertex, cs_id)
        VALUES {values}
        "
    }

    read SelectChangesetId(
        repo_id: RepositoryId,
        version: IdMapVersion,
        dag_id: u64
    ) -> (ChangesetId) {
        "
        SELECT idmap.cs_id as cs_id
        FROM segmented_changelog_idmap AS idmap
        WHERE idmap.repo_id = {repo_id} AND idmap.version = {version} AND idmap.vertex = {dag_id}
        "
    }

    read SelectManyChangesetIds(
        repo_id: RepositoryId,
        version: IdMapVersion,
        >list dag_ids: u64
    ) -> (u64, ChangesetId) {
        "
        SELECT idmap.vertex as vertex, idmap.cs_id as cs_id
        FROM segmented_changelog_idmap AS idmap
        WHERE idmap.repo_id = {repo_id} AND idmap.version = {version} AND idmap.vertex IN {dag_ids}
        "
    }

    read SelectManyDagIds(
        repo_id: RepositoryId,
        version: IdMapVersion,
        >list cs_ids: ChangesetId
    ) -> (ChangesetId, u64) {
        "
        SELECT idmap.cs_id as cs_id, idmap.vertex as vertex
        FROM segmented_changelog_idmap AS idmap
        WHERE idmap.repo_id = {repo_id} AND idmap.version = {version} AND idmap.cs_id in {cs_ids}
        "
    }

    read SelectLastEntry(repo_id: RepositoryId, version: IdMapVersion) -> (u64, ChangesetId) {
        "
        SELECT idmap.vertex as vertex, idmap.cs_id as cs_id
        FROM segmented_changelog_idmap AS idmap
        WHERE idmap.repo_id = {repo_id} AND idmap.version = {version} AND idmap.vertex = (
            SELECT MAX(inx.vertex)
            FROM segmented_changelog_idmap AS inx
            WHERE inx.repo_id = {repo_id} AND inx.version = {version}
        )
        "
    }
}

impl SqlIdMap {
    pub fn new(
        connections: SqlConnections,
        replica_lag_monitor: Arc<dyn ReplicaLagMonitor>,
        repo_id: RepositoryId,
        version: IdMapVersion,
    ) -> Self {
        Self {
            connections,
            replica_lag_monitor,
            repo_id,
            version,
        }
    }

    async fn select_many_changesetids(
        &self,
        conn: &Connection,
        dag_ids: &[u64],
    ) -> Result<HashMap<DagId, ChangesetId>, Error> {
        if dag_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let rows =
            SelectManyChangesetIds::query(conn, &self.repo_id, &self.version, dag_ids).await?;
        Ok::<_, Error>(rows.into_iter().map(|row| (DagId(row.0), row.1)).collect())
    }

    async fn select_many_dag_ids(
        &self,
        conn: &Connection,
        cs_ids: &[ChangesetId],
    ) -> Result<HashMap<ChangesetId, DagId>, Error> {
        if cs_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let rows = SelectManyDagIds::query(conn, &self.repo_id, &self.version, cs_ids).await?;
        Ok(rows.into_iter().map(|row| (row.0, DagId(row.1))).collect())
    }
}

#[async_trait]
impl IdMap for SqlIdMap {
    async fn insert_many(
        &self,
        ctx: &CoreContext,
        mut mappings: Vec<(DagId, ChangesetId)>,
    ) -> Result<()> {
        // On correctness. This code is slightly coupled with the IdMap update algorithm.
        // We need to ensure algorithm correctness with multiple writers and potential failures.
        // We need to "throttle" writes to prevent replication lag so big transaction are
        // undesirable.
        //
        // The IdMap update happens before the IdDag update so if a process in killed in between
        // those two steps, the update algorithm has to handle "a lagging" IdDag. The last IdDag
        // computed may have fewer commits processed than the database IdMap.
        //
        // Since we cannot do updates in one transaction the IdMap may have partial data in the
        // database from an update. To help with this problem we insert IdMap entries in increasing
        // order by DagId. This results in the invariant that all DagIds between 1 and
        // last_dag_id are assigned. This means that the IdMap algorithm may have to deal with
        // multiple "heads".
        //
        // Let's look at the situation where we have multiple update processes that start from
        // different commits then race to update the database. If they insert the same results we
        // may choose to be optimistic and allow them both to proceed with their process until some
        // difference is encountered. Updating dag_ids in order should leave the IdMap in a state
        // that already has to be handled. That said, being pessimistic is easier to reason about
        // so we rollback the transaction if any dag_id in our batch is already present. What may
        // happen is that one process updates a batch and second process starts a new update and
        // wins the race to update. The first process aborts and we are in a state that we
        // previously described as a requirement for the update algorithm.
        STATS::insert.add_value(mappings.len() as i64);
        mappings.sort();

        // Ensure that we have no gaps in the assignments in the IdMap by validating that mappings
        // has consecutive DagIds and they start with last_dag_id+1.
        // This isn't a great place for these checks. I feel pretty clowny adding them here but
        // they don't hurt. Might remove them later.
        if let Some(&(first, _)) = mappings.first() {
            if let Some(&(last, _)) = mappings.last() {
                if first + mappings.len() as u64 != last + 1 {
                    return Err(format_err!(
                        "repo {}: mappings sent for insertion are not consecutive",
                        self.repo_id
                    ));
                }
            }
            match self
                .get_last_entry(ctx)
                .await
                .context("error fetching last entry")?
            {
                None => {
                    if first.0 != 0 {
                        return Err(format_err!(
                            "repo {}: first dag_id being inserted into idmap is not 0 ({})",
                            self.repo_id,
                            first,
                        ));
                    }
                }
                Some((last_stored, _)) => {
                    if first != last_stored + 1 {
                        return Err(format_err!(
                            "repo {}: smallest dag_id being inserted does not follow last entry \
                             ({} + 1 != {})",
                            self.repo_id,
                            last_stored,
                            first
                        ));
                    }
                }
            }
        }


        // With validation passed, we split the mappings into batches that we write in separate
        // transactions.
        for (i, chunk) in mappings.chunks(INSERT_MAX).enumerate() {
            if i > 0 {
                let wait_config = WaitForReplicationConfig::default().with_logger(ctx.logger());
                self.replica_lag_monitor
                    .wait_for_replication(&wait_config)
                    .await?;
            }
            let mut to_insert = Vec::with_capacity(chunk.len());
            for (dag_id, cs_id) in chunk {
                to_insert.push((&self.repo_id, &self.version, &dag_id.0, cs_id));
            }
            ctx.perf_counters()
                .increment_counter(PerfCounterType::SqlWrites);
            let mut transaction = self
                .connections
                .write_connection
                .start_transaction()
                .await?;
            let query_result =
                InsertIdMapEntry::query_with_transaction(transaction, &to_insert).await;
            match query_result {
                Err(err) => {
                    // transaction is "lost" to the query
                    return Err(err.context(format_err!(
                        "repo {}: failed inserting IdMap entries",
                        self.repo_id
                    )));
                }
                Ok((t, insert_result)) => {
                    transaction = t;
                    if insert_result.affected_rows() != chunk.len() as u64 {
                        transaction.rollback().await?;
                        return Err(format_err!(
                            "repo {}: failed insert race, total entries {}, batch {}",
                            self.repo_id,
                            mappings.len(),
                            i
                        ));
                    } else {
                        transaction.commit().await?;
                    }
                }
            }
        }
        Ok(())
    }

    async fn find_many_changeset_ids(
        &self,
        ctx: &CoreContext,
        dag_ids: Vec<DagId>,
    ) -> Result<HashMap<DagId, ChangesetId>> {
        STATS::find_changeset_id.add_value(dag_ids.len() as i64);
        ctx.perf_counters()
            .increment_counter(PerfCounterType::SqlReadsReplica);
        let to_query: Vec<_> = dag_ids.iter().map(|v| v.0).collect();
        let mut cs_ids = self
            .select_many_changesetids(&self.connections.read_connection, &to_query)
            .await?;
        let not_found_in_replica: Vec<_> = dag_ids
            .iter()
            .filter(|x| !cs_ids.contains_key(x))
            .map(|v| v.0)
            .collect();
        if !not_found_in_replica.is_empty() {
            ctx.perf_counters()
                .increment_counter(PerfCounterType::SqlReadsMaster);
            let from_master = self
                .select_many_changesetids(
                    &self.connections.read_master_connection,
                    &not_found_in_replica,
                )
                .await?;
            for (k, v) in from_master {
                cs_ids.insert(k, v);
            }
        }
        Ok(cs_ids)
    }



    async fn find_many_dag_ids(
        &self,
        ctx: &CoreContext,
        cs_ids: Vec<ChangesetId>,
    ) -> Result<HashMap<ChangesetId, DagId>> {
        STATS::find_dag_id.add_value(cs_ids.len() as i64);
        ctx.perf_counters()
            .increment_counter(PerfCounterType::SqlReadsReplica);
        let mut dag_ids = self
            .select_many_dag_ids(&self.connections.read_connection, &cs_ids)
            .await?;
        let not_found_in_replica: Vec<_> = cs_ids
            .iter()
            .filter(|x| !dag_ids.contains_key(x))
            .cloned()
            .collect();
        if !not_found_in_replica.is_empty() {
            ctx.perf_counters()
                .increment_counter(PerfCounterType::SqlReadsMaster);
            let from_master = self
                .select_many_dag_ids(
                    &self.connections.read_master_connection,
                    &not_found_in_replica,
                )
                .await?;
            for (k, v) in from_master {
                dag_ids.insert(k, v);
            }
        }
        Ok(dag_ids)
    }

    async fn get_last_entry(&self, ctx: &CoreContext) -> Result<Option<(DagId, ChangesetId)>> {
        STATS::get_last_entry.add_value(1);
        ctx.perf_counters()
            .increment_counter(PerfCounterType::SqlReadsMaster);
        // From the update algorithm perspective, it makes most sense to read from master. Because
        // trying to insert a value that was already inserted will fail the whole processing an
        // outdated entry will definitely lead to wasted work.
        let rows = SelectLastEntry::query(
            &self.connections.write_connection,
            &self.repo_id,
            &self.version,
        )
        .await?;
        Ok(rows.into_iter().next().map(|r| (DagId(r.0), r.1)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use maplit::hashmap;
    use sql::{rusqlite::Connection as SqliteConnection, Connection};

    use fbinit::FacebookInit;

    use mononoke_types_mocks::changesetid::{
        AS_CSID, FIVES_CSID, FOURS_CSID, ONES_CSID, THREES_CSID, TWOS_CSID,
    };
    use sql_construct::SqlConstruct;
    use sql_ext::replication::NoReplicaLagMonitor;

    use crate::builder::SegmentedChangelogSqlConnections;

    fn new_sql_idmap() -> Result<SqlIdMap> {
        let conns = SegmentedChangelogSqlConnections::with_sqlite_in_memory()?;
        Ok(SqlIdMap::new(
            conns.0,
            Arc::new(NoReplicaLagMonitor()),
            RepositoryId::new(0),
            IdMapVersion(0),
        ))
    }

    #[fbinit::test]
    async fn test_get_last_entry(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let idmap = new_sql_idmap()?;

        assert_eq!(idmap.get_last_entry(&ctx).await?, None);

        idmap.insert(&ctx, DagId(0), AS_CSID).await?;
        idmap.insert(&ctx, DagId(1), ONES_CSID).await?;
        idmap.insert(&ctx, DagId(2), TWOS_CSID).await?;
        idmap.insert(&ctx, DagId(3), THREES_CSID).await?;

        assert_eq!(
            idmap.get_last_entry(&ctx).await?,
            Some((DagId(3), THREES_CSID))
        );

        Ok(())
    }

    #[fbinit::test]
    async fn test_insert_many(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let idmap = new_sql_idmap()?;

        assert_eq!(idmap.get_last_entry(&ctx).await?, None);

        idmap.insert_many(&ctx, vec![]).await?;
        assert!(idmap.get_changeset_id(&ctx, DagId(1)).await.is_err());

        idmap.insert(&ctx, DagId(0), AS_CSID).await?;
        idmap
            .insert_many(
                &ctx,
                vec![
                    (DagId(1), ONES_CSID),
                    (DagId(2), TWOS_CSID),
                    (DagId(3), THREES_CSID),
                ],
            )
            .await?;

        assert_eq!(idmap.get_changeset_id(&ctx, DagId(1)).await?, ONES_CSID);
        assert_eq!(idmap.get_changeset_id(&ctx, DagId(3)).await?, THREES_CSID);

        assert!(
            idmap
                .insert_many(
                    &ctx,
                    vec![
                        (DagId(1), ONES_CSID),
                        (DagId(2), TWOS_CSID),
                        (DagId(3), THREES_CSID),
                    ],
                )
                .await
                .is_err()
        );

        idmap
            .insert_many(&ctx, vec![(DagId(4), FOURS_CSID)])
            .await?;
        assert_eq!(idmap.get_changeset_id(&ctx, DagId(4)).await?, FOURS_CSID);

        assert!(
            idmap
                .insert_many(&ctx, vec![(DagId(1), FIVES_CSID)])
                .await
                .is_err()
        );

        Ok(())
    }

    #[fbinit::test]
    async fn test_find_many_changeset_ids(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let idmap = new_sql_idmap()?;

        let response = idmap
            .find_many_changeset_ids(&ctx, vec![DagId(1), DagId(2), DagId(3), DagId(6)])
            .await?;
        assert!(response.is_empty());

        idmap.insert(&ctx, DagId(0), AS_CSID).await?;
        idmap
            .insert_many(
                &ctx,
                vec![
                    (DagId(1), ONES_CSID),
                    (DagId(2), TWOS_CSID),
                    (DagId(3), THREES_CSID),
                    (DagId(4), FOURS_CSID),
                    (DagId(5), FIVES_CSID),
                ],
            )
            .await?;

        let response = idmap
            .find_many_changeset_ids(&ctx, vec![DagId(1), DagId(2), DagId(3), DagId(6)])
            .await?;
        assert_eq!(
            response,
            hashmap![DagId(1) => ONES_CSID, DagId(2) => TWOS_CSID, DagId(3) => THREES_CSID]
        );

        let response = idmap
            .find_many_changeset_ids(&ctx, vec![DagId(4), DagId(5)])
            .await?;
        assert_eq!(
            response,
            hashmap![DagId(4) => FOURS_CSID, DagId(5) => FIVES_CSID]
        );

        let response = idmap.find_many_changeset_ids(&ctx, vec![DagId(6)]).await?;
        assert!(response.is_empty());

        Ok(())
    }

    #[fbinit::test]
    async fn test_find_many_changeset_ids_leader_fallback(fb: FacebookInit) -> Result<()> {
        fn conn() -> Result<Connection> {
            let con = SqliteConnection::open_in_memory()?;
            con.execute_batch(SegmentedChangelogSqlConnections::CREATION_QUERY)?;
            Ok(Connection::with_sqlite(con))
        }

        let ctx = CoreContext::test_mock(fb);

        let leader = conn()?;
        let replica = conn()?;

        let conns = SqlConnections {
            write_connection: leader.clone(),
            read_connection: replica,
            read_master_connection: leader,
        };

        let idmap = SqlIdMap::new(
            conns,
            Arc::new(NoReplicaLagMonitor()),
            RepositoryId::new(0),
            IdMapVersion(0),
        );

        idmap.insert(&ctx, DagId(0), ONES_CSID).await?;

        let res = idmap.get_changeset_id(&ctx, DagId(0)).await?;
        assert_eq!(res, ONES_CSID);

        let res = idmap.find_many_changeset_ids(&ctx, vec![DagId(0)]).await?;
        assert_eq!(res, hashmap![DagId(0) => ONES_CSID]);

        Ok(())
    }

    #[fbinit::test]
    async fn test_find_many_dag_ids(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let idmap = new_sql_idmap()?;

        let response = idmap
            .find_many_dag_ids(&ctx, vec![ONES_CSID, TWOS_CSID, THREES_CSID, FOURS_CSID])
            .await?;
        assert!(response.is_empty());

        idmap.insert(&ctx, DagId(0), AS_CSID).await?;
        idmap
            .insert_many(
                &ctx,
                vec![
                    (DagId(1), ONES_CSID),
                    (DagId(2), TWOS_CSID),
                    (DagId(3), THREES_CSID),
                    (DagId(4), FOURS_CSID),
                ],
            )
            .await?;

        let response = idmap
            .find_many_dag_ids(&ctx, vec![ONES_CSID, TWOS_CSID, THREES_CSID])
            .await?;
        assert_eq!(
            response,
            hashmap![ONES_CSID => DagId(1), TWOS_CSID => DagId(2), THREES_CSID => DagId(3)]
        );

        let response = idmap
            .find_many_dag_ids(&ctx, vec![FOURS_CSID, FIVES_CSID])
            .await?;
        assert_eq!(response, hashmap![FOURS_CSID => DagId(4)]);

        let response = idmap.find_many_dag_ids(&ctx, vec![FIVES_CSID]).await?;
        assert!(response.is_empty());

        Ok(())
    }

    #[fbinit::test]
    async fn test_many_repo_id_many_versions(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let conns = SegmentedChangelogSqlConnections::with_sqlite_in_memory()?;

        let idmap11 = SqlIdMap::new(
            conns.0.clone(),
            Arc::new(NoReplicaLagMonitor()),
            RepositoryId::new(1),
            IdMapVersion(1),
        );
        let idmap12 = SqlIdMap::new(
            conns.0.clone(),
            Arc::new(NoReplicaLagMonitor()),
            RepositoryId::new(1),
            IdMapVersion(2),
        );
        let idmap21 = SqlIdMap::new(
            conns.0.clone(),
            Arc::new(NoReplicaLagMonitor()),
            RepositoryId::new(2),
            IdMapVersion(1),
        );

        idmap11.insert(&ctx, DagId(0), AS_CSID).await?;
        idmap12.insert(&ctx, DagId(0), AS_CSID).await?;
        idmap21.insert(&ctx, DagId(0), AS_CSID).await?;

        idmap11.insert(&ctx, DagId(1), ONES_CSID).await?;
        idmap11.insert(&ctx, DagId(2), TWOS_CSID).await?;
        idmap12.insert(&ctx, DagId(1), TWOS_CSID).await?;
        idmap21.insert(&ctx, DagId(1), FOURS_CSID).await?;
        idmap21.insert(&ctx, DagId(2), ONES_CSID).await?;

        assert_eq!(idmap11.get_changeset_id(&ctx, DagId(1)).await?, ONES_CSID);
        assert_eq!(idmap11.get_changeset_id(&ctx, DagId(2)).await?, TWOS_CSID);
        assert_eq!(idmap12.get_changeset_id(&ctx, DagId(1)).await?, TWOS_CSID);
        assert_eq!(idmap21.get_changeset_id(&ctx, DagId(1)).await?, FOURS_CSID);
        assert_eq!(idmap21.get_changeset_id(&ctx, DagId(2)).await?, ONES_CSID);

        assert_eq!(idmap11.get_dag_id(&ctx, ONES_CSID).await?, DagId(1));
        assert_eq!(idmap11.get_dag_id(&ctx, TWOS_CSID).await?, DagId(2));

        Ok(())
    }

    #[fbinit::test]
    async fn test_many_fetch_fn_with_no_entries(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let idmap = new_sql_idmap()?;

        assert!(idmap.find_many_dag_ids(&ctx, vec![]).await?.is_empty());
        assert!(
            idmap
                .find_many_changeset_ids(&ctx, vec![])
                .await?
                .is_empty()
        );

        Ok(())
    }
}
