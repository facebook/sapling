/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::hash::Hasher;
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
use twox_hash::XxHash32;
use vec1::Vec1;

pub const MYSQL_INSERT_CHUNK_SIZE: usize = 1000;

mononoke_queries! {
    write InsertBlobKeysForChangesets(values: (repo_id: RepositoryId, cs_id: ChangesetId, blob_key: String)) {
    insert_or_ignore,
    "{insert_or_ignore} INTO bonsai_blob_mapping (repo_id, cs_id, blob_key) VALUES {values}"
    }

    read GetBlobKeysForChangesets(repo_id: RepositoryId, >list cs_ids: ChangesetId) -> (ChangesetId, String) {
        "SELECT cs_id, blob_key FROM bonsai_blob_mapping WHERE repo_id = {repo_id} AND cs_id in {cs_ids}"
    }

    read GetChangesetsForBlobKeys(repo_id: RepositoryId, >list blob_keys: String) -> (ChangesetId, String) {
        "SELECT cs_id, blob_key FROM bonsai_blob_mapping WHERE repo_id = {repo_id} AND blob_key in {blob_keys}"
    }
}

#[facet::facet]
pub struct BonsaiBlobMapping {
    pub sql_bonsai_blob_mapping: SqlBonsaiBlobMapping,
}

impl BonsaiBlobMapping {
    pub async fn get_blob_keys_for_changesets(
        &self,
        repo_id: RepositoryId,
        cs_ids: Vec<ChangesetId>,
    ) -> Result<Vec<(ChangesetId, String)>> {
        self.sql_bonsai_blob_mapping
            .get_blob_keys_for_changesets(repo_id, cs_ids)
            .await
    }

    pub async fn get_changesets_for_blob_keys(
        &self,
        repo_id: RepositoryId,
        blob_keys: Vec<String>,
    ) -> Result<Vec<(ChangesetId, String)>> {
        self.sql_bonsai_blob_mapping
            .get_changesets_for_blob_keys(repo_id, blob_keys)
            .await
    }

    pub async fn insert_blob_keys_for_changesets(
        &self,
        repo_id: RepositoryId,
        mappings: Vec<(ChangesetId, String)>,
    ) -> Result<u64> {
        self.sql_bonsai_blob_mapping
            .insert_blob_keys_for_changesets(repo_id, mappings)
            .await
    }
}

pub struct SqlBonsaiBlobMapping {
    shard_count: usize,
    write_connections: Arc<Vec1<Connection>>,
    read_connections: Arc<Vec1<Connection>>,
}

impl SqlBonsaiBlobMapping {
    pub fn new(
        write_connections: Arc<Vec1<Connection>>,
        read_connections: Arc<Vec1<Connection>>,
    ) -> Self {
        let shard_count = read_connections.len();
        Self {
            shard_count,
            write_connections,
            read_connections,
        }
    }

    pub async fn get_blob_keys_for_changesets(
        &self,
        repo_id: RepositoryId,
        cs_ids: Vec<ChangesetId>,
    ) -> Result<Vec<(ChangesetId, String)>> {
        let mut res = vec![];
        for shard_id in 0..self.shard_count {
            let rows = GetBlobKeysForChangesets::query(
                &self.read_connections[shard_id],
                &repo_id,
                &cs_ids[..],
            )
            .await?;
            res.extend(rows);
        }
        Ok(res)
    }

    pub async fn get_changesets_for_blob_keys(
        &self,
        repo_id: RepositoryId,
        blob_keys: Vec<String>,
    ) -> Result<Vec<(ChangesetId, String)>> {
        let shard_to_blobs = blob_keys
            .into_iter()
            .map(|key| (self.shard(repo_id, &key), key))
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
        repo_id: RepositoryId,
        mappings: Vec<(ChangesetId, String)>,
    ) -> Result<u64> {
        let shard_to_values = mappings
            .into_iter()
            .map(|(cs_id, blob_key)| (self.shard(repo_id, &blob_key), (repo_id, cs_id, blob_key)))
            .into_group_map();
        if shard_to_values.is_empty() {
            return Ok(0);
        }

        stream::iter(shard_to_values)
            .map(|(shard_id, values)| async move {
                let mut affected_rows = 0;
                for chunk in values.chunks(MYSQL_INSERT_CHUNK_SIZE) {
                    // This iter().map() is needed to convert &(_,_,_) to (&_, &_, &_)
                    let chunk: Vec<_> = chunk.iter().map(|(a, b, c)| (a, b, c)).collect();
                    let result = InsertBlobKeysForChangesets::query(
                        &self.write_connections[shard_id],
                        &chunk[..],
                    )
                    .await?;
                    affected_rows += result.affected_rows();
                }
                anyhow::Ok(affected_rows)
            })
            .buffer_unordered(100)
            .try_fold(
                0,
                |acc, affected_rows| async move { Ok(acc + affected_rows) },
            )
            .await
    }

