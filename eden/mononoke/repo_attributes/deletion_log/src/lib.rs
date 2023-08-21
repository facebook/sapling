/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashSet;
use std::str::FromStr;
use std::string::ToString;

use anyhow::anyhow;
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
use strum::Display;
use strum::EnumString;

pub const MYSQL_INSERT_CHUNK_SIZE: usize = 1000;

mononoke_queries! {
    write InsertCandidate(values: (repo_id: RepositoryId, cs_id: ChangesetId, blob_key: String, reason: String, stage: String, timestamp: Timestamp)) {
    insert_or_ignore,
    "{insert_or_ignore} INTO deletion_plan (repo_id, cs_id, blob_key, reason, stage, timestamp) VALUES {values}"
    }

    write UpdateCandidate(values: (repo_id: RepositoryId, cs_id: ChangesetId, blob_key: String, reason: String, stage: String, timestamp: Timestamp)) {
        none,
        mysql("INSERT INTO deletion_plan
            (repo_id, cs_id, blob_key, reason, stage, timestamp)
        VALUES {values}
        ON DUPLICATE KEY UPDATE
            stage = VALUES(stage),
            timestamp = VALUES(timestamp)")
        sqlite("INSERT INTO deletion_plan
            (repo_id, cs_id, blob_key, reason, stage, timestamp)
        VALUES {values}
        ON CONFLICT(repo_id, cs_id, blob_key, reason) DO UPDATE SET
            stage = excluded.stage,
            timestamp = excluded.timestamp")
    }

    read GetBlobKeysForRequest(repo_id: RepositoryId, reason: String) -> (ChangesetId, String, String) {
        "SELECT cs_id, blob_key, stage FROM deletion_plan WHERE repo_id = {repo_id} AND reason = {reason}"
    }

    read GetBlobKeysForRequestAndStage(repo_id: RepositoryId, reason: String, stage: String) -> (ChangesetId, String) {
        "SELECT cs_id, blob_key FROM deletion_plan WHERE repo_id = {repo_id} AND reason = {reason} AND stage = {stage}"
    }
}

#[derive(Clone, Display, Debug, EnumString, PartialEq, Eq, Hash)]
#[strum(serialize_all = "snake_case")]
pub enum DeletionStage {
    Planned,
    Staged,
    Deleted,
    Cancelled,
}

impl DeletionStage {
    pub fn allowed_transition_from(self, from_stage: Self) -> bool {
        match (from_stage, self) {
            (Self::Planned, Self::Staged) => true,
            (Self::Staged, Self::Deleted) => true,
            (Self::Staged, Self::Cancelled) => true,
            (_, _) => false,
        }
    }
}

pub trait TransitionState {
    fn from() -> DeletionStage;
    fn to() -> DeletionStage;
}

pub struct PlannedToStaged {}

impl TransitionState for PlannedToStaged {
    fn from() -> DeletionStage {
        DeletionStage::Planned
    }
    fn to() -> DeletionStage {
        DeletionStage::Staged
    }
}
pub struct StagedToDeleted {}

impl TransitionState for StagedToDeleted {
    fn from() -> DeletionStage {
        DeletionStage::Staged
    }
    fn to() -> DeletionStage {
        DeletionStage::Deleted
    }
}
pub struct StagedToCancelled {}

impl TransitionState for StagedToCancelled {
    fn from() -> DeletionStage {
        DeletionStage::Staged
    }
    fn to() -> DeletionStage {
        DeletionStage::Cancelled
    }
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

    pub async fn update_candidates<T: TransitionState>(
        &self,
        repo_id: RepositoryId,
        reason: String,
        cs_to_blobs: Vec<(ChangesetId, String)>,
    ) -> Result<u64> {
        self.sql_deletion_log
            .update_candidates::<T>(repo_id, reason, cs_to_blobs)
            .await
    }

    pub async fn get_blob_keys_for_request(
        &self,
        repo_id: RepositoryId,
        reason: String,
        stage: DeletionStage,
    ) -> Result<Vec<(ChangesetId, String)>> {
        self.sql_deletion_log
            .get_blob_keys_for_request_and_stage(repo_id, reason, stage.to_string())
            .await
    }
}

pub struct SqlDeletionLog {
    write_connection: Connection,
    read_connection: Connection,
}

impl SqlConstruct for SqlDeletionLog {
    const LABEL: &'static str = "deletion_plan";

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

    async fn update_candidates<T: TransitionState>(
        &self,
        repo_id: RepositoryId,
        reason: String,
        cs_to_blobs: Vec<(ChangesetId, String)>,
    ) -> Result<u64> {
        let blobs_count = cs_to_blobs.len();
        if blobs_count == 0 {
            return Ok(0);
        }
        let from_stage = T::from();
        let to_stage = T::to();
        let rows = self
            .get_blob_keys_for_request(repo_id, reason.clone())
            .await?;
        let (in_target_state, not_in_target_state): (Vec<_>, Vec<_>) = rows
            .into_iter()
            .filter(|(cs_id, blob, _)| cs_to_blobs.contains(&(*cs_id, blob.clone())))
            .partition(|(_, _, stage)| *stage == to_stage);
        if blobs_count == in_target_state.len() {
            return Ok(0);
        }
        let (from_state, other_state): (HashSet<_>, HashSet<_>) = not_in_target_state
            .into_iter()
            .partition(|(_, _, stage)| *stage == from_stage);
        if !other_state.is_empty() {
            return Err(anyhow!(
                "While transition from {} to {} found blobs in the incorrect state {:?}",
                from_stage,
                to_stage,
                other_state
            ));
        }

        // collect the blobs which are requested and has not been processed yet
        let blobs_to_update = cs_to_blobs
            .into_iter()
            .map(|(cs_id, blob)| (cs_id, blob, from_stage.clone()))
            .collect::<HashSet<_>>();
        let remaining: Vec<_> = blobs_to_update.intersection(&from_state).collect();

        stream::iter(remaining.chunks(MYSQL_INSERT_CHUNK_SIZE))
            .then(|values| {
                let reason = reason.clone();
                let stage = to_stage.to_string();
                let timestamp = Timestamp::now();
                async move {
                    let v = values
                        .iter()
                        .map(|(cs_id, blob_key, _)| {
                            (&repo_id, cs_id, blob_key, &reason, &stage, &timestamp)
                        })
                        .collect::<Vec<_>>();
                    UpdateCandidate::query(&self.write_connection, v.as_slice()).await
                }
            })
            .try_fold(0, |acc, res| async move { Ok(acc + res.affected_rows()) })
            .await
    }

    async fn get_blob_keys_for_request(
        &self,
        repo_id: RepositoryId,
        reason: String,
    ) -> Result<Vec<(ChangesetId, String, DeletionStage)>> {
        let blobs = GetBlobKeysForRequest::query(&self.read_connection, &repo_id, &reason).await?;
        blobs
            .into_iter()
            .map(|(cs_id, blob, stage)| Ok((cs_id, blob, DeletionStage::from_str(&stage)?)))
            .collect::<Result<Vec<_>>>()
    }

    async fn get_blob_keys_for_request_and_stage(
        &self,
        repo_id: RepositoryId,
        reason: String,
        stage: String,
    ) -> Result<Vec<(ChangesetId, String)>> {
        GetBlobKeysForRequestAndStage::query(&self.read_connection, &repo_id, &reason, &stage).await
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
            .get_blob_keys_for_request_and_stage(
                repo_id,
                reason,
                DeletionStage::Deleted.to_string(),
            )
            .await?;
        assert_eq!(res, one);
        Ok(())
    }

    #[tokio::test]
    async fn test_update() -> Result<()> {
        let sql = SqlDeletionLog::with_sqlite_in_memory()?;
        let deletion_log = DeletionLog {
            sql_deletion_log: sql,
        };
        let repo_id = RepositoryId::new(1);
        let reason = "my_reason".to_string();
        let one = vec![
            (ONES_CSID, "blob1".to_string()),
            (ONES_CSID, "blob2".to_string()),
            (ONES_CSID, "blob3".to_string()),
        ];

        let res = deletion_log
            .insert_candidates(repo_id, reason.clone(), DeletionStage::Planned, one.clone())
            .await?;
        assert_eq!(res, 3);

        let update = vec![(ONES_CSID, "blob1".to_string())];

        let res = deletion_log
            .update_candidates::<PlannedToStaged>(repo_id, reason.clone(), update.clone())
            .await?;
        assert_eq!(res, 1);
        let res = deletion_log
            .get_blob_keys_for_request(repo_id, reason.clone(), DeletionStage::Staged)
            .await?;
        assert_eq!(res, update);
        let res = deletion_log
            .get_blob_keys_for_request(repo_id, reason, DeletionStage::Planned)
            .await?;
        assert_eq!(res, one.into_iter().skip(1).collect::<Vec<_>>());
        Ok(())
    }
}
