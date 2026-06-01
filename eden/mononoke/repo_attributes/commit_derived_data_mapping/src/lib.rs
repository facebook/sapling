/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use anyhow::Result;
use anyhow::anyhow;
use context::CoreContext;
use metaconfig_types::OssRemoteDatabaseConfig;
use metaconfig_types::OssRemoteMetadataDatabaseConfig;
use metaconfig_types::RemoteMetadataDatabaseConfig;
use metaconfig_types::ShardableRemoteDatabaseConfig;
use mononoke_types::ChangesetId;
use mononoke_types::DerivableType;
use mononoke_types::RepositoryId;
use sql_construct::SqlShardableConstructFromMetadataDatabaseConfig;
use sql_construct::SqlShardedConstruct;
use sql_ext::Connection;
use sql_ext::SqlShardedConnections;
use sql_ext::mononoke_queries;
use vec1::Vec1;

pub const MYSQL_INSERT_CHUNK_SIZE: usize = 1000;

mononoke_queries! {
    write InsertMappings(values: (
        repo_id: RepositoryId,
        cs_id: ChangesetId,
        derived_data_type: i32,
        derived_data_version: i32,
        derived_value: Vec<u8>
    )) {
        insert_or_ignore,
        "{insert_or_ignore} INTO commit_derived_data (repo_id, cs_id, derived_data_type, derived_data_version, derived_value) VALUES {values}"
    }

    read SelectMapping(
        repo_id: RepositoryId,
        cs_id: ChangesetId,
        derived_data_type: i32,
        derived_data_version: i32
    ) -> (Vec<u8>,) {
        "SELECT derived_value FROM commit_derived_data
         WHERE repo_id = {repo_id}
           AND cs_id = {cs_id}
           AND derived_data_type = {derived_data_type}
           AND derived_data_version = {derived_data_version}"
    }

    read SelectMappingBatch(
        repo_id: RepositoryId,
        derived_data_type: i32,
        derived_data_version: i32,
        >list cs_ids: ChangesetId
    ) -> (ChangesetId, Vec<u8>) {
        "SELECT cs_id, derived_value FROM commit_derived_data
         WHERE repo_id = {repo_id}
           AND cs_id IN {cs_ids}
           AND derived_data_type = {derived_data_type}
           AND derived_data_version = {derived_data_version}"
    }
}

/// Mapping of (repo_id, changeset_id, derived_data_type, version) -> derived_value
/// stored in XDB, sharded by derived data type.
///
/// The caller is responsible for providing the correct connection index
/// (from `xdb_mapping_shard_ids` config) for each derived data type.
#[facet::facet]
pub struct CommitDerivedDataMapping {
    pub sql: SqlCommitDerivedDataMapping,
}

impl CommitDerivedDataMapping {
    pub async fn store_mapping(
        &self,
        ctx: &CoreContext,
        repo_id: RepositoryId,
        cs_id: ChangesetId,
        derived_data_type: DerivableType,
        derived_data_version: i32,
        derived_value: &[u8],
        shard_id: usize,
    ) -> Result<()> {
        self.sql
            .store_mapping(
                ctx,
                repo_id,
                cs_id,
                derived_data_type,
                derived_data_version,
                derived_value,
                shard_id,
            )
            .await
    }

    /// Store a batch of mappings, all for the same derived data type
    /// and targeting the same connection.
    pub async fn store_mapping_batch(
        &self,
        ctx: &CoreContext,
        repo_id: RepositoryId,
        entries: Vec<(ChangesetId, i32, Vec<u8>)>,
        derived_data_type: DerivableType,
        derived_data_version: i32,
        shard_id: usize,
    ) -> Result<u64> {
        self.sql
            .store_mapping_batch(
                ctx,
                repo_id,
                entries,
                derived_data_type,
                derived_data_version,
                shard_id,
            )
            .await
    }

    pub async fn fetch_mapping(
        &self,
        ctx: &CoreContext,
        repo_id: RepositoryId,
        cs_id: ChangesetId,
        derived_data_type: DerivableType,
        derived_data_version: i32,
        shard_id: usize,
    ) -> Result<Option<Vec<u8>>> {
        self.sql
            .fetch_mapping(
                ctx,
                repo_id,
                cs_id,
                derived_data_type,
                derived_data_version,
                shard_id,
            )
            .await
    }

    pub async fn fetch_mapping_batch(
        &self,
        ctx: &CoreContext,
        repo_id: RepositoryId,
        cs_ids: Vec<ChangesetId>,
        derived_data_type: DerivableType,
        derived_data_version: i32,
        shard_id: usize,
    ) -> Result<Vec<(ChangesetId, Vec<u8>)>> {
        self.sql
            .fetch_mapping_batch(
                ctx,
                repo_id,
                cs_ids,
                derived_data_type,
                derived_data_version,
                shard_id,
            )
            .await
    }
}

pub struct SqlCommitDerivedDataMapping {
    write_connections: Arc<Vec1<Connection>>,
    read_connections: Arc<Vec1<Connection>>,
}

