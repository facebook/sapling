/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::detail::log;

use anyhow::bail;
use anyhow::Error;
use bulkops::Direction;
use mononoke_types::RepositoryId;
use mononoke_types::Timestamp;
use slog::info;
use slog::Logger;
use sql::queries;
use sql_construct::SqlConstruct;
use sql_construct::SqlConstructFromMetadataDatabaseConfig;
use sql_ext::SqlConnections;
use std::cmp::Ordering;
use std::fmt;
use std::sync::Arc;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Checkpoint {
    pub lower_bound: u64,
    pub upper_bound: u64,
    pub create_timestamp: Timestamp,
    pub update_timestamp: Timestamp,
    pub update_run_number: u64,
    pub update_chunk_number: u64,
    pub last_finish_timestamp: Option<Timestamp>,
}

impl Checkpoint {
    /// Get the bounds for a catchup stream for new Changesets, plus the main stream for continuing from this checkpoint
    pub fn stream_bounds(
        &self,
        repo_lower: u64,
        repo_upper: u64,
        direction: Direction,
    ) -> Result<(Option<(u64, u64)>, Option<(u64, u64)>), Error> {
        if direction == Direction::NewestFirst {
            // First work out the main bound from checkpoint restart
            let main_bound = match repo_lower.cmp(&self.lower_bound) {
                // Checkpoint didn't get to the end, continue from it
                Ordering::Less => Some((repo_lower, self.lower_bound)),
                Ordering::Greater => bail!(
                    "Repo lower bound reversed from {} to {}",
                    self.lower_bound,
                    repo_lower
                ),
                Ordering::Equal => None,
            };

            // Then if we need to catchup due to new Changesets
            match repo_upper.cmp(&self.upper_bound) {
                // repo has advanced. We'll do the newest part first, then continue from checkpoint
                Ordering::Greater => Ok((Some((self.upper_bound, repo_upper)), main_bound)),
                Ordering::Less => bail!(
                    "Repo upper bound reversed from {} to {}",
                    self.upper_bound,
                    repo_upper
                ),
                // repo upper still the same as checkpoint, no need to catchup for new Changesets
                Ordering::Equal => Ok((None, main_bound)),
            }
        } else {
            // First work out the main bound from checkpoint restart
            let main_bound = match repo_upper.cmp(&self.upper_bound) {
                // Checkpoint didn't get to the end, continue from it
                Ordering::Greater => Some((self.upper_bound, repo_upper)),
                Ordering::Less => bail!(
                    "Repo upper bound reversed from {} to {}",
                    self.upper_bound,
                    repo_upper
                ),
                Ordering::Equal => None,
            };

            // Then if we need to catchup due to lower bound moving (unlikely, but lets cover it)
            match repo_lower.cmp(&self.lower_bound) {
                // repo bounds have widened. We'll do the new wider part first, then continue from checkpoint
                Ordering::Less => Ok((Some((repo_lower, self.lower_bound)), main_bound)),
                Ordering::Greater => bail!(
                    "Repo lower bound reversed from {} to {}",
                    self.lower_bound,
                    repo_lower
                ),
                // repo lower still the same as checkpoint, no need to catchup for wider bounds (expect this to be normal case)
                Ordering::Equal => Ok((None, main_bound)),
            }
        }
    }
}

#[derive(Clone)]
pub struct CheckpointsByName {
    pub checkpoint_name: String,
    sql_checkpoints: Arc<SqlCheckpoints>,
    pub sample_rate: u64,
}

impl CheckpointsByName {
    pub fn new(checkpoint_name: String, sql_checkpoints: SqlCheckpoints, sample_rate: u64) -> Self {
        Self {
            checkpoint_name,
            sql_checkpoints: Arc::new(sql_checkpoints),
            sample_rate,
        }
    }

    pub async fn load(&self, repo_id: RepositoryId) -> Result<Option<Checkpoint>, Error> {
        self.sql_checkpoints
            .load(repo_id, &self.checkpoint_name)
            .await
    }

