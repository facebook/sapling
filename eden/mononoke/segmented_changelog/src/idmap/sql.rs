/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::format_err;
use anyhow::Error;
use anyhow::Result;
use async_trait::async_trait;
use sql::queries;
use sql::Connection;
use sql_ext::replication::ReplicaLagMonitor;
use sql_ext::replication::WaitForReplicationConfig;
use sql_ext::SqlConnections;

use stats::prelude::*;

use context::CoreContext;
use context::PerfCounterType;
use mononoke_types::ChangesetId;
use mononoke_types::RepositoryId;

use crate::idmap::IdMap;
use crate::types::IdMapVersion;
use crate::DagId;

define_stats! {
    prefix = "mononoke.segmented_changelog.idmap";
    insert: timeseries(Sum),
    find_changeset_id: timeseries(Sum),
    find_dag_id: timeseries(Sum),
    get_last_entry: timeseries(Sum),
    find_by_changeset_id_prefix: timeseries(Sum),
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

    write CopyIdMap(
        values: (repo_id: RepositoryId, new_version: IdMapVersion, source_version: IdMapVersion, copy_limit: u64)
    ) {
        insert_or_ignore,
        "
        {insert_or_ignore} INTO segmented_changelog_idmap_copy_mappings (repo_id, idmap_version, copied_version, copy_limit)
        VALUES {values}
        "
    }

    read GetIdMapCopySource(
        repo_id: RepositoryId,
        version: IdMapVersion,
    ) -> (IdMapVersion, u64) {
        "
        SELECT copied_version, copy_limit
        FROM segmented_changelog_idmap_copy_mappings
        WHERE repo_id = {repo_id} AND idmap_version = {version}
        "
    }

    read CheckCopyMapping(
        repo_id: RepositoryId,
        version: IdMapVersion,
        dag_id: u64,
    ) -> (IdMapVersion) {
        "
        SELECT idmap_version
        FROM segmented_changelog_idmap_copy_mappings
        WHERE repo_id = {repo_id} AND copied_version = {version} AND copy_limit >= {dag_id}
        LIMIT 1
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
            LEFT JOIN segmented_changelog_idmap_copy_mappings AS copy_mappings
            ON (idmap.repo_id = copy_mappings.repo_id AND idmap.version = copy_mappings.copied_version)
        WHERE
            idmap.repo_id = {repo_id} AND
            (idmap.version = {version} OR copy_mappings.idmap_version = {version}) AND
            (copy_mappings.copy_limit IS NULL OR copy_mappings.copy_limit >= idmap.vertex) AND
            idmap.vertex IN {dag_ids}
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
            LEFT JOIN segmented_changelog_idmap_copy_mappings AS copy_mappings
            ON (idmap.repo_id = copy_mappings.repo_id AND idmap.version = copy_mappings.copied_version)
        WHERE
            idmap.repo_id = {repo_id} AND
            (idmap.version = {version} OR copy_mappings.idmap_version = {version}) AND
            (copy_mappings.copy_limit IS NULL OR copy_mappings.copy_limit >= idmap.vertex) AND
            idmap.cs_id in {cs_ids}
        "
    }

    read SelectLastEntry(repo_id: RepositoryId, version: IdMapVersion) -> (u64, ChangesetId) {
        "
        SELECT idmap.vertex as vertex, idmap.cs_id as cs_id
        FROM segmented_changelog_idmap AS idmap
        WHERE
            idmap.repo_id = {repo_id} AND
            idmap.version = {version} AND
            idmap.vertex = (
                SELECT MAX(inx.vertex)
                FROM segmented_changelog_idmap AS inx
                WHERE
                    inx.repo_id = {repo_id} AND
                    inx.version = {version}
              )
        "
    }

    read SelectHighestCopyLimit(repo_id: RepositoryId, version: IdMapVersion) -> (Option<u64>,) {
        "
        SELECT MAX(copy_mappings.copy_limit)
        FROM segmented_changelog_idmap_copy_mappings AS copy_mappings
        WHERE
            copy_mappings.repo_id = {repo_id} AND
            copy_mappings.idmap_version = {version}
        "

    }

    read SelectHexPrefix(repo_id: RepositoryId, version: IdMapVersion, prefix: &[u8], limit: usize) -> (ChangesetId, u64) {
        "
        SELECT idmap.cs_id as cs_id, idmap.vertex as vertex
        FROM segmented_changelog_idmap AS idmap
            LEFT JOIN segmented_changelog_idmap_copy_mappings AS copy_mappings
            ON (idmap.repo_id = copy_mappings.repo_id AND idmap.version = copy_mappings.copied_version)
        WHERE
            idmap.repo_id = {repo_id} AND
            (idmap.version = {version} OR copy_mappings.idmap_version = {version}) AND
            (copy_mappings.copy_limit IS NULL OR copy_mappings.copy_limit >= idmap.vertex) AND
            HEX(cs_id) LIKE CONCAT({prefix}, '%')
        LIMIT {limit}
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

    pub async fn copy(&self, dag_limit: DagId, new_version: IdMapVersion) -> Result<Self, Error> {
        // Check that this version *has* a DagId at `dag_limit`
        if self
            .select_many_changesetids(&self.connections.write_connection, &[dag_limit.0])
            .await?
            .is_empty()
        {
            return Err(format_err!(
                "repo {} {:?} does not have DagId {}",
                self.repo_id,
                self.version,
                dag_limit.0
            ));
        }
        let new_self = Self::new(
            self.connections.clone(),
            self.replica_lag_monitor.clone(),
            self.repo_id,
            new_version,
        );
        let transaction = self
            .connections
            .write_connection
            .start_transaction()
            .await?;

        // Check copy sources for this version, and create new copy sources for new_version
        let (mut transaction, copy_sources) =
            GetIdMapCopySource::query_with_transaction(transaction, &self.repo_id, &self.version)
                .await?;
        for (version, limit) in copy_sources {
            let limit = dag_limit.0.min(limit);
            let (t, _) = CopyIdMap::query_with_transaction(
                transaction,
                &[(&self.repo_id, &new_version, &version, &limit)],
            )
            .await?;
            transaction = t;
            if limit == dag_limit.0 {
                // Copying existing copy mappings has covered everything. Stop copying here.
                transaction.commit().await?;
                return Ok(new_self);
            }
        }

        // Then, if not everything is covered, add this version as a copy source for new_version
        let (transaction, _) = CopyIdMap::query_with_transaction(
            transaction,
            &[(&self.repo_id, &new_version, &self.version, &dag_limit.0)],
        )
        .await?;
        transaction.commit().await?;
        Ok(new_self)
    }

    async fn check_copy_mapping(&self, conn: &Connection, lowest_id: &DagId) -> Result<()> {
        let copy_source =
            CheckCopyMapping::query(conn, &self.repo_id, &self.version, &lowest_id.0).await?;
        if let Some(copy_source) = copy_source.first() {
            return Err(format_err!(
                "repo {}: dag_ig {} in version {:?} is a copy baseline for version {:?}",
                self.repo_id,
                lowest_id,
                self.version,
                copy_source
            ));
        }
        Ok(())
    }
}

#[async_trait]
impl IdMap for SqlIdMap {
    async fn insert_many(
        &self,
        ctx: &CoreContext,
        mut mappings: Vec<(DagId, ChangesetId)>,
    ) -> Result<()> {
        STATS::insert.add_value(mappings.len() as i64);
        mappings.sort();

        // Sanity check for copy mappings - we want to reject inserts to vertex IDs that are
        // referenced by a copy mapping.
        // This is not perfect (races, replication lag), but should show bugs in tests
        if let Some(min) = mappings.iter().map(|(id, _cs)| id).min() {
            self.check_copy_mapping(&self.connections.read_connection, min)
                .await?;
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

    /// The maybe stale version of the method doesn't reach out beyond
    /// replica servers even if information is missing so it might ommit
    /// newest entries.
    async fn find_many_dag_ids_maybe_stale(
        &self,
        ctx: &CoreContext,
        cs_ids: Vec<ChangesetId>,
    ) -> Result<HashMap<ChangesetId, DagId>> {
        STATS::find_dag_id.add_value(cs_ids.len() as i64);
        ctx.perf_counters()
            .increment_counter(PerfCounterType::SqlReadsReplica);
        self.select_many_dag_ids(&self.connections.read_connection, &cs_ids)
            .await
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
        if rows.is_empty() {
            // This IdMap version doesn't yet have its own entries. Check to see if it's a copy
            ctx.perf_counters()
                .increment_counter(PerfCounterType::SqlReadsMaster);
            let highest_copy_limit = SelectHighestCopyLimit::query(
                &self.connections.write_connection,
                &self.repo_id,
                &self.version,
            )
            .await?
            .into_iter()
            .next()
            .map(|(r,)| r);

            if let Some(Some(highest_copy_limit)) = highest_copy_limit {
                // It is a copy, so take the last entry from the copy source instead
                ctx.perf_counters()
                    .increment_counter(PerfCounterType::SqlReadsMaster);
                let mut res = self
                    .select_many_changesetids(
                        &self.connections.write_connection,
                        &[highest_copy_limit],
                    )
                    .await?;
                Ok(res.remove_entry(&DagId(highest_copy_limit)))
            } else {
                Ok(None)
            }
        } else {
            Ok(rows.into_iter().next().map(|r| (DagId(r.0), r.1)))
        }
    }

    fn idmap_version(&self) -> Option<IdMapVersion> {
        Some(self.version)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use maplit::hashmap;
    use sql::rusqlite::Connection as SqliteConnection;
    use sql::Connection;

    use fbinit::FacebookInit;

    use mononoke_types_mocks::changesetid::AS_CSID;
    use mononoke_types_mocks::changesetid::BS_CSID;
    use mononoke_types_mocks::changesetid::FIVES_CSID;
    use mononoke_types_mocks::changesetid::FOURS_CSID;
    use mononoke_types_mocks::changesetid::ONES_CSID;
    use mononoke_types_mocks::changesetid::THREES_CSID;
    use mononoke_types_mocks::changesetid::TWOS_CSID;
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
    async fn test_find_many_dag_maybe_stale_no_leader_fallback(fb: FacebookInit) -> Result<()> {
        fn conn() -> Result<Connection> {
            let con = SqliteConnection::open_in_memory()?;
            con.execute_batch(SegmentedChangelogSqlConnections::CREATION_QUERY)?;
            Ok(Connection::with_sqlite(con))
        }

        let ctx = CoreContext::test_mock(fb);

        let leader = conn()?;
        let replica = conn()?;

        // We're setting those conns separately so we can craft replica content
        // and make it different from leader.
        let write_to_replica_conns = SqlConnections {
            write_connection: replica.clone(),
            read_connection: replica.clone(),
            read_master_connection: replica.clone(),
        };

        let write_to_replica_idmap = SqlIdMap::new(
            write_to_replica_conns,
            Arc::new(NoReplicaLagMonitor()),
            RepositoryId::new(0),
            IdMapVersion(0),
        );

        write_to_replica_idmap
            .insert(&ctx, DagId(0), AS_CSID)
            .await?;
        write_to_replica_idmap
            .insert_many(&ctx, vec![(DagId(1), ONES_CSID), (DagId(2), TWOS_CSID)])
            .await?;

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
            .find_many_dag_ids_maybe_stale(&ctx, vec![ONES_CSID, TWOS_CSID, THREES_CSID])
            .await?;
        assert_eq!(
            response,
            hashmap![ONES_CSID => DagId(1), TWOS_CSID => DagId(2)]
        );

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

    #[fbinit::test]
    async fn test_copy_map(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let conns = SegmentedChangelogSqlConnections::with_sqlite_in_memory()?;

        let idmap1 = SqlIdMap::new(
            conns.0.clone(),
            Arc::new(NoReplicaLagMonitor()),
            RepositoryId::new(1),
            IdMapVersion(1),
        );
        idmap1.insert(&ctx, DagId(0), AS_CSID).await?;
        // This way, if we don't truncate correctly, things will go wrong.
        idmap1.insert(&ctx, DagId(1), THREES_CSID).await?;
        idmap1.insert(&ctx, DagId(2), FOURS_CSID).await?;

        let idmap2 = idmap1.copy(DagId(0), IdMapVersion(2)).await?;
        // Check the copy didn't go too deep
        assert!(idmap2.get_changeset_id(&ctx, DagId(1)).await.is_err());
        assert!(idmap2.get_changeset_id(&ctx, DagId(2)).await.is_err());
        assert!(idmap2.get_dag_id(&ctx, THREES_CSID).await.is_err());
        assert!(idmap2.get_dag_id(&ctx, FOURS_CSID).await.is_err());

        idmap2.insert(&ctx, DagId(1), ONES_CSID).await?;
        idmap2.insert(&ctx, DagId(2), FIVES_CSID).await?;

        let idmap3 = idmap2.copy(DagId(1), IdMapVersion(3)).await?;
        // Check the copy didn't go too deep
        assert!(idmap3.get_changeset_id(&ctx, DagId(2)).await.is_err());
        assert!(idmap3.get_dag_id(&ctx, FOURS_CSID).await.is_err());

        idmap3.insert(&ctx, DagId(2), TWOS_CSID).await?;

        let idmap4 = idmap3.copy(DagId(2), IdMapVersion(4)).await?;
        idmap4.insert(&ctx, DagId(3), THREES_CSID).await?;

        // Check native table still works
        assert_eq!(idmap3.get_changeset_id(&ctx, DagId(2)).await?, TWOS_CSID);
        assert_eq!(idmap3.get_dag_id(&ctx, TWOS_CSID).await?, DagId(2));
        assert_eq!(idmap2.get_changeset_id(&ctx, DagId(1)).await?, ONES_CSID);
        assert_eq!(idmap2.get_dag_id(&ctx, ONES_CSID).await?, DagId(1));
        assert_eq!(idmap1.get_changeset_id(&ctx, DagId(0)).await?, AS_CSID);
        assert_eq!(idmap1.get_dag_id(&ctx, AS_CSID).await?, DagId(0));

        // And both levels of copy
        assert_eq!(idmap3.get_changeset_id(&ctx, DagId(1)).await?, ONES_CSID);
        assert_eq!(idmap3.get_dag_id(&ctx, ONES_CSID).await?, DagId(1));
        assert_eq!(idmap3.get_changeset_id(&ctx, DagId(0)).await?, AS_CSID);
        assert_eq!(idmap3.get_dag_id(&ctx, AS_CSID).await?, DagId(0));
        assert_eq!(idmap2.get_changeset_id(&ctx, DagId(0)).await?, AS_CSID);
        assert_eq!(idmap2.get_dag_id(&ctx, AS_CSID).await?, DagId(0));

        // IdMap 4 is needed to catch cases where the copy doesn't go deep enough
        // Just check that it's fine
        assert_eq!(idmap4.get_changeset_id(&ctx, DagId(0)).await?, AS_CSID);
        assert_eq!(idmap4.get_dag_id(&ctx, AS_CSID).await?, DagId(0));
        assert_eq!(idmap4.get_changeset_id(&ctx, DagId(1)).await?, ONES_CSID);
        assert_eq!(idmap4.get_dag_id(&ctx, ONES_CSID).await?, DagId(1));
        assert_eq!(idmap4.get_changeset_id(&ctx, DagId(2)).await?, TWOS_CSID);
        assert_eq!(idmap4.get_dag_id(&ctx, TWOS_CSID).await?, DagId(2));
        assert_eq!(idmap4.get_changeset_id(&ctx, DagId(3)).await?, THREES_CSID);
        assert_eq!(idmap4.get_dag_id(&ctx, THREES_CSID).await?, DagId(3));

        Ok(())
    }

    #[fbinit::test]
    async fn test_insert_fail_after_copy_map(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let conns = SegmentedChangelogSqlConnections::with_sqlite_in_memory()?;

        let idmap1 = SqlIdMap::new(
            conns.0.clone(),
            Arc::new(NoReplicaLagMonitor()),
            RepositoryId::new(1),
            IdMapVersion(1),
        );
        idmap1.insert(&ctx, DagId(0), AS_CSID).await?;
        idmap1.insert(&ctx, DagId(2), BS_CSID).await?;

        let idmap2 = idmap1.copy(DagId(2), IdMapVersion(2)).await?;
        idmap2.insert(&ctx, DagId(3), ONES_CSID).await?;

        assert!(
            idmap1.insert(&ctx, DagId(1), TWOS_CSID).await.is_err(),
            "Inserted into a copy base table"
        );
        Ok(())
    }
}