impl SqlCommitDerivedDataMapping {
    fn derived_data_type_id(dt: DerivableType) -> i32 {
        i32::from(dt.into_thrift())
    }

    fn shard_id(&self, id: usize) -> Result<usize> {
        let count = self.read_connections.len();
        if id >= count {
            return Err(anyhow!(
                "connection index {id} out of range (have {count} connections)",
            ));
        }
        Ok(id)
    }

    pub async fn store_mapping(
        &self,
        ctx: &CoreContext,
        repo_id: RepositoryId,
        cs_id: ChangesetId,
        derived_data_type: DerivableType,
        derived_data_version: i32,
        derived_value: &[u8],
        shard_id: usize,
    ) -> Result<()> {
        let idx = self.shard_id(shard_id)?;
        let type_id = Self::derived_data_type_id(derived_data_type);
        let value_vec = derived_value.to_vec();
        let values: Vec<_> = vec![(
            &repo_id,
            &cs_id,
            &type_id,
            &derived_data_version,
            &value_vec,
        )];
        InsertMappings::query(
            &self.write_connections[idx],
            ctx.sql_query_telemetry(),
            &values[..],
        )
        .await?;
        Ok(())
    }

    /// Store a batch of mappings, all for the same derived data type
    /// and targeting the same connection.
    pub async fn store_mapping_batch(
        &self,
        ctx: &CoreContext,
        repo_id: RepositoryId,
        entries: Vec<(ChangesetId, i32, Vec<u8>)>,
        derived_data_type: DerivableType,
        derived_data_version: i32,
        shard_id: usize,
    ) -> Result<u64> {
        if entries.is_empty() {
            return Ok(0);
        }

        let idx = self.shard_id(shard_id)?;
        let type_id = Self::derived_data_type_id(derived_data_type);

        let prepared: Vec<_> = entries
            .iter()
            .map(|(cs_id, _version, value)| {
                (
                    repo_id,
                    *cs_id,
                    type_id,
                    derived_data_version,
                    value.clone(),
                )
            })
            .collect();

        let mut affected_rows = 0;
        for chunk in prepared.chunks(MYSQL_INSERT_CHUNK_SIZE) {
            // This pattern is used to convert a ref to tuple into a tuple of refs.
            #[allow(clippy::map_identity)]
            let chunk: Vec<_> = chunk
                .iter()
                .map(|(a, b, c, d, e)| (a, b, c, d, e))
                .collect();
            let result = InsertMappings::query(
                &self.write_connections[idx],
                ctx.sql_query_telemetry(),
                &chunk[..],
            )
            .await?;
            affected_rows += result.affected_rows();
        }
        Ok(affected_rows)
    }

    pub async fn fetch_mapping(
        &self,
        ctx: &CoreContext,
        repo_id: RepositoryId,
        cs_id: ChangesetId,
        derived_data_type: DerivableType,
        derived_data_version: i32,
        shard_id: usize,
    ) -> Result<Option<Vec<u8>>> {
        let idx = self.shard_id(shard_id)?;
        let type_id = Self::derived_data_type_id(derived_data_type);
        let rows = SelectMapping::query(
            &self.read_connections[idx],
            ctx.sql_query_telemetry(),
            &repo_id,
            &cs_id,
            &type_id,
            &derived_data_version,
        )
        .await?;
        Ok(rows.into_iter().next().map(|(v,)| v))
    }

    pub async fn fetch_mapping_batch(
        &self,
        ctx: &CoreContext,
        repo_id: RepositoryId,
        cs_ids: Vec<ChangesetId>,
        derived_data_type: DerivableType,
        derived_data_version: i32,
        shard_id: usize,
    ) -> Result<Vec<(ChangesetId, Vec<u8>)>> {
        let idx = self.shard_id(shard_id)?;
        let type_id = Self::derived_data_type_id(derived_data_type);
        SelectMappingBatch::query(
            &self.read_connections[idx],
            ctx.sql_query_telemetry(),
            &repo_id,
            &type_id,
            &derived_data_version,
            &cs_ids[..],
        )
        .await
    }
}

impl SqlShardedConstruct for SqlCommitDerivedDataMapping {
    const LABEL: &'static str = "commit_derived_data_mapping";

    const CREATION_QUERY: &'static str = include_str!("../schemas/sqlite-commit-derived-data.sql");

    fn from_sql_shard_connections(connections: SqlShardedConnections) -> Self {
        let SqlShardedConnections {
            read_connections,
            read_master_connections: _,
            write_connections,
        } = connections;

        Self {
            write_connections: Arc::new(write_connections),
            read_connections: Arc::new(read_connections),
        }
    }
}

impl SqlShardableConstructFromMetadataDatabaseConfig for SqlCommitDerivedDataMapping {
    fn remote_database_config(
        remote: &RemoteMetadataDatabaseConfig,
    ) -> Option<&ShardableRemoteDatabaseConfig> {
        remote.commit_derived_data_mapping.as_ref()
    }
    fn oss_remote_database_config(
        remote: &OssRemoteMetadataDatabaseConfig,
    ) -> Option<&OssRemoteDatabaseConfig> {
        remote.commit_derived_data_mapping.as_ref()
    }
}

