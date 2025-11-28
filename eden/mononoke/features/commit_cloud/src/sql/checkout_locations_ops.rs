/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::PathBuf;

use ::sql_ext::mononoke_queries;
use async_trait::async_trait;
use commit_cloud_types::WorkspaceCheckoutLocation;
use commit_cloud_types::changeset::CloudChangesetId;
use context::CoreContext;
use mononoke_types::Timestamp;
use sql_ext::Transaction;

use crate::ctx::CommitCloudContext;
use crate::sql::common::UpdateWorkspaceNameArgs;
use crate::sql::ops::Get;
use crate::sql::ops::Insert;
use crate::sql::ops::SqlCommitCloud;
use crate::sql::ops::Update;

mononoke_queries! {
    pub(crate) read GetCheckoutLocations(reponame: String, workspace: String) -> (String, String, String, CloudChangesetId, Timestamp, String) {
        "SELECT
            `hostname`,
            `checkout_path`,
            `shared_path`,
            `commit` ,
            `timestamp`,
            `unixname`
        FROM `checkoutlocations`
        WHERE `reponame`={reponame} AND `workspace`={workspace}"
    }

    pub(crate) write InsertCheckoutLocations(reponame: String, workspace: String, hostname: String, commit: CloudChangesetId, checkout_path: String, shared_path: String, unixname: String, timestamp: Timestamp) {
        none,
        mysql("INSERT INTO `checkoutlocations` (
            `reponame`,
            `workspace`,
            `hostname`,
            `commit`,
            `checkout_path`,
            `shared_path` ,
            `unixname`,
            `timestamp`
        ) VALUES (
            {reponame},
            {workspace},
            {hostname},
            {commit},
            {checkout_path},
            {shared_path},
            {unixname},
            {timestamp})
        ON DUPLICATE KEY UPDATE
            `commit` = {commit},
            `timestamp` = current_timestamp")

        sqlite("INSERT OR REPLACE INTO `checkoutlocations` (
            `reponame`,
            `workspace`,
            `hostname`,
            `commit`,
            `checkout_path`,
            `shared_path`,
            `unixname`,
            `timestamp`
        ) VALUES (
            {reponame},
            {workspace},
            {hostname},
            {commit},
            {checkout_path},
            {shared_path},
            {unixname},
            {timestamp})")
    }

    write UpdateWorkspaceName( reponame: String, workspace: String, new_workspace: String) {
        none,
        "UPDATE checkoutlocations SET workspace = {new_workspace} WHERE workspace = {workspace} and reponame = {reponame}"
    }

}

#[async_trait]
impl Get<WorkspaceCheckoutLocation> for SqlCommitCloud {
    async fn get(
        &self,
        ctx: &CoreContext,
        reponame: String,
        workspace: String,
    ) -> anyhow::Result<Vec<WorkspaceCheckoutLocation>> {
        let rows = GetCheckoutLocations::query(
            &self.connections.read_connection,
            ctx.sql_query_telemetry(),
            &reponame,
            &workspace,
        )
        .await?;

        rows.into_iter()
            .map(
                |(hostname, checkout_path, shared_path, commit, timestamp, unixname)| {
                    Ok(WorkspaceCheckoutLocation {
                        hostname,
                        commit,
                        checkout_path: PathBuf::from(checkout_path),
                        shared_path: PathBuf::from(shared_path),
                        timestamp,
                        unixname,
                    })
                },
            )
            .collect::<anyhow::Result<Vec<WorkspaceCheckoutLocation>>>()
    }
}

#[async_trait]
impl Insert<WorkspaceCheckoutLocation> for SqlCommitCloud {
    async fn insert(
        &self,
        txn: Transaction,
        _ctx: &CoreContext,
        reponame: String,
        workspace: String,
        data: WorkspaceCheckoutLocation,
    ) -> anyhow::Result<Transaction> {
        let (txn, _) = InsertCheckoutLocations::query_with_transaction(
            txn,
            &reponame,
            &workspace,
            &data.hostname,
            &data.commit,
            &data.checkout_path.display().to_string(),
            &data.shared_path.display().to_string(),
            &data.unixname,
            &data.timestamp,
        )
        .await?;
        Ok(txn)
    }
}

#[async_trait]
impl Update<WorkspaceCheckoutLocation> for SqlCommitCloud {
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
