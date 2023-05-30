/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::string::ToString;

use anyhow::Result;
use futures::stream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use metaconfig_types::RemoteDatabaseConfig;
use metaconfig_types::RemoteMetadataDatabaseConfig;
use mononoke_types::ChangesetId;
use mononoke_types::RepositoryId;
use mononoke_types::Timestamp;
use sql::Connection;
use sql_construct::SqlConstruct;
use sql_construct::SqlConstructFromMetadataDatabaseConfig;
use sql_ext::mononoke_queries;
use sql_ext::SqlConnections;
use strum_macros::Display;

pub const MYSQL_INSERT_CHUNK_SIZE: usize = 1000;

mononoke_queries! {
    write InsertCandidate(values: (repo_id: RepositoryId, cs_id: ChangesetId, blob_key: String, reason: String, stage: String, timestamp: Timestamp)) {
    insert_or_ignore,
    "{insert_or_ignore} INTO deletion_log (repo_id, cs_id, blob_key, reason, stage, timestamp) VALUES {values}"
    }

    read GetBlobKeysForRequest(repo_id: RepositoryId, reason: String, stage: String) -> (ChangesetId, String) {
        "SELECT cs_id, blob_key FROM deletion_log WHERE repo_id = {repo_id} AND reason = {reason} AND stage = {stage}"
    }
}

#[derive(Display, Debug)]
pub enum DeletionStage {
    Planned,
    Staged,
    Deleted,
    Cancelled,
}

#[facet::facet]
pub struct DeletionLog {
    pub sql_deletion_log: SqlDeletionLog,
}

impl DeletionLog {
    pub async fn insert_candidates(
        &self,
        repo_id: RepositoryId,
        reason: String,
        stage: DeletionStage,
        cs_to_blobs: Vec<(ChangesetId, String)>,
    ) -> Result<u64> {
        self.sql_deletion_log
            .insert_candidates(repo_id, reason, stage.to_string(), cs_to_blobs)
            .await
    }

    pub async fn get_blob_keys_for_request(
        &self,
        repo_id: RepositoryId,
        reason: String,
        stage: DeletionStage,
    ) -> Result<Vec<(ChangesetId, String)>> {
        self.sql_deletion_log
            .get_blob_keys_for_request(repo_id, reason, stage.to_string())
            .await
    }
}

pub struct SqlDeletionLog {
    write_connection: Connection,
    read_connection: Connection,
}

impl SqlConstruct for SqlDeletionLog {
    const LABEL: &'static str = "deletion_log";

    const CREATION_QUERY: &'static str = include_str!("../schemas/sqlite-deletion-log.sql");

    fn from_sql_connections(connections: SqlConnections) -> Self {
        Self {
            write_connection: connections.write_connection,
            read_connection: connections.read_connection,
        }
    }
}

impl SqlConstructFromMetadataDatabaseConfig for SqlDeletionLog {
    fn remote_database_config(
        remote: &RemoteMetadataDatabaseConfig,
    ) -> Option<&RemoteDatabaseConfig> {
        remote.deletion_log.as_ref()
    }
}

impl SqlDeletionLog {
    async fn insert_candidates(
        &self,
        repo_id: RepositoryId,
        reason: String,
        stage: String,
        cs_to_blobs: Vec<(ChangesetId, String)>,
    ) -> Result<u64> {
        if cs_to_blobs.is_empty() {
            return Ok(0);
        }
        stream::iter(cs_to_blobs.chunks(MYSQL_INSERT_CHUNK_SIZE))
            .then(|values| {
                let reason = reason.clone();
                let stage = stage.clone();
                let timestamp = Timestamp::now();
                async move {
                    let v = values
                        .iter()
                        .map(|(cs_id, blob_key)| {
                            (&repo_id, cs_id, blob_key, &reason, &stage, &timestamp)
                        })
                        .collect::<Vec<_>>();
                    InsertCandidate::query(&self.write_connection, v.as_slice()).await
                }
            })
            .try_fold(0, |acc, res| async move { Ok(acc + res.affected_rows()) })
            .await
    }

    async fn get_blob_keys_for_request(
        &self,
        repo_id: RepositoryId,
        reason: String,
        stage: String,
    ) -> Result<Vec<(ChangesetId, String)>> {
        GetBlobKeysForRequest::query(&self.read_connection, &repo_id, &reason, &stage).await
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
    async fn test_read_write() -> Result<()> {
        let sql = SqlDeletionLog::with_sqlite_in_memory()?;
        let repo_id = RepositoryId::new(1);
        let reason = "my_reason".to_string();
        let one = vec![
            (ONES_CSID, "blob1".to_string()),
            (ONES_CSID, "blob2".to_string()),
            (ONES_CSID, "blob3".to_string()),
        ];
        let two = vec![
            (TWOS_CSID, "blob4".to_string()),
            (TWOS_CSID, "blob5".to_string()),
        ];
        let three = vec![(THREES_CSID, "blob6".to_string())];
        let res = sql
            .insert_candidates(
                repo_id,
                reason.clone(),
                DeletionStage::Deleted.to_string(),
                one.clone(),
            )
            .await?;
        assert_eq!(res, 3);
        let res = sql
            .insert_candidates(
                repo_id,
                reason.clone(),
                DeletionStage::Staged.to_string(),
                two,
            )
            .await?;
        assert_eq!(res, 2);
        let res = sql
            .insert_candidates(
                repo_id,
                reason.clone(),
                DeletionStage::Staged.to_string(),
                three,
            )
            .await?;
        assert_eq!(res, 1);

        let res = sql
            .get_blob_keys_for_request(repo_id, reason, DeletionStage::Deleted.to_string())
            .await?;
        assert_eq!(res, one);
        Ok(())
    }
}
