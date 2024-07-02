/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use ::sql_ext::mononoke_queries;
use async_trait::async_trait;
use clientinfo::ClientRequestInfo;
use sql::Connection;
use sql::Transaction;

use crate::references::local_bookmarks::WorkspaceLocalBookmark;
use crate::sql::ops::Delete;
use crate::sql::ops::Get;
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
                Ok(WorkspaceLocalBookmark {
                    name,
                    commit: changeset_from_bytes(&commit, self.uses_mysql)?,
                })
            })
            .collect::<anyhow::Result<Vec<WorkspaceLocalBookmark>>>()
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
            &data.name,
            &changeset_as_bytes(&data.commit, self.uses_mysql)?,
        )
        .await?;
        Ok(txn)
    }
}

#[async_trait]
impl Update<WorkspaceLocalBookmark> for SqlCommitCloud {
    type UpdateArgs = ();

    async fn update(
        &self,
        _reponame: String,
        _workspace: String,
        _args: Self::UpdateArgs,
    ) -> anyhow::Result<()> {
        //To be implemented among other Update queries
        return Err(anyhow::anyhow!("Not implemented yet"));
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
