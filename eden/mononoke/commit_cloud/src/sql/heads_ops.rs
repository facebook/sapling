/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use ::sql_ext::mononoke_queries;
use async_trait::async_trait;
use clientinfo::ClientRequestInfo;
use commit_cloud_types::WorkspaceHead;
use mercurial_types::HgChangesetId;
use sql::Transaction;

use crate::ctx::CommitCloudContext;
use crate::sql::common::UpdateWorkspaceNameArgs;
use crate::sql::ops::Delete;
use crate::sql::ops::Get;
use crate::sql::ops::Insert;
use crate::sql::ops::SqlCommitCloud;
use crate::sql::ops::Update;
use crate::sql::utils::changeset_as_bytes;
use crate::sql::utils::changeset_from_bytes;
use crate::sql::utils::list_as_bytes;

pub struct DeleteArgs {
    pub removed_commits: Vec<HgChangesetId>,
}

mononoke_queries! {
    read GetHeads(reponame: String, workspace: String) -> (String, Vec<u8>){
        mysql("SELECT `reponame`, `node` FROM `heads` WHERE `reponame`={reponame} AND `workspace`={workspace} ORDER BY `seq`")
        sqlite("SELECT `reponame`, `commit` FROM `heads` WHERE `reponame`={reponame} AND `workspace`={workspace} ORDER BY `seq`")
    }

    write DeleteHead(reponame: String, workspace: String, >list commits: Vec<u8>) {
        none,
        mysql("DELETE FROM `heads` WHERE `reponame`={reponame} AND `workspace`={workspace} AND `node` IN {commits}")
        sqlite("DELETE FROM `heads` WHERE `reponame`={reponame} AND `workspace`={workspace} AND `commit` IN {commits}")
    }

    write InsertHead(reponame: String, workspace: String, commit: Vec<u8>) {
        none,
        mysql("INSERT INTO `heads` (`reponame`, `workspace`, `node`) VALUES ({reponame}, {workspace}, {commit})")
        sqlite("INSERT INTO `heads` (`reponame`, `workspace`, `commit`) VALUES ({reponame}, {workspace}, {commit})")
    }

    write UpdateWorkspaceName( reponame: String, workspace: String, new_workspace: String) {
        none,
        "UPDATE heads SET workspace = {new_workspace} WHERE workspace = {workspace} and reponame = {reponame}"
    }
}

#[async_trait]
impl Get<WorkspaceHead> for SqlCommitCloud {
    async fn get(&self, reponame: String, workspace: String) -> anyhow::Result<Vec<WorkspaceHead>> {
        let rows =
            GetHeads::query(&self.connections.read_connection, &reponame, &workspace).await?;
        rows.into_iter()
            .map(|(_reponame, commit)| {
                Ok(WorkspaceHead {
                    commit: changeset_from_bytes(&commit, self.uses_mysql)?,
                })
            })
            .collect::<anyhow::Result<Vec<WorkspaceHead>>>()
    }
}

#[async_trait]
impl Insert<WorkspaceHead> for SqlCommitCloud {
    async fn insert(
        &self,
        txn: Transaction,
        cri: Option<&ClientRequestInfo>,
        reponame: String,
        workspace: String,
        data: WorkspaceHead,
    ) -> anyhow::Result<Transaction> {
        let (txn, _) = InsertHead::maybe_traced_query_with_transaction(
            txn,
            cri,
            &reponame,
            &workspace,
            &changeset_as_bytes(&data.commit, self.uses_mysql)?,
        )
        .await?;
        Ok(txn)
    }
}

#[async_trait]
impl Update<WorkspaceHead> for SqlCommitCloud {
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
impl Delete<WorkspaceHead> for SqlCommitCloud {
    type DeleteArgs = DeleteArgs;
    async fn delete(
        &self,
        txn: Transaction,
        cri: Option<&ClientRequestInfo>,
        reponame: String,
        workspace: String,
        args: Self::DeleteArgs,
    ) -> anyhow::Result<Transaction> {
        let (txn, _) = DeleteHead::maybe_traced_query_with_transaction(
            txn,
            cri,
            &reponame,
            &workspace,
            &list_as_bytes(args.removed_commits, self.uses_mysql)?,
        )
        .await?;
        Ok(txn)
    }
}
