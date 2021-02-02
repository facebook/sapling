/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use futures::compat::Future01CompatExt;
use mononoke_types::{RepositoryId, Timestamp};
use sql::queries;
use sql_construct::SqlConstruct;
use sql_ext::SqlConnections;
use std::{fmt, sync::Arc};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Checkpoint {
    pub lower_bound: u64,
    pub upper_bound: u64,
    pub create_timestamp: Timestamp,
    pub update_timestamp: Timestamp,
}

#[derive(Clone)]
pub struct CheckpointsByName {
    checkpoint_name: String,
    sql_checkpoints: Arc<SqlCheckpoints>,
}

impl CheckpointsByName {
    pub fn new(checkpoint_name: String, sql_checkpoints: SqlCheckpoints) -> Self {
        Self {
            checkpoint_name,
            sql_checkpoints: Arc::new(sql_checkpoints),
        }
    }

    pub async fn load(&self, repo_id: RepositoryId) -> Result<Option<Checkpoint>, Error> {
        self.sql_checkpoints
            .load(repo_id, &self.checkpoint_name)
            .await
    }

    pub async fn insert(
        &self,
        repo_id: RepositoryId,
        checkpoint: &Checkpoint,
    ) -> Result<(), Error> {
        self.sql_checkpoints
            .insert(repo_id, &self.checkpoint_name, checkpoint)
            .await
    }

    pub async fn update(
        &self,
        repo_id: RepositoryId,
        checkpoint: &Checkpoint,
    ) -> Result<(), Error> {
        self.sql_checkpoints
            .update(repo_id, &self.checkpoint_name, checkpoint)
            .await
    }
}

impl fmt::Debug for CheckpointsByName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CheckpointsByName")
            .field("checkpoint_name", &self.checkpoint_name)
            .finish()
    }
}

pub struct SqlCheckpoints {
    connections: SqlConnections,
}

impl SqlConstruct for SqlCheckpoints {
    const LABEL: &'static str = "walker_checkpoints";

    const CREATION_QUERY: &'static str = include_str!("../schemas/sqlite-walker_checkpoints.sql");

    fn from_sql_connections(connections: SqlConnections) -> Self {
        Self { connections }
    }
}

impl SqlCheckpoints {
    pub async fn load(
        &self,
        repo_id: RepositoryId,
        checkpoint_name: &str,
    ) -> Result<Option<Checkpoint>, Error> {
        let rows = SelectCheckpoint::query(
            &self.connections.read_master_connection,
            &repo_id,
            &checkpoint_name,
        )
        .compat()
        .await?;

        Ok(rows.into_iter().next().map(|row| Checkpoint {
            lower_bound: row.0,
            upper_bound: row.1,
            create_timestamp: row.2,
            update_timestamp: row.3,
        }))
    }

    pub async fn insert(
        &self,
        repo_id: RepositoryId,
        // Query macro wants &String rather than &str
        checkpoint_name: &String,
        checkpoint: &Checkpoint,
    ) -> Result<(), Error> {
        InsertCheckpoint::query(
            &self.connections.write_connection,
            &[(
                &repo_id,
                checkpoint_name,
                &checkpoint.lower_bound,
                &checkpoint.upper_bound,
                &checkpoint.create_timestamp,
                &checkpoint.update_timestamp,
            )],
        )
        .compat()
        .await?;
        Ok(())
    }

    pub async fn update(
        &self,
        repo_id: RepositoryId,
        // Query macro wants &String rather than &str
        checkpoint_name: &String,
        checkpoint: &Checkpoint,
    ) -> Result<(), Error> {
        UpdateCheckpoint::query(
            &self.connections.write_connection,
            &repo_id,
            checkpoint_name,
            &checkpoint.lower_bound,
            &checkpoint.upper_bound,
            &checkpoint.update_timestamp,
        )
        .compat()
        .await?;
        Ok(())
    }
}

queries! {
    read SelectCheckpoint(
        repo_id: RepositoryId,
        checkpoint_name: &str,
    ) -> (u64, u64, Timestamp, Timestamp) {
        "SELECT lower_bound, upper_bound, create_timestamp, update_timestamp
        FROM walker_checkpoints WHERE repo_id={repo_id} AND checkpoint_name={checkpoint_name}"
    }

    write InsertCheckpoint(
        values: (
            repo_id: RepositoryId,
            checkpoint_name: String,
            lower_bound: u64,
            upper_bound: u64,
            create_timestamp: Timestamp,
            update_timestamp: Timestamp,
        ),
    ) {
        none,
        "INSERT INTO walker_checkpoints
         (repo_id, checkpoint_name, lower_bound, upper_bound, create_timestamp, update_timestamp)
         VALUES {values}"
    }

    write UpdateCheckpoint(
        repo_id: RepositoryId,
        checkpoint_name: String,
        lower_bound: u64,
        upper_bound: u64,
        update_timestamp: Timestamp,
    ) {
        none,
        "UPDATE walker_checkpoints
        SET lower_bound={lower_bound}, upper_bound={upper_bound}, update_timestamp={update_timestamp}
        WHERE repo_id={repo_id} AND checkpoint_name={checkpoint_name}"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fbinit::FacebookInit;

    #[fbinit::test]
    async fn test_sql_roundtrip(_fb: FacebookInit) -> Result<(), Error> {
        let checkpoints = CheckpointsByName::new(
            "test_checkpoint".to_string(),
            SqlCheckpoints::with_sqlite_in_memory()?,
        );

        let repo_id = RepositoryId::new(123);
        let lower_bound = 0;
        let upper_bound = 8;

        // Brand new checkpoint
        let now = Timestamp::now();
        let initial = Checkpoint {
            lower_bound,
            upper_bound,
            create_timestamp: now,
            update_timestamp: now,
        };
        checkpoints.insert(repo_id, &initial).await?;

        // Check roundtrip
        let roundtripped = checkpoints.load(repo_id).await?;
        assert_eq!(Some(&initial), roundtripped.as_ref());

        // Update
        let updated = Checkpoint {
            upper_bound: 9,
            ..initial.clone()
        };
        checkpoints.update(repo_id, &updated).await?;

        // Check roundtrip of update
        let roundtripped2 = checkpoints.load(repo_id).await?;
        assert_ne!(Some(&initial), roundtripped2.as_ref());
        assert_eq!(Some(&updated), roundtripped2.as_ref());

        Ok(())
    }
}