    fn shard(&self, repo_id: RepositoryId, blob_key: &str) -> usize {
        let mut hasher = XxHash32::with_seed(0);
        hasher.write(&repo_id.id().to_ne_bytes());
        hasher.write(blob_key.as_bytes());
        (hasher.finish() % self.shard_count as u64) as usize
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
        let shard_count = read_connections.len();

        Self {
            shard_count,
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
        let values = vec!["blob1", "blob2", "blob3"]
            .into_iter()
            .map(|blob| (ONES_CSID, blob.into()))
            .collect::<Vec<_>>();
        let res = sql
            .insert_blob_keys_for_changesets(repo_id, values.clone())
            .await?;

        assert_eq!(res, 3); // we are inserting 3 blobs each mapped to ONES_CSID

        let rows = sql
            .get_changesets_for_blob_keys(repo_id, vec!["blob1".into()])
            .await?;
        assert_eq!(rows, vec![values[0].clone()]);

        let rows = sql
            .get_blob_keys_for_changesets(repo_id, vec![ONES_CSID])
            .await?;

        assert_eq!(rows, values);
        Ok(())
    }

    #[tokio::test]
    async fn test_read_write_multiple_values() -> Result<()> {
        let sql = SqlBonsaiBlobMapping::with_sqlite_in_memory()?;
        let repo_id = RepositoryId::new(1);
        let blobs1 = vec!["blob1", "blob2", "blob3"]
            .into_iter()
            .map(|blob| (ONES_CSID, blob.into()))
            .collect::<Vec<_>>();
        let blobs2 = vec!["blob2", "blob4", "blob5"]
            .into_iter()
            .map(|blob| (TWOS_CSID, blob.into()))
            .collect::<Vec<_>>(); // overlap by blob2 with blobs1
        let blobs3 = vec!["blob3", "blob4", "blob6"]
            .into_iter()
            .map(|blob| (THREES_CSID, blob.into()))
            .collect::<Vec<_>>(); // overlap by blob3 and blob4 with blobs1 and blobs2
        let values = [blobs1.clone(), blobs2.clone(), blobs3.clone()]
            .concat()
            .into_iter()
            .collect::<Vec<_>>();

        let res = sql.insert_blob_keys_for_changesets(repo_id, values).await?;

        assert_eq!(res, 9); // for each of the 3 cs_ids we have 3 blobs to insert so total = 9

        let blob_keys = blobs2.into_iter().map(|(_, s)| s).collect::<Vec<_>>();
        let rows = sql.get_changesets_for_blob_keys(repo_id, blob_keys).await?;

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

    #[test]
    fn test_sharding() -> Result<()> {
        let mut sql = SqlBonsaiBlobMapping::with_sqlite_in_memory()?;
        // manually specif multiple shards
        sql.shard_count = 3;
        let repo_id = RepositoryId::new(1);
        let blobs = [
            "blob1", "blob2", "blob3", "blob4", "blob5", "blob6", "blob7",
        ];
        let shards: Vec<_> = blobs.iter().map(|blob| sql.shard(repo_id, blob)).collect();
        assert_eq!(shards, vec![1, 2, 1, 2, 0, 2, 0]);

        // verify that blob will consistently hashed to the same shard
        let shard = sql.shard(repo_id, blobs[2]);
        assert_eq!(shard, 1);

        // verify that different repo_id with the same blob will land on a different shard
        let shard = sql.shard(RepositoryId::new(3), blobs[2]);
        assert_eq!(shard, 0);

        Ok(())
    }
}
