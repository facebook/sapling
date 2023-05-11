/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use anyhow::Result;
use futures::stream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use itertools::Itertools;
use metaconfig_types::RemoteMetadataDatabaseConfig;
use metaconfig_types::ShardableRemoteDatabaseConfig;
use mononoke_types::ChangesetId;
use mononoke_types::RepositoryId;
use sql::Connection;
use sql_construct::SqlShardableConstructFromMetadataDatabaseConfig;
use sql_construct::SqlShardedConstruct;
use sql_ext::mononoke_queries;
use sql_ext::SqlShardedConnections;
use vec1::Vec1;

mononoke_queries! {
    write InsertBlobKeysForChangesets(values: (repo_id: RepositoryId, cs_id: ChangesetId, blob_key: &str)) {
    insert_or_ignore,
    "{insert_or_ignore} INTO bonsai_blob_mapping (repo_id, cs_id, blob_key) VALUES {values}"
    }

    read GetBlobKeysForChangesets(repo_id: RepositoryId, >list cs_ids: ChangesetId) -> (ChangesetId, String) {
        "SELECT cs_id, blob_key FROM bonsai_blob_mapping WHERE repo_id = {repo_id} AND cs_id in {cs_ids}"
    }

    read GetChangesetsForBlobKeys(repo_id: RepositoryId, >list blob_keys: &str) -> (ChangesetId, String) {
        "SELECT cs_id, blob_key FROM bonsai_blob_mapping WHERE repo_id = {repo_id} AND blob_key in {blob_keys}"
    }
}

#[facet::facet]
pub struct BonsaiBlobMapping {
    pub sql_bonsai_blob_mapping: Option<SqlBonsaiBlobMapping>,
}

pub struct SqlBonsaiBlobMapping {
    write_connections: Arc<Vec1<Connection>>,
    read_connections: Arc<Vec1<Connection>>,
}

impl SqlBonsaiBlobMapping {
    pub fn new(
        write_connections: Arc<Vec1<Connection>>,
        read_connections: Arc<Vec1<Connection>>,
    ) -> Self {
        Self {
            write_connections,
            read_connections,
        }
    }

    pub async fn get_blob_keys_for_changesets(
        &self,
        repo_id: RepositoryId,
        cs_ids: Vec<ChangesetId>,
    ) -> Result<Vec<(ChangesetId, String)>> {
        Ok(stream::iter(self.read_connections.iter())
            .map(|connection| async {
                GetBlobKeysForChangesets::query(connection, &repo_id, &cs_ids[..]).await
            })
            .buffer_unordered(100)
            .try_collect::<Vec<_>>()
            .await?
            .into_iter()
            .flatten()
            .collect())
    }

    pub async fn get_changesets_for_blob_keys(
        &self,
        repo_id: RepositoryId,
        blob_keys: Vec<&str>,
    ) -> Result<Vec<(ChangesetId, String)>> {
        let shard_to_blobs = blob_keys
            .into_iter()
            .map(|key| (self.shard(repo_id, key), key))
            .into_group_map()
            .into_iter()
            .collect::<Vec<_>>();
        Ok(stream::iter(shard_to_blobs)
            .map(|(shard_id, blob_keys)| async move {
                GetChangesetsForBlobKeys::query(
                    &self.read_connections[shard_id],
                    &repo_id,
                    &blob_keys[..],
                )
                .await
            })
            .buffer_unordered(100)
            .try_collect::<Vec<_>>()
            .await?
            .into_iter()
            .flatten()
            .collect())
    }

    pub async fn insert_blob_keys_for_changesets(
        &self,
        repo_id: &RepositoryId,
        mappings: Vec<(ChangesetId, Vec<&str>)>,
    ) -> Result<u64> {
        let shard_to_values = mappings
            .iter()
            .flat_map(|(cs_id, keys)| keys.iter().map(move |key| (repo_id, cs_id, key)))
            .map(|(repo_id, cs_id, blob_key)| {
                (self.shard(*repo_id, blob_key), (repo_id, cs_id, blob_key))
            })
            .into_group_map();
        if shard_to_values.is_empty() {
            return Ok(0);
        }

        stream::iter(shard_to_values)
            .map(|(shard_id, values)| async move {
                InsertBlobKeysForChangesets::query(&self.write_connections[shard_id], &values[..])
                    .await
                    .map(|result| result.affected_rows())
            })
            .buffer_unordered(100)
            .try_fold(
                0,
                |acc, affected_rows| async move { Ok(acc + affected_rows) },
            )
            .await
    }

