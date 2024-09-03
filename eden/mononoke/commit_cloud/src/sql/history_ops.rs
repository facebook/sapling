/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use ::sql_ext::mononoke_queries;
use async_trait::async_trait;
use clientinfo::ClientRequestInfo;
use commit_cloud_types::references::WorkspaceRemoteBookmark;
use commit_cloud_types::WorkspaceHead;
use commit_cloud_types::WorkspaceLocalBookmark;
use mononoke_types::Timestamp;
use sql::Transaction;

use crate::ctx::CommitCloudContext;
use crate::references::heads::heads_from_list;
use crate::references::heads::heads_to_list;
use crate::references::history::WorkspaceHistory;
use crate::references::local_bookmarks::lbs_from_map;
use crate::references::local_bookmarks::lbs_to_map;
use crate::references::remote_bookmarks::rbs_from_list;
use crate::references::remote_bookmarks::rbs_to_list;
use crate::sql::common::UpdateWorkspaceNameArgs;
use crate::sql::ops::Delete;
use crate::sql::ops::GenericGet;
use crate::sql::ops::Insert;
use crate::sql::ops::SqlCommitCloud;
use crate::sql::ops::Update;

#[derive(Debug, Clone, PartialEq)]
pub enum GetType {
    GetHistoryVersionTimestamp,
    GetHistoryDate { timestamp: Timestamp, limit: u64 },
    GetHistoryVersion { version: u64 },
}

pub enum GetOutput {
    WorkspaceHistory(WorkspaceHistory),
    VersionTimestamp((u64, Timestamp)),
}

pub struct DeleteArgs {
    pub keep_days: u64,
    pub keep_version: u64,
    pub delete_limit: u64,
}

