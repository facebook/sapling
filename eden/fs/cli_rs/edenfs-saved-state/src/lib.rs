/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::ops::Add;

use edenfs_client::sapling::get_commit_timestamp;
use edenfs_client::sapling::is_commit_in_repo;
use mysql_client::query;
use mysql_client::DbLocator;
use mysql_client::InstanceRequirement;
use mysql_client::MysqlCppClient;
use mysql_client::Query;

const XDB_SAVED_STATE: &str = "xdb.devinfra_saved_state";

struct SavedStateInfo {
    hash: String,
    synced_hash: String,
    manifold_bucket: String,
    project_metadata: String,
}

pub struct SavedState {
    pub commit_id: String,
    pub manifold_bucket: String,
    pub manifold_path: String,
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

    pub async fn get_most_recent_saved_state(&self, commit_id: &str) -> anyhow::Result<SavedState> {
        let timestamp = get_commit_timestamp(commit_id)
            .await
            .map_err(anyhow::Error::msg)?;
        let saved_state_info = self
            .get_saved_state_info(timestamp, commit_id, "")
            .await
            .map_err(anyhow::Error::msg)?;

        let commit_id = if saved_state_info.hash.is_empty() {
            return Err(anyhow::anyhow!("No saved state commit id found"));
        } else if is_commit_in_repo(&saved_state_info.hash).await? {
            saved_state_info.hash.clone()
        } else if !saved_state_info.synced_hash.is_empty()
            && is_commit_in_repo(&saved_state_info.synced_hash).await?
        {
            saved_state_info.synced_hash
        } else {
            return Err(anyhow::anyhow!(
                "Saved state hash or sync_hash not found in repo"
            ));
        };

        // NOTE: always use the saved state hash, even if it's not in the repo.
        let manifold_path =
            self.get_manifold_path(&saved_state_info.hash, &saved_state_info.project_metadata);
        Ok(SavedState {
            commit_id,
            manifold_bucket: saved_state_info.manifold_bucket,
            manifold_path,
        })
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
        let row: Vec<(String, String, String, String)> = result.into_rows()?;
        let saved_state_info =
            row.into_iter()
                .next()
                .map(
                    |(hash, synced_hash, manifold_bucket, project_metadata)| SavedStateInfo {
                        hash,
                        synced_hash,
                        manifold_bucket,
                        project_metadata,
                    },
                );
        saved_state_info.ok_or_else(|| anyhow::anyhow!("No saved state found"))
    }

    fn get_query(&self, timestamp: u64, commit_id: &str, project_metadata: &str) -> Query {
        let mut query = query!(
            r"SELECT `hash`, `synced_hash`, `manifold_bucket`, `project_metadata`
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
        // commit ID is arbitrary and not likley to match any saved state.
        let timestamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
        let saved_state = SavedStateClient::new(PROJECT_NAME)?;
        let saved_state_info = saved_state
            .get_saved_state_info(timestamp, FBSOURCE_COMMIT_ID, "")
            .await?;
        assert!(!saved_state_info.hash.is_empty());
        assert!(!saved_state_info.synced_hash.is_empty());
        assert_eq!(saved_state_info.manifold_bucket, MANIFOLD_BUCKET);
        Ok(())
    }
}