    fn shard(&self, _repo_id: RepositoryId, _blob_key: &str) -> usize {
        // TODO(Egor): Shard based on repo_id + blob_key
        0
    }
}

impl SqlShardedConstruct for SqlBonsaiBlobMapping {
    const LABEL: &'static str = "bonsai_blob_mapping";

    const CREATION_QUERY: &'static str = include_str!("../schemas/sqlite-bonsai-blob-mapping.sql");

    fn from_sql_shard_connections(connections: SqlShardedConnections) -> Self {
        let SqlShardedConnections {
            read_connections,
            read_master_connections: _,
            write_connections,
        } = connections;

        let write_connections = Arc::new(write_connections);
        let read_connections = Arc::new(read_connections);

        Self {
            write_connections,
            read_connections,
        }
    }
}

impl SqlShardableConstructFromMetadataDatabaseConfig for SqlBonsaiBlobMapping {
    fn remote_database_config(
        remote: &RemoteMetadataDatabaseConfig,
    ) -> Option<&ShardableRemoteDatabaseConfig> {
        remote.bonsai_blob_mapping.as_ref()
    }
}

#[cfg(test)]
mod test {
    use mononoke_types_mocks::changesetid::ONES_CSID;
    use mononoke_types_mocks::changesetid::THREES_CSID;
    use mononoke_types_mocks::changesetid::TWOS_CSID;
    use sql_construct::SqlConstruct;

    use super::*;

    #[tokio::test]
    async fn test_single_write_and_read() -> Result<()> {
        let sql = SqlBonsaiBlobMapping::with_sqlite_in_memory()?;
        let repo_id = RepositoryId::new(1);
        let blobs = vec!["blob1", "blob2", "blob3"];
        let res = sql
            .insert_blob_keys_for_changesets(&repo_id, vec![(ONES_CSID, blobs.clone())])
            .await?;

        assert_eq!(res, 3); // we are inserting 3 blobs each mapped to ONES_CSID

        let rows = sql
            .get_changesets_for_blob_keys(repo_id, vec!["blob1"])
            .await?;
        assert_eq!(rows, vec![(ONES_CSID, blobs[0].to_string())]);

        let rows = sql
            .get_blob_keys_for_changesets(repo_id, vec![ONES_CSID])
            .await?;

        let expected: Vec<_> = blobs
            .iter()
            .map(|blob| (ONES_CSID, blob.to_string()))
            .collect();
        assert_eq!(rows, expected);
        Ok(())
    }

    #[tokio::test]
    async fn test_read_write_multiple_values() -> Result<()> {
        let sql = SqlBonsaiBlobMapping::with_sqlite_in_memory()?;
        let repo_id = RepositoryId::new(1);
        let blobs1 = vec!["blob1", "blob2", "blob3"];
        let blobs2 = vec!["blob2", "blob4", "blob5"]; // overlap by blob2 with blobs1
        let blobs3 = vec!["blob3", "blob4", "blob6"]; // overlap by blob3 and blob4 with blobs1 and blobs2
        let values = vec![
            (ONES_CSID, blobs1.clone()),
            (TWOS_CSID, blobs2.clone()),
            (THREES_CSID, blobs3.clone()),
        ];

        let res = sql
            .insert_blob_keys_for_changesets(&repo_id, values)
            .await?;

        assert_eq!(res, 9); // for each of the 3 cs_ids we have 3 blobs to insert so total = 9

        let rows = sql.get_changesets_for_blob_keys(repo_id, blobs2).await?;

        assert_eq!(
            rows,
            vec![
                (ONES_CSID, "blob2".to_string()),
                (TWOS_CSID, "blob2".to_string()),
                (TWOS_CSID, "blob4".to_string()),
                (TWOS_CSID, "blob5".to_string()),
                (THREES_CSID, "blob4".to_string()),
            ]
        );

        let rows = sql
            .get_blob_keys_for_changesets(repo_id, vec![TWOS_CSID, THREES_CSID])
            .await?;

        assert_eq!(
            rows,
            vec![
                (TWOS_CSID, "blob2".to_string()),
                (TWOS_CSID, "blob4".to_string()),
                (TWOS_CSID, "blob5".to_string()),
                (THREES_CSID, "blob3".to_string()),
                (THREES_CSID, "blob4".to_string()),
                (THREES_CSID, "blob6".to_string()),
            ]
        );

        Ok(())
    }
}
