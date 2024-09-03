/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use async_trait::async_trait;
use clientinfo::ClientRequestInfo;
use commit_cloud_types::RemoteBookmarksMap;
use commit_cloud_types::WorkspaceRemoteBookmark;
use sql::Transaction;
use sql_ext::mononoke_queries;

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
    read GetRemoteBookmarks(reponame: String, workspace: String) -> (String, String, Vec<u8>){
        mysql("SELECT `remote`, `name`, `node` FROM `remotebookmarks` WHERE `reponame`={reponame} AND `workspace`={workspace}")
        sqlite("SELECT `remote`, `name`, `commit` FROM `remotebookmarks` WHERE `reponame`={reponame} AND `workspace`={workspace}")
    }
    //TODO: Handle changesets as bytes (migth require an entirely different query)
    write DeleteRemoteBookmark(reponame: String, workspace: String,  >list removed_bookmarks: String) {
        none,
        mysql("DELETE FROM `remotebookmarks` WHERE `reponame`={reponame} AND `workspace`={workspace} AND CONCAT(`remote`, '/', `name`) IN {removed_bookmarks}")
        sqlite( "DELETE FROM `remotebookmarks` WHERE `reponame`={reponame} AND `workspace`={workspace} AND CAST(`remote`||'/'||`name` AS BLOB) IN {removed_bookmarks}")
    }
    write InsertRemoteBookmark(reponame: String, workspace: String, remote: String, name: String, commit:  Vec<u8>) {
        none,
        mysql("INSERT INTO `remotebookmarks` (`reponame`, `workspace`, `remote`,`name`, `node` ) VALUES ({reponame}, {workspace}, {remote}, {name}, {commit})")
        sqlite("INSERT INTO `remotebookmarks` (`reponame`, `workspace`, `remote`,`name`, `commit` ) VALUES ({reponame}, {workspace}, {remote}, {name}, {commit})")
    }
    write UpdateWorkspaceName( reponame: String, workspace: String, new_workspace: String) {
        none,
        "UPDATE remotebookmarks SET workspace = {new_workspace} WHERE workspace = {workspace} and reponame = {reponame}"
    }
}

#[async_trait]
impl Get<WorkspaceRemoteBookmark> for SqlCommitCloud {
    async fn get(
        &self,
        reponame: String,
        workspace: String,
    ) -> anyhow::Result<Vec<WorkspaceRemoteBookmark>> {
        let rows = GetRemoteBookmarks::query(
            &self.connections.read_connection,
            &reponame.clone(),
            &workspace,
        )
        .await?;
        rows.into_iter()
            .map(|(remote, name, commit)| {
                WorkspaceRemoteBookmark::new(
                    remote,
                    name,
                    changeset_from_bytes(&commit, self.uses_mysql)?,
                )
            })
            .collect::<anyhow::Result<Vec<WorkspaceRemoteBookmark>>>()
    }
}

#[async_trait]
impl GetAsMap<RemoteBookmarksMap> for SqlCommitCloud {
    async fn get_as_map(
        &self,
        reponame: String,
        workspace: String,
    ) -> anyhow::Result<RemoteBookmarksMap> {
        let rows = GetRemoteBookmarks::query(
            &self.connections.read_connection,
            &reponame.clone(),
            &workspace,
        )
        .await?;

        let mut map = RemoteBookmarksMap::new();
        for (remote, name, commit) in rows {
            match changeset_from_bytes(&commit, self.uses_mysql) {
                Ok(hgid) => {
                    let rb = WorkspaceRemoteBookmark::new(remote, name, hgid)?;
                    if let Some(val) = map.get_mut(&hgid) {
                        val.push(rb);
                    } else {
                        map.insert(hgid, vec![rb]);
                    }
                }
                Err(e) => return Err(e),
            }
        }
        Ok(map)
    }
}

#[async_trait]
impl Insert<WorkspaceRemoteBookmark> for SqlCommitCloud {
    async fn insert(
        &self,
        txn: Transaction,
        cri: Option<&ClientRequestInfo>,
        reponame: String,
        workspace: String,
        data: WorkspaceRemoteBookmark,
    ) -> anyhow::Result<Transaction> {
        let (txn, _) = InsertRemoteBookmark::maybe_traced_query_with_transaction(
            txn,
            cri,
            &reponame,
            &workspace,
            data.remote(),
            data.name(),
            &changeset_as_bytes(data.commit(), self.uses_mysql)?,
        )
        .await?;
        Ok(txn)
    }
}

#[async_trait]
impl Update<WorkspaceRemoteBookmark> for SqlCommitCloud {
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
impl Delete<WorkspaceRemoteBookmark> for SqlCommitCloud {
    type DeleteArgs = DeleteArgs;
    async fn delete(
        &self,
        txn: Transaction,
        cri: Option<&ClientRequestInfo>,
        reponame: String,
        workspace: String,
        args: Self::DeleteArgs,
    ) -> anyhow::Result<Transaction> {
        let (txn, _) = DeleteRemoteBookmark::maybe_traced_query_with_transaction(
            txn,
            cri,
            &reponame,
            &workspace,
            args.removed_bookmarks.as_slice(),
        )
        .await?;
        Ok(txn)
    }
}
