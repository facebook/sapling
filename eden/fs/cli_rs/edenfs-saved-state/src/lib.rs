/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::ops::Add;

use anyhow::Context;
use mysql_client::DbLocator;
use mysql_client::InstanceRequirement;
use mysql_client::MysqlCppClient;
use mysql_client::Query;
use mysql_client::query;
use sapling_client::commit::get_commit_timestamp;
use sapling_client::commit::is_commit_in_repo;
use serde::Deserialize;
use serde::Serialize;

const XDB_SAVED_STATE: &str = "xdb.devinfra_saved_state";

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
}

pub struct SavedStateClient {
    xdb_client: MysqlCppClient,
    project: String,
}

impl SavedStateClient {
    pub fn new(project: &str) -> anyhow::Result<Self> {
        let xdb_client = MysqlCppClient::new(fbinit::expect_init())?;
        Ok(Self {
            xdb_client,
            project: project.to_string(),
        })
    }

    /// Get the most recent saved state for a given commit ID, with repository checkout available.
    /// Repository checkout is used for resolving timestamp and commit presence check in the repo.
    /// The method is assumed to be called from the repository checkout.
    pub async fn get_most_recent_saved_state(&self, commit_id: &str) -> anyhow::Result<SavedState> {
        self.get_most_recent_saved_state_with_timestamp(commit_id, None, true)
            .await
            .map(|(saved_state, _)| saved_state)
    }

    /// Get the most recent saved state for a given commit ID, without repository checkout available.
    /// Client is assumed to provide the valid inputs.
    pub async fn get_most_recent_saved_state_without_repo_checkout(
        &self,
        commit_id: &str,
        timestamp: u64,
    ) -> anyhow::Result<RepolessSavedState> {
        self.get_most_recent_saved_state_with_timestamp(commit_id, Some(timestamp), false)
            .await
            .map(|(saved_state, sync_commit)| RepolessSavedState {
                commit_id: saved_state.commit_id,
                manifold_bucket: saved_state.manifold_bucket,
                manifold_path: saved_state.manifold_path,
                synced_commit_id: Some(sync_commit).filter(|s| !s.is_empty()),
                cas_digest: saved_state.cas_digest,
            })
    }

    /// Internal helper method to get the most recent saved state for a given commit ID.
    async fn get_most_recent_saved_state_with_timestamp(
        &self,
        commit_id: &str,
        timestamp: Option<u64>,
        repo_check: bool,
    ) -> anyhow::Result<(SavedState, String)> {
        let timestamp = match timestamp {
            Some(timestamp) => timestamp,
            None => get_commit_timestamp(commit_id)
                .await
                .map_err(anyhow::Error::msg)?,
        };

        let saved_state_info = self
            .get_saved_state_info(timestamp, commit_id, "")
            .await
            .map_err(anyhow::Error::msg)?;

        let commit_id = if repo_check {
            let hash = &saved_state_info.hash;
            let sync_hash = &saved_state_info.synced_hash;
            if hash.is_empty() {
                return Err(anyhow::anyhow!("No saved state commit id found"));
            }
            if is_commit_in_repo(hash).await? {
                hash.to_string()
            } else if !sync_hash.is_empty() && is_commit_in_repo(sync_hash).await? {
                sync_hash.to_string()
            } else {
                return Err(anyhow::anyhow!(
                    "Saved state hash or sync_hash not found in repo"
                ));
            }
        } else {
            saved_state_info.hash.clone()
        };

        // NOTE: always use the saved state hash, even if it's not in the repo.
        let manifold_path =
            self.get_manifold_path(&saved_state_info.hash, &saved_state_info.project_metadata);
        Ok((
            SavedState {
                commit_id,
                manifold_bucket: saved_state_info.manifold_bucket,
                manifold_path,
                cas_digest: Some(saved_state_info.cas_digest).filter(|s| !s.is_empty()),
            },
            saved_state_info.synced_hash,
        ))
    }

    async fn get_saved_state_info(
        &self,
        timestamp: u64,
        commit_id: &str,
        project_metadata: &str,
    ) -> anyhow::Result<SavedStateInfo> {
        let locator = DbLocator::new(XDB_SAVED_STATE, InstanceRequirement::Master)?;
        let query = self.get_query(timestamp, commit_id, project_metadata);
        let result = self.xdb_client.query(&locator, query).await?;
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
                return Err(anyhow::anyhow!("Both hash and synced_hash are empty"));
            }
        }
        saved_state_info.ok_or_else(|| anyhow::anyhow!("No saved state found"))
    }

    fn get_query(&self, timestamp: u64, commit_id: &str, project_metadata: &str) -> Query {
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

        if !project_metadata.is_empty() {
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

    use crate::*;

    const PROJECT_NAME: &str = "meerkat";
    const FBSOURCE_COMMIT_ID: &str = "5496dd87e5fe7430a1a399530cc339a479097524";
    const MANIFOLD_BUCKET: &str = "devinfra_saved_state";

    #[fbinit::test]
    pub async fn test_get_saved_state_info() -> anyhow::Result<()> {
        // Using current time should ensure we always get a saved state, even though our
        // commit ID is arbitrary and not likely to match any saved state.
        let timestamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
        let saved_state = SavedStateClient::new(PROJECT_NAME)?;
        let saved_state_info = saved_state
            .get_saved_state_info(timestamp, FBSOURCE_COMMIT_ID, "")
            .await?;
        assert!(!(saved_state_info.hash.is_empty() && saved_state_info.synced_hash.is_empty()));
        assert_eq!(saved_state_info.manifold_bucket, MANIFOLD_BUCKET);
        Ok(())
    }
}
