/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use ::sql_ext::mononoke_queries;
use async_trait::async_trait;
use commit_cloud_types::WorkspaceHead;
use commit_cloud_types::changeset::CloudChangesetId;
use context::CoreContext;
use sql_ext::Transaction;

use crate::ctx::CommitCloudContext;
use crate::sql::common::UpdateWorkspaceNameArgs;
use crate::sql::ops::Delete;
use crate::sql::ops::Get;
use crate::sql::ops::Insert;
use crate::sql::ops::InsertMany;
use crate::sql::ops::SqlCommitCloud;
use crate::sql::ops::Update;

pub struct DeleteArgs {
    pub removed_commits: Vec<CloudChangesetId>,
}

mononoke_queries! {
    read GetHeads(reponame: String, workspace: String) -> (String, CloudChangesetId, Option<i64>){
        mysql("SELECT `reponame`, `node`, `author_date` FROM `heads` WHERE `reponame`={reponame} AND `workspace`={workspace} ORDER BY `seq`")
        sqlite("SELECT `reponame`, `commit`, `author_date` FROM `heads` WHERE `reponame`={reponame} AND `workspace`={workspace} ORDER BY `seq`")
    }

    write DeleteHead(reponame: String, workspace: String, >list commits: CloudChangesetId) {
        none,
        mysql("DELETE FROM `heads` WHERE `reponame`={reponame} AND `workspace`={workspace} AND `node` IN {commits}")
        sqlite("DELETE FROM `heads` WHERE `reponame`={reponame} AND `workspace`={workspace} AND `commit` IN {commits}")
    }

    write InsertHead(reponame: String, workspace: String, commit: CloudChangesetId, author_date: Option<i64>) {
        none,
        mysql("INSERT INTO `heads` (`reponame`, `workspace`, `node`, `author_date`) VALUES ({reponame}, {workspace}, {commit}, {author_date})")
        sqlite("INSERT INTO `heads` (`reponame`, `workspace`, `commit`, `author_date`) VALUES ({reponame}, {workspace}, {commit}, {author_date})")
    }

    write BulkInsertHeads(values: (reponame: String, workspace: String, commit: CloudChangesetId, author_date: Option<i64>)) {
        none,
        mysql("INSERT INTO `heads` (`reponame`, `workspace`, `node`, `author_date`) VALUES {values}")
        sqlite("INSERT INTO `heads` (`reponame`, `workspace`, `commit`, `author_date`) VALUES {values}")
    }

    write UpdateWorkspaceName( reponame: String, workspace: String, new_workspace: String) {
        none,
        "UPDATE heads SET workspace = {new_workspace} WHERE workspace = {workspace} and reponame = {reponame}"
    }
}

#[async_trait]
impl Get<WorkspaceHead> for SqlCommitCloud {
    async fn get(
        &self,
        ctx: &CoreContext,
        reponame: String,
        workspace: String,
    ) -> anyhow::Result<Vec<WorkspaceHead>> {
        let rows = GetHeads::query(
            &self.connections.read_connection,
            ctx.sql_query_telemetry(),
            &reponame,
            &workspace,
        )
        .await?;
        rows.into_iter()
            .map(|(_reponame, commit, author_date)| {
                Ok(WorkspaceHead {
                    commit,
                    author_date,
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
        reponame: String,
        workspace: String,
        data: WorkspaceHead,
    ) -> anyhow::Result<Transaction> {
        let (txn, _) = InsertHead::query_with_transaction(
            txn,
            &reponame,
            &workspace,
            &data.commit,
            &data.author_date,
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
impl Delete<WorkspaceHead> for SqlCommitCloud {
    type DeleteArgs = DeleteArgs;
    async fn delete(
        &self,
        txn: Transaction,
        reponame: String,
        workspace: String,
        args: Self::DeleteArgs,
    ) -> anyhow::Result<Transaction> {
        let (txn, _) = DeleteHead::query_with_transaction(
            txn,
            &reponame,
            &workspace,
            args.removed_commits.as_slice(),
        )
        .await?;
        Ok(txn)
    }
}

#[async_trait]
impl InsertMany<WorkspaceHead> for SqlCommitCloud {
    async fn insert_many(
        &self,
        txn: Transaction,
        reponame: String,
        workspace: String,
        data: Vec<WorkspaceHead>,
    ) -> anyhow::Result<Transaction> {
        if data.is_empty() {
            return Ok(txn);
        }
        let rows: Vec<(String, String, CloudChangesetId, Option<i64>)> = data
            .into_iter()
            .map(|h| (reponame.clone(), workspace.clone(), h.commit, h.author_date))
            .collect();
        let rows_ref: Vec<(&String, &String, &CloudChangesetId, &Option<i64>)> =
            rows.iter().map(|(r, w, c, a)| (r, w, c, a)).collect();
        let (txn, _) = BulkInsertHeads::query_with_transaction(txn, rows_ref.as_slice()).await?;
        Ok(txn)
    }
}
