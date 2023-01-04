/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use mononoke_types::RepositoryId;
use sql_construct::SqlConstruct;
use sql_construct::SqlConstructFromMetadataDatabaseConfig;
use sql_ext::mononoke_queries;
use sql_ext::SqlConnections;

pub struct CommitGraphBackfillerCheckpoints {
    connections: SqlConnections,
}

impl SqlConstruct for CommitGraphBackfillerCheckpoints {
    const LABEL: &'static str = "commit_graph_backfiller_checkpoints";

    const CREATION_QUERY: &'static str =
        include_str!("../../../schemas/sqlite-commit-graph-backfiller-checkpoints.sql");

    fn from_sql_connections(connections: SqlConnections) -> Self {
        Self { connections }
    }
}

mononoke_queries! {
    read SelectCheckpoint(
        repo_id: RepositoryId,
    ) -> (Option<u64>) {
        "SELECT last_finished_id FROM commit_graph_backfiller_checkpoints WHERE repo_id={repo_id}"
    }

    write UpdateCheckpoints(
        values: (
            repo_id: RepositoryId,
            last_finished_id: u64,
        ),
    ) {
        none,
        "REPLACE INTO commit_graph_backfiller_checkpoints (repo_id, last_finished_id) VALUES {values}"
    }
}

impl CommitGraphBackfillerCheckpoints {
    pub async fn load_checkpoint(&self, repo_id: RepositoryId) -> Result<Option<u64>> {
        let rows =
            SelectCheckpoint::query(&self.connections.read_master_connection, &repo_id).await?;
        Ok(rows
            .first()
            .and_then(|(last_finished_id,)| *last_finished_id))
    }

    pub async fn update_checkpoint(
        &self,
        repo_id: RepositoryId,
        last_finished_id: u64,
    ) -> Result<()> {
        UpdateCheckpoints::query(
            &self.connections.write_connection,
            &[(&repo_id, &last_finished_id)],
        )
        .await?;
        Ok(())
    }
}

impl SqlConstructFromMetadataDatabaseConfig for CommitGraphBackfillerCheckpoints {}

#[cfg(test)]
mod tests {
    use fbinit::FacebookInit;

    use super::*;

    #[fbinit::test]
    async fn test_checkpoints(_fb: FacebookInit) -> Result<()> {
        let checkpoints = CommitGraphBackfillerCheckpoints::with_sqlite_in_memory()?;

        let first_repo_id = RepositoryId::new(111);
        let second_repo_id = RepositoryId::new(222);

        assert_eq!(checkpoints.load_checkpoint(first_repo_id).await?, None);
        assert_eq!(checkpoints.load_checkpoint(second_repo_id).await?, None);

        checkpoints.update_checkpoint(first_repo_id, 100).await?;

        assert_eq!(checkpoints.load_checkpoint(first_repo_id).await?, Some(100));
        assert_eq!(checkpoints.load_checkpoint(second_repo_id).await?, None);

        Ok(())
    }
}
