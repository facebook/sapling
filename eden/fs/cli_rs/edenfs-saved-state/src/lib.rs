/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::ops::Add;
use std::path::Path;

use anyhow::Context;
use mysql_client::DbLocator;
use mysql_client::InstanceRequirement;
use mysql_client::MysqlCppClient;
use mysql_client::MysqlError;
use mysql_client::Query;
use mysql_client::query;
use sapling_client::commit::get_commit_timestamp;
use sapling_client::commit::is_commit_in_repo;
use serde::Deserialize;
use serde::Serialize;
use thiserror::Error;

const XDB_SAVED_STATE: &str = "xdb.devinfra_saved_state";

#[derive(Debug, Error)]
pub enum SavedStateError {
    #[error("No saved state found")]
    NoSavedState,
    #[error("Saved-state query failed: {0}")]
    Query(#[source] MysqlError),
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

struct SavedStateInfo {
    hash: String,
    synced_hash: String,
    manifold_bucket: String,
    project_metadata: String,
    cas_digest: String,
}

#[derive(Serialize, Deserialize)]
pub struct SavedState {
    pub commit_id: String,
    pub manifold_bucket: String,
    pub manifold_path: String,
    pub cas_digest: Option<String>,
    #[serde(default)]
    pub project_metadata: Option<String>,
}

// In repoless queries, we do not have access to the full repository,
// so we cannot verify whether a given commit ID actually exists in the repo.
// Therefore, we return both the saved state commit ID (the commit the saved state was generated from)
// and the synced commit ID (1-repo)
// This allows the client to make informed decisions and use appropriate hash.
#[derive(Serialize, Deserialize)]
pub struct RepolessSavedState {
    pub commit_id: String,
    pub manifold_bucket: String,
    pub manifold_path: String,
    pub synced_commit_id: Option<String>,
    pub cas_digest: Option<String>,
    #[serde(default)]
    pub project_metadata: Option<String>,
}

pub struct SavedStateClient {
    xdb_client: MysqlCppClient,
    project: String,
    db_shard_name: String,
}

impl SavedStateClient {
    pub fn new(project: &str) -> Result<Self, SavedStateError> {
        Self::new_with_db_shard(project, XDB_SAVED_STATE)
    }

    /// Create a client using a custom database shard for lookup. Useful for testing.
    pub fn new_with_db_shard(project: &str, db_shard_name: &str) -> Result<Self, SavedStateError> {
        let xdb_client = MysqlCppClient::new(fbinit::expect_init())
            .context("Failed to create saved-state database client")?;
        Ok(Self {
            xdb_client,
            project: project.to_string(),
            db_shard_name: db_shard_name.to_string(),
        })
    }

    /// Get the most recent saved state for a given commit ID, with repository checkout available.
    /// Repository checkout is used for resolving timestamp and commit presence check in the repo.
    /// If `repo_path` is `None`, treat current working directory as target repository.
    pub async fn get_most_recent_saved_state(
        &self,
        repo_path: Option<&Path>,
        commit_id: &str,
        project_metadata: Option<&str>,
    ) -> Result<SavedState, SavedStateError> {
        self.get_most_recent_saved_state_with_timestamp(
            repo_path,
            commit_id,
            None,
            project_metadata,
            true,
        )
        .await
        .map(|(saved_state, _)| saved_state)
    }

    /// Get the most recent saved state for a given commit ID, without repository checkout available.
    /// Client is assumed to provide the valid inputs.
    pub async fn get_most_recent_saved_state_without_repo_checkout(
        &self,
        commit_id: &str,
        timestamp: u64,
        project_metadata: Option<&str>,
    ) -> Result<RepolessSavedState, SavedStateError> {
        self.get_most_recent_saved_state_with_timestamp(
            None,
            commit_id,
            Some(timestamp),
            project_metadata,
            false,
        )
        .await
        .map(|(saved_state, sync_commit)| RepolessSavedState {
            commit_id: saved_state.commit_id,
            manifold_bucket: saved_state.manifold_bucket,
            manifold_path: saved_state.manifold_path,
            synced_commit_id: Some(sync_commit).filter(|s| !s.is_empty()),
            cas_digest: saved_state.cas_digest,
            project_metadata: saved_state.project_metadata,
        })
    }

    /// Internal helper method to get the most recent saved state for a given commit ID.
    async fn get_most_recent_saved_state_with_timestamp(
        &self,
        repo_path: Option<&Path>,
        commit_id: &str,
        timestamp: Option<u64>,
        project_metadata: Option<&str>,
        repo_check: bool,
    ) -> Result<(SavedState, String), SavedStateError> {
        let timestamp = match timestamp {
            Some(timestamp) => timestamp,
            None => get_commit_timestamp(commit_id, repo_path)
                .await
                .context("Failed to resolve commit timestamp")?,
        };

        let saved_state_info = self
            .get_saved_state_info(timestamp, commit_id, project_metadata)
            .await?;

        let commit_id = if repo_check {
            let hash = &saved_state_info.hash;
            let sync_hash = &saved_state_info.synced_hash;
            if hash.is_empty() {
                return Err(anyhow::anyhow!("No saved state commit id found").into());
            }
            if is_commit_in_repo(hash, repo_path)
                .await
                .context("Failed to inspect repository")?
            {
                hash.to_string()
            } else if !sync_hash.is_empty()
                && is_commit_in_repo(sync_hash, repo_path)
                    .await
                    .context("Failed to inspect repository")?
            {
                sync_hash.to_string()
            } else {
                return Err(
                    anyhow::anyhow!("Saved state hash or sync_hash not found in repo").into(),
                );
            }
        } else {
            saved_state_info.hash.clone()
        };

        // NOTE: always use the saved state hash, even if it's not in the repo.
        let manifold_path =
            self.get_manifold_path(&saved_state_info.hash, &saved_state_info.project_metadata);
        let project_metadata = Some(saved_state_info.project_metadata).filter(|s| !s.is_empty());
        Ok((
            SavedState {
                commit_id,
                manifold_bucket: saved_state_info.manifold_bucket,
                manifold_path,
                cas_digest: Some(saved_state_info.cas_digest).filter(|s| !s.is_empty()),
                project_metadata,
            },
            saved_state_info.synced_hash,
        ))
    }

    async fn get_saved_state_info(
        &self,
        timestamp: u64,
        commit_id: &str,
        project_metadata: Option<&str>,
    ) -> Result<SavedStateInfo, SavedStateError> {
        let locator = DbLocator::new(&self.db_shard_name, InstanceRequirement::Master)
            .map_err(SavedStateError::Query)?;
        let query = self.get_query(timestamp, commit_id, project_metadata);
        let result = self
            .xdb_client
            .query(&locator, query)
            .await
            .map_err(SavedStateError::Query)?;
        let row: Vec<(
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
        )> = result
            .into_rows()
            .context("saved state query result did not match expected schema")?;
        let saved_state_info = row.into_iter().next().map(
            |(hash, synced_hash, manifold_bucket, project_metadata, cas_digest)| SavedStateInfo {
                hash: hash.unwrap_or_default(),
                synced_hash: synced_hash.unwrap_or_default(),
                manifold_bucket: manifold_bucket.unwrap_or_default(),
                project_metadata: project_metadata.unwrap_or_default(),
                cas_digest: cas_digest.unwrap_or_default(),
            },
        );
        // Throw an error if both hash and synced_hash are empty
        if let Some(info) = &saved_state_info {
            if info.hash.is_empty() && info.synced_hash.is_empty() {
                return Err(anyhow::anyhow!("Both hash and synced_hash are empty").into());
            }
        }
        saved_state_info.ok_or(SavedStateError::NoSavedState)
    }

    fn get_query(&self, timestamp: u64, commit_id: &str, project_metadata: Option<&str>) -> Query {
        let mut query = query!(
            r"SELECT `hash`, `synced_hash`, `manifold_bucket`, `project_metadata`, `cas_digest`
             FROM `saved_states`
             WHERE `project` = {project} AND
                 (`timestamp` < {timestamp} OR
                     (`timestamp` = {timestamp} AND `hash` = {commit_id}) OR
                     (`timestamp` = {timestamp} AND `synced_hash` = {commit_id}))",
            project = &self.project,
            timestamp = timestamp,
            commit_id = commit_id,
        );

        if let Some(project_metadata) = project_metadata {
            query = query.add(query!(
                "AND `project_metadata` = {project_metadata}",
                project_metadata = project_metadata
            ));
        }

        query.add(query!("ORDER BY `timestamp` DESC LIMIT 1"))
    }

    pub fn get_manifold_path(&self, commit_id: &str, project_metadata: &str) -> String {
        let filename = if !project_metadata.is_empty() {
            format!("{commit_id}_{project_metadata}")
        } else {
            commit_id.to_string()
        };

        format!("tree/{}/{}", self.project, filename)
    }
}

#[cfg(test)]
mod tests {
    use std::time::*;

    use ephemeral_shards::EphemeralShardGuard;
    use fbinit::FacebookInit;

    use crate::*;

    const PROJECT_NAME: &str = "analyze_resources";
    const FBSOURCE_COMMIT_ID: &str = "5496dd87e5fe7430a1a399530cc339a479097524";
    const MANIFOLD_BUCKET: &str = "devinfra_saved_state";

    #[fbinit::test]
    pub async fn test_get_saved_state_info() -> anyhow::Result<()> {
        // Using current time should ensure we always get a saved state, even though our
        // commit ID is arbitrary and not likely to match any saved state.
        let timestamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
        let saved_state = SavedStateClient::new(PROJECT_NAME)?;
        let saved_state_info = saved_state
            .get_saved_state_info(timestamp, FBSOURCE_COMMIT_ID, None)
            .await?;
        assert!(!(saved_state_info.hash.is_empty() && saved_state_info.synced_hash.is_empty()));
        assert_eq!(saved_state_info.manifold_bucket, MANIFOLD_BUCKET);
        Ok(())
    }

    async fn create_test_shard_and_client(
        fb: FacebookInit,
    ) -> anyhow::Result<(EphemeralShardGuard, SavedStateClient)> {
        let test_shard = EphemeralShardGuard::acquire_local_mysqld(
            fb,
            "scm_client_infra",
            // This ensures our test database has the same schema as xdb.devinfra_saved_state
            Some(XDB_SAVED_STATE.to_string()),
        )
        .await?;
        let client = SavedStateClient::new_with_db_shard("proj_main", test_shard.name())?;
        let locator = DbLocator::new(&client.db_shard_name, InstanceRequirement::Master)?;
        let rows = [
            // Using non-hex strings rev_* for hashes to make things more readable
            ("proj_main", "rev_100", "", 100, "meta_wanted"),
            ("proj_main", "rev_200", "", 200, "meta_wanted"),
            ("proj_main", "rev_300", "rev_sync", 300, "meta_other"),
            ("proj_other", "rev_400", "", 400, "meta_wanted"),
        ];
        for (project, hash, synced_hash, timestamp, project_metadata) in rows {
            client
                .xdb_client
                .query(
                    &locator,
                    query!(
                        r"INSERT INTO `saved_states`
                            (`project`, `hash`, `synced_hash`, `manifold_bucket`, `timestamp`,
                             `expires`, `project_metadata`)
                           VALUES
                            ({project}, {hash}, {synced_hash}, {manifold_bucket}, {timestamp},
                             {expires}, {project_metadata})",
                        project = project,
                        hash = hash,
                        synced_hash = synced_hash,
                        manifold_bucket = MANIFOLD_BUCKET,
                        timestamp = timestamp,
                        expires = timestamp + 3600,
                        project_metadata = project_metadata,
                    ),
                )
                .await?;
        }
        Ok((test_shard, client))
    }

    #[fbinit::test]
    async fn test_repoless_lookup_filters_optional_project_metadata(
        fb: FacebookInit,
    ) -> anyhow::Result<()> {
        let (_test_shard, client) = create_test_shard_and_client(fb).await?;

        // Filtering to project metadata "meta_wanted"
        let filtered = client
            .get_most_recent_saved_state_without_repo_checkout("rev_tip", 500, Some("meta_wanted"))
            .await?;
        assert_eq!(filtered.commit_id, "rev_200");
        assert_eq!(filtered.project_metadata.as_deref(), Some("meta_wanted"));

        // With no project metadata filter, we should get the newer row with timestamp 300
        let unfiltered = client
            .get_most_recent_saved_state_without_repo_checkout("rev_tip", 500, None)
            .await?;
        assert_eq!(unfiltered.commit_id, "rev_300");
        assert_eq!(unfiltered.project_metadata.as_deref(), Some("meta_other"));
        Ok(())
    }

    #[fbinit::test]
    async fn test_repoless_lookup_timestamp_boundary(fb: FacebookInit) -> anyhow::Result<()> {
        let (_test_shard, client) = create_test_shard_and_client(fb).await?;

        // On timestamp equality, need matching hash or synced hash
        let exact_hash = client
            .get_most_recent_saved_state_without_repo_checkout("rev_200", 200, None)
            .await?;
        assert_eq!(exact_hash.commit_id, "rev_200");

        let exact_synced_hash = client
            .get_most_recent_saved_state_without_repo_checkout("rev_sync", 300, None)
            .await?;
        assert_eq!(exact_synced_hash.commit_id, "rev_300");
        assert_eq!(
            exact_synced_hash.synced_commit_id.as_deref(),
            Some("rev_sync")
        );

        // Timestamp is equal, but hash is not: Returning older saved state
        let no_boundary_match = client
            .get_most_recent_saved_state_without_repo_checkout("rev_unrelated", 200, None)
            .await?;
        assert_eq!(no_boundary_match.commit_id, "rev_100");

        assert!(
            matches!(
                client
                    .get_most_recent_saved_state_without_repo_checkout("rev_unrelated", 50, None,)
                    .await,
                Err(SavedStateError::NoSavedState)
            ),
            "Expected a typed no-saved-state error"
        );
        Ok(())
    }
}
