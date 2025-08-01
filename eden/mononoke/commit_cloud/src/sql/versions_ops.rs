/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use ::sql_ext::mononoke_queries;
use async_trait::async_trait;
use context::CoreContext;
use mononoke_types::Timestamp;
use sql_ext::SqlConnections;
use sql_ext::Transaction;

use crate::ctx::CommitCloudContext;
use crate::references::versions::WorkspaceVersion;
use crate::sql::ops::Get;
use crate::sql::ops::Insert;
use crate::sql::ops::SqlCommitCloud;
use crate::sql::ops::Update;
use crate::sql::utils::prepare_prefix;

mononoke_queries! {
    read GetVersion(reponame: String, workspace: String) -> (String, u64, bool,  Option<i64>){
        mysql("SELECT `workspace`, `version`, `archived`, UNIX_TIMESTAMP(`timestamp`) FROM `versions` WHERE `reponame`={reponame} AND `workspace`={workspace}")
        sqlite("SELECT `workspace`, `version`, `archived`, `timestamp` FROM `versions` WHERE `reponame`={reponame} AND `workspace`={workspace}")
    }

    read GetVersionByPrefix(reponame: String, prefix: String) -> (String,  u64, bool, Option<i64>){
        mysql("SELECT `workspace`, `version`, `archived`, UNIX_TIMESTAMP(`timestamp`) FROM `versions` WHERE `reponame`={reponame} AND `workspace` LIKE {prefix}")
        sqlite("SELECT `workspace`,  `version`, `archived`, `timestamp` FROM `versions` WHERE `reponame`={reponame} AND `workspace` LIKE {prefix}")
    }

    read GetOtherRepoWorkspaces(workspace:String) -> (String, String,  u64, bool, Option<i64>){
        mysql("SELECT `workspace`,`reponame`, `version`, `archived`, UNIX_TIMESTAMP(`timestamp`) FROM `versions` WHERE `workspace`={workspace}")
        sqlite("SELECT `workspace`,`reponame`,  `version`, `archived`, `timestamp` FROM `versions` WHERE `workspace`={workspace}")
    }

    // We have to check the version again inside the transaction because in rare case
    // it could be modified by another transaction fail the transaction in such cases
    write InsertVersion(reponame: String, workspace: String, version: u64, timestamp: i64, now: i64) {
        none,
        mysql("INSERT INTO versions (`reponame`, `workspace`, `version`, `timestamp`) VALUES ({reponame}, {workspace}, {version}, COALESCE(FROM_UNIXTIME({timestamp}),FROM_UNIXTIME({now}))) \
        ON DUPLICATE KEY UPDATE timestamp = current_timestamp, version = \
          IF(version + 1 = VALUES(version), \
            VALUES(version), \
            /* hack: the query below always generates runtime error \
              this is a way to raise an exception (err 1242) */ \
            (SELECT table_name FROM information_schema.tables LIMIT 2) \
          )")
        sqlite("INSERT INTO versions (`reponame`, `workspace`, `version`, `timestamp`)
        VALUES ({reponame}, {workspace}, {version}, {timestamp})
        ON CONFLICT(`reponame`, `workspace`)  DO UPDATE SET`timestamp` = {now} ,
        `version` = CASE
            WHEN `version` + 1 = {version} THEN {version}
            ELSE
                /* hack: the query below always generates runtime error this is a way to raise an exception (err 1242) */
                (SELECT name FROM sqlite_master WHERE type='table' LIMIT 2)
            END")
    }

    write UpdateArchive(reponame: String, workspace: String, archived: bool) {
        none,
        "UPDATE versions SET archived={archived} WHERE reponame={reponame} AND workspace={workspace}"
    }

    write UpdateWorkspaceName( reponame: String, workspace: String, new_workspace: String) {
        none,
        "UPDATE versions SET workspace = {new_workspace} WHERE workspace = {workspace} and reponame = {reponame}"
    }

}

#[async_trait]
impl Get<WorkspaceVersion> for SqlCommitCloud {
    async fn get(
        &self,
        ctx: &CoreContext,
        reponame: String,
        workspace: String,
    ) -> anyhow::Result<Vec<WorkspaceVersion>> {
        let rows = GetVersion::query(
            &self.connections.read_connection,
            ctx.sql_query_telemetry(),
            &reponame.clone(),
            &workspace,
        )
        .await?;
        rows.into_iter()
            .map(|(workspace, version, archived, timestamp)| {
                Ok(WorkspaceVersion {
                    workspace,
                    reponame: reponame.clone(),
                    version,
                    archived,
                    timestamp: Timestamp::from_timestamp_secs(timestamp.unwrap_or(0)),
                })
            })
            .collect::<anyhow::Result<Vec<WorkspaceVersion>>>()
    }
}

#[async_trait]
impl Insert<WorkspaceVersion> for SqlCommitCloud {
    async fn insert(
        &self,
        txn: Transaction,
        _ctx: &CoreContext,
        reponame: String,
        workspace: String,
        data: WorkspaceVersion,
    ) -> anyhow::Result<Transaction> {
        let (txn, _) = InsertVersion::query_with_transaction(
            txn,
            &reponame,
            &workspace,
            &data.version,
            &data.timestamp.timestamp_seconds(),
            &Timestamp::now().timestamp_seconds(),
        )
        .await?;
        Ok(txn)
    }
}

pub enum UpdateVersionArgs {
    Archive(bool),
    WorkspaceName(String),
}

#[async_trait]
impl Update<WorkspaceVersion> for SqlCommitCloud {
    type UpdateArgs = UpdateVersionArgs;
    async fn update(
        &self,
        txn: Transaction,
        _ctx: &CoreContext,
        cc_ctx: CommitCloudContext,
        args: Self::UpdateArgs,
    ) -> anyhow::Result<(Transaction, u64)> {
        match args {
            UpdateVersionArgs::Archive(archived) => {
                let (txn, result) = UpdateArchive::query_with_transaction(
                    txn,
                    &cc_ctx.reponame,
                    &cc_ctx.workspace,
                    &archived,
                )
                .await?;
                Ok((txn, result.affected_rows()))
            }
            UpdateVersionArgs::WorkspaceName(new_workspace) => {
                let (txn, result) = UpdateWorkspaceName::query_with_transaction(
                    txn,
                    &cc_ctx.reponame,
                    &cc_ctx.workspace,
                    &new_workspace,
                )
                .await?;
                return Ok((txn, result.affected_rows()));
            }
        }
    }
}

pub async fn get_version_by_prefix(
    ctx: &CoreContext,
    connections: &SqlConnections,
    reponame: String,
    prefix: String,
) -> anyhow::Result<Vec<WorkspaceVersion>> {
    let rows = GetVersionByPrefix::query(
        &connections.read_connection,
        ctx.sql_query_telemetry(),
        &reponame.clone(),
        &prepare_prefix(&prefix),
    )
    .await?;
    rows.into_iter()
        .map(|(workspace, version, archived, timestamp)| {
            Ok(WorkspaceVersion {
                workspace,
                reponame: reponame.clone(),
                version,
                archived,
                timestamp: Timestamp::from_timestamp_secs(timestamp.unwrap_or(0)),
            })
        })
        .collect::<anyhow::Result<Vec<WorkspaceVersion>>>()
}

pub async fn get_other_repo_workspaces(
    ctx: &CoreContext,
    connections: &SqlConnections,
    workspace: String,
) -> anyhow::Result<Vec<WorkspaceVersion>> {
    let rows = GetOtherRepoWorkspaces::query(
        &connections.read_connection,
        ctx.sql_query_telemetry(),
        &workspace,
    )
    .await?;
    rows.into_iter()
        .map(|(workspace, reponame, version, archived, timestamp)| {
            Ok(WorkspaceVersion {
                workspace,
                reponame,
                version,
                archived,
                timestamp: Timestamp::from_timestamp_secs(timestamp.unwrap_or(0)),
            })
        })
        .collect::<anyhow::Result<Vec<WorkspaceVersion>>>()
}
