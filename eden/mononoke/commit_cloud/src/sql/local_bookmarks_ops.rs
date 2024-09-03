/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use ::sql_ext::mononoke_queries;
use async_trait::async_trait;
use clientinfo::ClientRequestInfo;
use commit_cloud_types::LocalBookmarksMap;
use commit_cloud_types::WorkspaceLocalBookmark;
use sql::Connection;
use sql::Transaction;

use crate::ctx::CommitCloudContext;
use crate::sql::common::UpdateWorkspaceNameArgs;
use crate::sql::ops::Delete;
use crate::sql::ops::Get;
use crate::sql::ops::GetAsMap;
use crate::sql::ops::Insert;
use crate::sql::ops::SqlCommitCloud;
use crate::sql::ops::Update;
use crate::sql::utils::changeset_as_bytes;
use crate::sql::utils::changeset_from_bytes;

pub struct DeleteArgs {
    pub removed_bookmarks: Vec<String>,
}

mononoke_queries! {
    read GetLocalBookmarks(reponame: String, workspace: String) -> (String,  Vec<u8>){
        mysql("SELECT `name`, `node` FROM `bookmarks` WHERE `reponame`={reponame} AND `workspace`={workspace}")
        sqlite("SELECT `name`, `commit` FROM `workspacebookmarks` WHERE `reponame`={reponame} AND `workspace`={workspace}")
    }
    write DeleteLocalBookmark(reponame: String, workspace: String, >list removed_bookmarks: String) {
        none,
        mysql("DELETE FROM `bookmarks` WHERE `reponame`={reponame} AND `workspace`={workspace} AND `name` IN {removed_bookmarks}")
        sqlite("DELETE FROM `workspacebookmarks` WHERE `reponame`={reponame} AND `workspace`={workspace} AND `name` IN {removed_bookmarks}")
    }
    write InsertLocalBookmark(reponame: String, workspace: String, name: String, commit: Vec<u8>) {
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
        reponame: String,
        workspace: String,
    ) -> anyhow::Result<Vec<WorkspaceLocalBookmark>> {
        let rows = GetLocalBookmarks::query(
            &self.connections.read_connection,
            &reponame.clone(),
            &workspace,
        )
        .await?;
        rows.into_iter()
            .map(|(name, commit)| {
                WorkspaceLocalBookmark::new(name, changeset_from_bytes(&commit, self.uses_mysql)?)
            })
            .collect::<anyhow::Result<Vec<WorkspaceLocalBookmark>>>()
    }
}

#[async_trait]
impl GetAsMap<LocalBookmarksMap> for SqlCommitCloud {
    async fn get_as_map(
        &self,
        reponame: String,
        workspace: String,
    ) -> anyhow::Result<LocalBookmarksMap> {
        let rows =
            GetLocalBookmarks::query(&self.connections.read_connection, &reponame, &workspace)
                .await?;
        let mut map = LocalBookmarksMap::new();
        for (name, node) in rows {
            match changeset_from_bytes(&node, self.uses_mysql) {
                Ok(hgid) => {
                    if let Some(val) = map.get_mut(&hgid) {
                        val.push(name.clone());
                    } else {
                        map.insert(hgid, vec![name]);
                    }
                }
                Err(e) => return Err(e),
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
        cri: Option<&ClientRequestInfo>,
        reponame: String,
        workspace: String,
        data: WorkspaceLocalBookmark,
    ) -> anyhow::Result<Transaction> {
        let (txn, _) = InsertLocalBookmark::maybe_traced_query_with_transaction(
            txn,
            cri,
            &reponame,
            &workspace,
            data.name(),
            &changeset_as_bytes(data.commit(), self.uses_mysql)?,
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
        cri: Option<&ClientRequestInfo>,
        cc_ctx: CommitCloudContext,
        args: Self::UpdateArgs,
    ) -> anyhow::Result<(Transaction, u64)> {
        let (txn, result) = UpdateWorkspaceName::maybe_traced_query_with_transaction(
            txn,
            cri,
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
        cri: Option<&ClientRequestInfo>,
        reponame: String,
        workspace: String,
        args: Self::DeleteArgs,
    ) -> anyhow::Result<Transaction> {
        let (txn, _) = DeleteLocalBookmark::maybe_traced_query_with_transaction(
            txn,
            cri,
            &reponame,
            &workspace,
            &args.removed_bookmarks,
        )
        .await?;
        Ok(txn)
    }
}
