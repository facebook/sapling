/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use ::sql_ext::mononoke_queries;
use async_trait::async_trait;
use commit_cloud_types::LocalBookmarksMap;
use commit_cloud_types::WorkspaceLocalBookmark;
use commit_cloud_types::changeset::CloudChangesetId;
use context::CoreContext;
use sql_ext::Transaction;

use crate::ctx::CommitCloudContext;
use crate::sql::common::UpdateWorkspaceNameArgs;
use crate::sql::ops::Delete;
use crate::sql::ops::Get;
use crate::sql::ops::GetAsMap;
use crate::sql::ops::Insert;
use crate::sql::ops::SqlCommitCloud;
use crate::sql::ops::Update;

pub struct DeleteArgs {
    pub removed_bookmarks: Vec<String>,
}

mononoke_queries! {
    read GetLocalBookmarks(reponame: String, workspace: String) -> (String,  CloudChangesetId){
        mysql("SELECT `name`, `node` FROM `bookmarks` WHERE `reponame`={reponame} AND `workspace`={workspace}")
        sqlite("SELECT `name`, `commit` FROM `workspacebookmarks` WHERE `reponame`={reponame} AND `workspace`={workspace}")
    }
    write DeleteLocalBookmark(reponame: String, workspace: String, >list removed_bookmarks: String) {
        none,
        mysql("DELETE FROM `bookmarks` WHERE `reponame`={reponame} AND `workspace`={workspace} AND `name` IN {removed_bookmarks}")
        sqlite("DELETE FROM `workspacebookmarks` WHERE `reponame`={reponame} AND `workspace`={workspace} AND `name` IN {removed_bookmarks}")
    }
    write InsertLocalBookmark(reponame: String, workspace: String, name: String, commit: CloudChangesetId) {
        none,
        mysql("INSERT INTO `bookmarks` (`reponame`, `workspace`, `name`, `node`) VALUES ({reponame}, {workspace}, {name}, {commit})")
        sqlite("INSERT INTO `workspacebookmarks` (`reponame`, `workspace`, `name`, `commit`) VALUES ({reponame}, {workspace}, {name}, {commit})")
    }
    write UpdateWorkspaceName( reponame: String, workspace: String, new_workspace: String) {
        none,
        mysql("UPDATE `bookmarks` SET workspace = {new_workspace} WHERE workspace = {workspace} and reponame = {reponame}")
        sqlite("UPDATE `workspacebookmarks` SET workspace = {new_workspace} WHERE workspace = {workspace} and reponame = {reponame}")
    }
}

#[async_trait]
impl Get<WorkspaceLocalBookmark> for SqlCommitCloud {
    async fn get(
        &self,
        ctx: &CoreContext,
        reponame: String,
        workspace: String,
    ) -> anyhow::Result<Vec<WorkspaceLocalBookmark>> {
        let rows = GetLocalBookmarks::query(
            &self.connections.read_connection,
            ctx.sql_query_telemetry(),
            &reponame.clone(),
            &workspace,
        )
        .await?;
        rows.into_iter()
            .map(|(name, commit)| WorkspaceLocalBookmark::new(name, commit))
            .collect::<anyhow::Result<Vec<WorkspaceLocalBookmark>>>()
    }
}

#[async_trait]
impl GetAsMap<LocalBookmarksMap> for SqlCommitCloud {
    async fn get_as_map(
        &self,
        ctx: &CoreContext,
        reponame: String,
        workspace: String,
    ) -> anyhow::Result<LocalBookmarksMap> {
        let rows = GetLocalBookmarks::query(
            &self.connections.read_connection,
            ctx.sql_query_telemetry(),
            &reponame,
            &workspace,
        )
        .await?;
        let mut map = LocalBookmarksMap::new();
        for (name, node) in rows {
            if let Some(val) = map.get_mut(&node) {
                val.push(name.clone());
            } else {
                map.insert(node, vec![name]);
            }
        }
        Ok(map)
    }
}

#[async_trait]
impl Insert<WorkspaceLocalBookmark> for SqlCommitCloud {
    async fn insert(
        &self,
        txn: Transaction,
        _ctx: &CoreContext,
        reponame: String,
        workspace: String,
        data: WorkspaceLocalBookmark,
    ) -> anyhow::Result<Transaction> {
        let (txn, _) = InsertLocalBookmark::query_with_transaction(
            txn,
            &reponame,
            &workspace,
            data.name(),
            data.commit(),
        )
        .await?;
        Ok(txn)
    }
}

#[async_trait]
impl Update<WorkspaceLocalBookmark> for SqlCommitCloud {
    type UpdateArgs = UpdateWorkspaceNameArgs;
    async fn update(
        &self,
        txn: Transaction,
        _ctx: &CoreContext,
        cc_ctx: CommitCloudContext,
        args: Self::UpdateArgs,
    ) -> anyhow::Result<(Transaction, u64)> {
        let (txn, result) = UpdateWorkspaceName::query_with_transaction(
            txn,
            &cc_ctx.reponame,
            &cc_ctx.workspace,
            &args.new_workspace,
        )
        .await?;
        Ok((txn, result.affected_rows()))
    }
}

#[async_trait]
impl Delete<WorkspaceLocalBookmark> for SqlCommitCloud {
    type DeleteArgs = DeleteArgs;
    async fn delete(
        &self,
        txn: Transaction,
        _ctx: &CoreContext,
        reponame: String,
        workspace: String,
        args: Self::DeleteArgs,
    ) -> anyhow::Result<Transaction> {
        let (txn, _) = DeleteLocalBookmark::query_with_transaction(
            txn,
            &reponame,
            &workspace,
            &args.removed_bookmarks,
        )
        .await?;
        Ok(txn)
    }
}