#[cfg(test)]
mod test {
    use std::collections::HashMap;

    use fbinit::FacebookInit;
    use mononoke_macros::mononoke;
    use mononoke_types::DerivableType;
    use mononoke_types_mocks::changesetid::ONES_CSID;
    use mononoke_types_mocks::changesetid::THREES_CSID;
    use mononoke_types_mocks::changesetid::TWOS_CSID;
    use sql_construct::SqlConstruct;

    use super::*;

    // In-memory SQLite has a single connection, so shard_id is always 0.
    const CONN: usize = 0;

    #[mononoke::fbinit_test]
    async fn test_single_write_and_read(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let sql = SqlCommitDerivedDataMapping::with_sqlite_in_memory()?;
        let repo_id = RepositoryId::new(1);
        let dt = DerivableType::Fsnodes;
        let version = 1;
        let value = vec![1u8; 32];

        sql.store_mapping(&ctx, repo_id, ONES_CSID, dt, version, &value, CONN)
            .await?;

        let result = sql
            .fetch_mapping(&ctx, repo_id, ONES_CSID, dt, version, CONN)
            .await?;
        assert_eq!(result, Some(value.clone()));

        // Non-existent mapping returns None
        let result = sql
            .fetch_mapping(&ctx, repo_id, TWOS_CSID, dt, version, CONN)
            .await?;
        assert_eq!(result, None);

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_batch_write_and_read(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let sql = SqlCommitDerivedDataMapping::with_sqlite_in_memory()?;
        let repo_id = RepositoryId::new(1);
        let dt = DerivableType::Fsnodes;
        let version = 1;
        let value1 = vec![1u8; 32];
        let value2 = vec![2u8; 32];
        let value3 = vec![3u8; 32];

        let entries = vec![
            (ONES_CSID, version, value1.clone()),
            (TWOS_CSID, version, value2.clone()),
            (THREES_CSID, version, value3.clone()),
        ];

        let affected = sql
            .store_mapping_batch(&ctx, repo_id, entries, dt, version, CONN)
            .await?;
        assert_eq!(affected, 3);

        let results = sql
            .fetch_mapping_batch(
                &ctx,
                repo_id,
                vec![ONES_CSID, TWOS_CSID, THREES_CSID],
                dt,
                version,
                CONN,
            )
            .await?;
        assert_eq!(results.len(), 3);

        let result_map: HashMap<_, _> = results.into_iter().collect();
        assert_eq!(result_map.get(&ONES_CSID), Some(&value1));
        assert_eq!(result_map.get(&TWOS_CSID), Some(&value2));
        assert_eq!(result_map.get(&THREES_CSID), Some(&value3));

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_version_independence(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let sql = SqlCommitDerivedDataMapping::with_sqlite_in_memory()?;
        let repo_id = RepositoryId::new(1);
        let dt = DerivableType::Fsnodes;
        let value_v1 = vec![1u8; 32];
        let value_v2 = vec![2u8; 32];

        sql.store_mapping(&ctx, repo_id, ONES_CSID, dt, 1, &value_v1, CONN)
            .await?;
        sql.store_mapping(&ctx, repo_id, ONES_CSID, dt, 2, &value_v2, CONN)
            .await?;

        let result_v1 = sql
            .fetch_mapping(&ctx, repo_id, ONES_CSID, dt, 1, CONN)
            .await?;
        assert_eq!(result_v1, Some(value_v1));

        let result_v2 = sql
            .fetch_mapping(&ctx, repo_id, ONES_CSID, dt, 2, CONN)
            .await?;
        assert_eq!(result_v2, Some(value_v2));

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_insert_or_ignore(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let sql = SqlCommitDerivedDataMapping::with_sqlite_in_memory()?;
        let repo_id = RepositoryId::new(1);
        let dt = DerivableType::Fsnodes;
        let version = 1;
        let value = vec![1u8; 32];

        sql.store_mapping(&ctx, repo_id, ONES_CSID, dt, version, &value, CONN)
            .await?;

        // Second insert of same key is silently ignored
        let different_value = vec![9u8; 32];
        sql.store_mapping(
            &ctx,
            repo_id,
            ONES_CSID,
            dt,
            version,
            &different_value,
            CONN,
        )
        .await?;

        // Original value is preserved
        let result = sql
            .fetch_mapping(&ctx, repo_id, ONES_CSID, dt, version, CONN)
            .await?;
        assert_eq!(result, Some(value));

        Ok(())
    }

    #[mononoke::test]
    fn test_invalid_shard_id() -> Result<()> {
        let sql = SqlCommitDerivedDataMapping::with_sqlite_in_memory()?;
        // Single in-memory connection, shard id 0 is valid, 1 is not
        assert!(sql.shard_id(0).is_ok());
        assert!(sql.shard_id(1).is_err());
        Ok(())
    }
}
