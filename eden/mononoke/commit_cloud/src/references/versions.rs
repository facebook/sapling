/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use commit_cloud_types::WorkspaceData;
use mononoke_types::Timestamp;

use crate::CoreContext;
use crate::Get;
use crate::SqlCommitCloud;
use crate::sql::versions_ops::get_other_repo_workspaces;
use crate::sql::versions_ops::get_version_by_prefix;

#[derive(Clone, Debug, PartialEq)]
pub struct WorkspaceVersion {
    pub workspace: String,
    pub reponame: String,
    pub version: u64,
    pub timestamp: Timestamp,
    pub archived: bool,
}

impl WorkspaceVersion {
    pub async fn fetch_from_db(
        ctx: &CoreContext,
        sql: &SqlCommitCloud,
        workspace: &str,
        reponame: &str,
    ) -> anyhow::Result<Option<Self>> {
        Get::<WorkspaceVersion>::get(sql, ctx, reponame.to_owned(), workspace.to_owned())
            .await
            .map(|versions| versions.into_iter().next())
    }

    pub async fn fetch_by_prefix(
        ctx: &CoreContext,
        sql: &SqlCommitCloud,
        prefix: &str,
        reponame: &str,
    ) -> anyhow::Result<Vec<Self>> {
        get_version_by_prefix(
            ctx,
            &sql.connections,
            reponame.to_string(),
            prefix.to_string(),
        )
        .await
    }

    pub async fn fetch_by_name(
        ctx: &CoreContext,
        sql: &SqlCommitCloud,
        name: &str,
    ) -> anyhow::Result<Vec<Self>> {
        get_other_repo_workspaces(ctx, &sql.connections, name.to_string()).await
    }
}

impl WorkspaceVersion {
    pub fn into_workspace_data(self) -> WorkspaceData {
        WorkspaceData {
            name: self.workspace,
            version: self.version,
            timestamp: self.timestamp.timestamp_seconds(),
            archived: self.archived,
            reponame: self.reponame,
        }
    }
}