mononoke_queries! {
    read GetHistoryVersionTimestamp(reponame: String, workspace: String) -> ( u64, i64){
        mysql("SELECT `version`, UNIX_TIMESTAMP(`timestamp`) FROM history WHERE reponame={reponame} AND workspace={workspace} ORDER BY version DESC LIMIT 500")
        sqlite("SELECT `version`, `timestamp` FROM history WHERE reponame={reponame} AND workspace={workspace} ORDER BY version DESC LIMIT 500")
    }

    read GetHistoryDate(reponame: String, workspace: String, timestamp: i64, limit: u64) -> (Vec<u8>, Vec<u8>, Vec<u8>, i64, u64){
        mysql("SELECT heads, bookmarks, remotebookmarks, UNIX_TIMESTAMP(timestamp), version FROM history
        WHERE reponame={reponame} AND workspace={workspace} AND UNIX_TIMESTAMP(timestamp) >= {timestamp} ORDER BY timestamp LIMIT {limit}")
        sqlite("SELECT heads, bookmarks, remotebookmarks, timestamp, version FROM history
        WHERE reponame={reponame} AND workspace={workspace} AND timestamp >= {timestamp} ORDER BY timestamp LIMIT {limit}")
    }

    read GetHistoryVersion(reponame: String, workspace: String, version: u64) -> (Vec<u8>, Vec<u8>, Vec<u8>, i64, u64){
        mysql("SELECT heads, bookmarks, remotebookmarks, UNIX_TIMESTAMP(timestamp), version FROM history
        WHERE reponame={reponame} AND workspace={workspace} AND version={version}")
       sqlite( "SELECT heads, bookmarks, remotebookmarks, timestamp, version FROM history
        WHERE reponame={reponame} AND workspace={workspace} AND version={version}")
    }

    write DeleteHistory(reponame: String, workspace: String, keep_days: u64, keep_version: u64, delete_limit: u64) {
        none,
        mysql("DELETE FROM history WHERE reponame = {reponame} AND workspace = {workspace}
        AND timestamp < date_sub(current_timestamp(), INTERVAL {keep_days} DAY) AND version <= {keep_version}
        ORDER BY version
        LIMIT {delete_limit}")

        sqlite("DELETE FROM history WHERE (`reponame`,`workspace`,`version`) IN
        (SELECT `reponame`,`workspace`,`version` FROM history
          WHERE `reponame` = {reponame} AND `workspace` = {workspace} AND `timestamp` < datetime('now', '-'||{keep_days}||' day') AND `version` <= {keep_version}
          ORDER BY `version`
          LIMIT {delete_limit})")
    }

    write InsertHistory(reponame: String, workspace: String, version: u64, heads: Vec<u8>, bookmarks: Vec<u8>, remote_bookmarks: Vec<u8>, timestamp: i64) {
        none,
        mysql("INSERT INTO history (reponame, workspace, version, heads, bookmarks, remotebookmarks, timestamp)
        VALUES ({reponame}, {workspace}, {version}, {heads}, {bookmarks}, {remote_bookmarks}, FROM_UNIXTIME({timestamp}))")
        sqlite("INSERT INTO history (reponame, workspace, version, heads, bookmarks, remotebookmarks, timestamp)
        VALUES ({reponame},{workspace},{version},{heads},{bookmarks},{remote_bookmarks}, {timestamp}) ")
    }

    write UpdateWorkspaceName( reponame: String, workspace: String, new_workspace: String) {
        none,
        "UPDATE history SET workspace = {new_workspace} WHERE workspace = {workspace} and reponame = {reponame}"
    }


}

#[async_trait]
impl GenericGet<WorkspaceHistory> for SqlCommitCloud {
    type GetArgs = GetType;
    type GetOutput = GetOutput;

    async fn get(
        &self,
        reponame: String,
        workspace: String,
        args: Self::GetArgs,
    ) -> anyhow::Result<Vec<Self::GetOutput>> {
        match args {
            GetType::GetHistoryVersionTimestamp => {
                let rows = GetHistoryVersionTimestamp::query(
                    &self.connections.read_connection,
                    &reponame,
                    &workspace,
                )
                .await?;
                return rows
                    .into_iter()
                    .map(|(version, timestamp)| {
                        Ok(GetOutput::VersionTimestamp((
                            version,
                            Timestamp::from_timestamp_secs(timestamp),
                        )))
                    })
                    .collect::<anyhow::Result<Vec<GetOutput>>>();
            }
            GetType::GetHistoryDate { timestamp, limit } => {
                let rows = GetHistoryDate::query(
                    &self.connections.read_connection,
                    &reponame,
                    &workspace,
                    &timestamp.timestamp_seconds(),
                    &limit,
                )
                .await?;
                return rows
                    .into_iter()
                    .map(|(heads, bookmarks, remotebookmarks, timestamp, version)| {
                        let heads: Vec<WorkspaceHead> =
                            heads_from_list(&serde_json::from_slice(&heads)?)?;
                        let bookmarks: Vec<WorkspaceLocalBookmark> =
                            lbs_from_map(&serde_json::from_slice(&bookmarks)?)?;
                        let remotebookmarks: Vec<WorkspaceRemoteBookmark> =
                            rbs_from_list(&serde_json::from_slice(&remotebookmarks)?)?;
                        Ok(GetOutput::WorkspaceHistory(WorkspaceHistory {
                            version,
                            timestamp: Some(Timestamp::from_timestamp_secs(timestamp)),
                            heads,
                            local_bookmarks: bookmarks,
                            remote_bookmarks: remotebookmarks,
                        }))
                    })
                    .collect::<anyhow::Result<Vec<GetOutput>>>();
            }
            GetType::GetHistoryVersion { version } => {
                let rows = GetHistoryVersion::query(
                    &self.connections.read_connection,
                    &reponame,
                    &workspace,
                    &version,
                )
                .await?;
                return rows
                    .into_iter()
                    .map(|(heads, bookmarks, remotebookmarks, timestamp, version)| {
                        let heads: Vec<WorkspaceHead> =
                            heads_from_list(&serde_json::from_slice(&heads)?)?;
                        let bookmarks: Vec<WorkspaceLocalBookmark> =
                            lbs_from_map(&serde_json::from_slice(&bookmarks)?)?;
                        let remotebookmarks: Vec<WorkspaceRemoteBookmark> =
                            rbs_from_list(&serde_json::from_slice(&remotebookmarks)?)?;
                        Ok(GetOutput::WorkspaceHistory(WorkspaceHistory {
                            version,
                            timestamp: Some(Timestamp::from_timestamp_secs(timestamp)),
                            heads,
                            local_bookmarks: bookmarks,
                            remote_bookmarks: remotebookmarks,
                        }))
                    })
                    .collect::<anyhow::Result<Vec<GetOutput>>>();
            }
        }
    }
}

#[async_trait]
impl Delete<WorkspaceHistory> for SqlCommitCloud {
    type DeleteArgs = DeleteArgs;

    async fn delete(
        &self,
        txn: Transaction,
        cri: Option<&ClientRequestInfo>,
        reponame: String,
        workspace: String,
        args: Self::DeleteArgs,
    ) -> anyhow::Result<Transaction> {
        let (txn, _) = DeleteHistory::maybe_traced_query_with_transaction(
            txn,
            cri,
            &reponame,
            &workspace,
            &args.keep_days,
            &args.keep_version,
            &args.delete_limit,
        )
        .await?;
        Ok(txn)
    }
}

#[async_trait]
impl Insert<WorkspaceHistory> for SqlCommitCloud {
    async fn insert(
        &self,
        txn: Transaction,
        cri: Option<&ClientRequestInfo>,
        reponame: String,
        workspace: String,
        data: WorkspaceHistory,
    ) -> anyhow::Result<Transaction> {
        let mut heads_bytes: Vec<u8> = Vec::new();
        serde_json::to_writer(&mut heads_bytes, &heads_to_list(&data.heads)).unwrap();
        let mut bookmarks_bytes: Vec<u8> = Vec::new();
        serde_json::to_writer(&mut bookmarks_bytes, &lbs_to_map(data.local_bookmarks)).unwrap();
        let mut remotebookmarks_bytes: Vec<u8> = Vec::new();
        serde_json::to_writer(
            &mut remotebookmarks_bytes,
            &rbs_to_list(data.remote_bookmarks),
        )
        .unwrap();
        let res_txn;

        let timestamp = data.timestamp.unwrap_or(Timestamp::now_as_secs());

        (res_txn, _) = InsertHistory::maybe_traced_query_with_transaction(
            txn,
            cri,
            &reponame,
            &workspace,
            &data.version,
            &heads_bytes,
            &bookmarks_bytes,
            &remotebookmarks_bytes,
            &timestamp.timestamp_seconds(),
        )
        .await?;

        Ok(res_txn)
    }
}

#[async_trait]
impl Update<WorkspaceHistory> for SqlCommitCloud {
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
