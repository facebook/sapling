/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use commit_cloud_types::WorkspaceData;
use mononoke_types::Timestamp;

use crate::sql::versions_ops::get_version_by_prefix;
use crate::Get;
use crate::SqlCommitCloud;

#[derive(Clone, Debug, PartialEq)]
pub struct WorkspaceVersion {
    pub workspace: String,
    pub version: u64,
    pub timestamp: Timestamp,
    pub archived: bool,
}

impl WorkspaceVersion {
    pub async fn fetch_from_db(
        sql: &SqlCommitCloud,
        workspace: &str,
        reponame: &str,
    ) -> anyhow::Result<Option<Self>> {
        Get::<WorkspaceVersion>::get(sql, reponame.to_owned(), workspace.to_owned())
            .await
            .map(|versions| versions.into_iter().next())
    }

    pub async fn fetch_by_prefix(
        sql: &SqlCommitCloud,
        prefix: &str,
        reponame: &str,
    ) -> anyhow::Result<Vec<Self>> {
        get_version_by_prefix(&sql.connections, reponame.to_string(), prefix.to_string()).await
    }
}

impl WorkspaceVersion {
    pub fn into_workspace_data(self, reponame: &str) -> WorkspaceData {
        WorkspaceData {
            name: self.workspace,
            version: self.version,
            timestamp: self.timestamp.timestamp_seconds(),
            archived: self.archived,
            reponame: reponame.to_owned(),
        }
    }
}