    pub async fn persist(
        &self,
        logger: &Logger,
        repo_id: RepositoryId,
        chunk_num: u64,
        checkpoint: Option<Checkpoint>,
        lower_bound: u64,
        upper_bound: u64,
    ) -> Result<Checkpoint, Error> {
        let now = Timestamp::now();
        let new_cp = if let Some(mut checkpoint) = checkpoint {
            checkpoint.lower_bound = lower_bound;
            checkpoint.upper_bound = upper_bound;
            checkpoint.update_chunk_number = chunk_num;
            checkpoint.update_timestamp = now;
            info!(logger, #log::CHUNKING, "Chunk {} updating checkpoint to ({}, {})", chunk_num, lower_bound, upper_bound);
            self.update(repo_id, &checkpoint).await?;
            checkpoint
        } else {
            let new_cp = Checkpoint {
                lower_bound,
                upper_bound,
                create_timestamp: now,
                update_timestamp: now,
                update_run_number: 1,
                update_chunk_number: chunk_num,
                last_finish_timestamp: None,
            };
            info!(logger, #log::CHUNKING, "Chunk {} inserting checkpoint ({}, {})", chunk_num, lower_bound, upper_bound);
            self.insert(repo_id, &new_cp).await?;
            new_cp
        };
        Ok(new_cp)
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

    pub async fn finish(
        &self,
        repo_id: RepositoryId,
        checkpoint: &Checkpoint,
    ) -> Result<(), Error> {
        self.sql_checkpoints
            .finish(repo_id, &self.checkpoint_name, checkpoint)
            .await
    }

    pub fn name(&self) -> &str {
        self.checkpoint_name.as_str()
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

    const CREATION_QUERY: &'static str =
        include_str!("../../schemas/sqlite-walker_checkpoints.sql");

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
        .await?;

        Ok(rows.into_iter().next().map(|row| Checkpoint {
            lower_bound: row.0,
            upper_bound: row.1,
            create_timestamp: row.2,
            update_timestamp: row.3,
            update_run_number: row.4,
            update_chunk_number: row.5,
            last_finish_timestamp: row.6,
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
                &checkpoint.update_run_number,
                &checkpoint.update_chunk_number,
            )],
        )
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
            &checkpoint.create_timestamp,
            &checkpoint.update_timestamp,
            &checkpoint.update_run_number,
            &checkpoint.update_chunk_number,
        )
        .await?;
        Ok(())
    }

    pub async fn finish(
        &self,
        repo_id: RepositoryId,
        // Query macro wants &String rather than &str
        checkpoint_name: &String,
        checkpoint: &Checkpoint,
    ) -> Result<(), Error> {
        FinishCheckpoint::query(
            &self.connections.write_connection,
            &repo_id,
            checkpoint_name,
            &checkpoint.lower_bound,
            &checkpoint.upper_bound,
            &checkpoint.update_timestamp,
            &checkpoint.update_run_number,
            &checkpoint.update_chunk_number,
        )
        .await?;
        Ok(())
    }
}

impl SqlConstructFromMetadataDatabaseConfig for SqlCheckpoints {}

queries! {
    read SelectCheckpoint(
        repo_id: RepositoryId,
        checkpoint_name: &str,
    ) -> (u64, u64, Timestamp, Timestamp, u64, u64, Option<Timestamp>) {
        "SELECT lower_bound, upper_bound, create_timestamp, update_timestamp, update_run_number, update_chunk_number, last_finish_timestamp
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
            update_run_number: u64,
            update_chunk_number: u64,
        ),
    ) {
        none,
        "INSERT INTO walker_checkpoints
         (repo_id, checkpoint_name, lower_bound, upper_bound, create_timestamp, update_timestamp, update_run_number, update_chunk_number)
         VALUES {values}"
    }

    write UpdateCheckpoint(
        repo_id: RepositoryId,
        checkpoint_name: String,
        lower_bound: u64,
        upper_bound: u64,
        create_timestamp: Timestamp,
        update_timestamp: Timestamp,
        update_run_number: u64,
        update_chunk_number: u64,
    ) {
        none,
        "UPDATE walker_checkpoints
        SET lower_bound={lower_bound}, upper_bound={upper_bound}, create_timestamp={create_timestamp}, update_timestamp={update_timestamp}, update_run_number={update_run_number}, update_chunk_number={update_chunk_number}
        WHERE repo_id={repo_id} AND checkpoint_name={checkpoint_name}"
    }

    write FinishCheckpoint(
        repo_id: RepositoryId,
        checkpoint_name: String,
        lower_bound: u64,
        upper_bound: u64,
        last_finish_timestamp: Timestamp,
        last_finish_run_number: u64,
        last_finish_chunk_number: u64,
    ) {
        none,
        "UPDATE walker_checkpoints
        SET lower_bound={lower_bound}, upper_bound={upper_bound}, last_finish_timestamp={last_finish_timestamp}, last_finish_run_number={last_finish_run_number}, last_finish_chunk_number={last_finish_chunk_number}
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
            0,
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
            update_run_number: 1,
            update_chunk_number: 1,
            last_finish_timestamp: None,
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

        // Finish
        checkpoints.finish(repo_id, &updated).await?;
        let roundtripped3 = checkpoints.load(repo_id).await?;
        assert_eq!(
            roundtripped2.map(|o| o.update_timestamp),
            roundtripped3.and_then(|o| o.last_finish_timestamp),
        );

        Ok(())
    }
}
