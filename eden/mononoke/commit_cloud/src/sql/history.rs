/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use ::sql_ext::mononoke_queries;
use async_trait::async_trait;
use mononoke_types::Timestamp;

use crate::sql::heads::WorkspaceHead;
use crate::sql::local_bookmarks::WorkspaceLocalBookmark;
use crate::sql::ops::Delete;
use crate::sql::ops::GenericGet;
use crate::sql::ops::Insert;
use crate::sql::ops::SqlCommitCloud;
use crate::sql::ops::Update;
use crate::sql::remote_bookmarks::WorkspaceRemoteBookmark;

#[derive(Clone, Debug, PartialEq)]
pub struct WorkspaceHistory {
    pub version: u64,
    pub timestamp: Option<Timestamp>,
    pub heads: Vec<WorkspaceHead>,
    pub local_bookmarks: Vec<WorkspaceLocalBookmark>,
    pub remote_bookmarks: Vec<WorkspaceRemoteBookmark>,
}

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
    read GetHistoryVersionTimestamp(reponame: String, workspace: String) -> ( u64, Timestamp){
        "SELECT `version`, `timestamp` FROM history WHERE reponame={reponame} AND workspace={workspace} ORDER BY version DESC LIMIT 500"
    }

    read GetHistoryDate(reponame: String, workspace: String, timestamp: Timestamp, limit: u64) -> (Vec<u8>, Vec<u8>, Vec<u8>, Timestamp, u64){
        "SELECT heads, bookmarks, remotebookmarks, timestamp, version FROM history
        WHERE reponame={reponame} AND workspace={workspace} AND timestamp >= {timestamp} ORDER BY timestamp LIMIT {limit}"
    }

    read GetHistoryVersion(reponame: String, workspace: String, version: u64) -> (Vec<u8>, Vec<u8>, Vec<u8>, Timestamp, u64){
        "SELECT heads, bookmarks, remotebookmarks, timestamp, version FROM history
        WHERE reponame={reponame} AND workspace={workspace} AND version={version}"
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
    write InsertHistoryNoTimestamp(reponame: String, workspace: String, version: u64, heads: Vec<u8>, bookmarks: Vec<u8>, remote_bookmarks: Vec<u8>) {
        none,
        "INSERT INTO history (reponame, workspace, version, heads, bookmarks, remotebookmarks)
        VALUES ({reponame},{workspace},{version},{heads},{bookmarks},{remote_bookmarks})"
    }

    write InsertHistory(reponame: String, workspace: String, version: u64, heads: Vec<u8>, bookmarks: Vec<u8>, remote_bookmarks: Vec<u8>, timestamp: Timestamp) {
        none,
        "INSERT INTO history (reponame, workspace, version, heads, bookmarks, remotebookmarks, timestamp)
        VALUES ({reponame},{workspace},{version},{heads},{bookmarks},{remote_bookmarks}, {timestamp}) "
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
                        Ok(GetOutput::VersionTimestamp((version, timestamp)))
                    })
                    .collect::<anyhow::Result<Vec<GetOutput>>>();
            }
            GetType::GetHistoryDate { timestamp, limit } => {
                let rows = GetHistoryDate::query(
                    &self.connections.read_connection,
                    &reponame,
                    &workspace,
                    &timestamp,
                    &limit,
                )
                .await?;
                return rows
                    .into_iter()
                    .map(|(heads, bookmarks, remotebookmarks, timestamp, version)| {
                        let heads: Vec<WorkspaceHead> = serde_json::from_slice(&heads)?;
                        let bookmarks: Vec<WorkspaceLocalBookmark> =
                            serde_json::from_slice(&bookmarks)?;
                        let remotebookmarks: Vec<WorkspaceRemoteBookmark> =
                            serde_json::from_slice(&remotebookmarks)?;
                        Ok(GetOutput::WorkspaceHistory(WorkspaceHistory {
                            version,
                            timestamp: Some(timestamp),
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
                        let heads: Vec<WorkspaceHead> = serde_json::from_slice(&heads)?;
                        let bookmarks: Vec<WorkspaceLocalBookmark> =
                            serde_json::from_slice(&bookmarks)?;
                        let remotebookmarks: Vec<WorkspaceRemoteBookmark> =
                            serde_json::from_slice(&remotebookmarks)?;
                        Ok(GetOutput::WorkspaceHistory(WorkspaceHistory {
                            version,
                            timestamp: Some(timestamp),
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
        reponame: String,
        workspace: String,
        args: Self::DeleteArgs,
    ) -> anyhow::Result<()> {
        DeleteHistory::query(
            &self.connections.write_connection,
            &reponame,
            &workspace,
            &args.keep_days,
            &args.keep_version,
            &args.delete_limit,
        )
        .await?;
        Ok(())
    }
}

#[async_trait]
impl Insert<WorkspaceHistory> for SqlCommitCloud {
    async fn insert(
        &self,
        reponame: String,
        workspace: String,
        data: WorkspaceHistory,
    ) -> anyhow::Result<()> {
        let mut heads_bytes: Vec<u8> = Vec::new();
        serde_json::to_writer(&mut heads_bytes, &data.heads).unwrap();
        let mut bookmarks_bytes: Vec<u8> = Vec::new();
        serde_json::to_writer(&mut bookmarks_bytes, &data.local_bookmarks).unwrap();
        let mut remotebookmarks_bytes: Vec<u8> = Vec::new();
        serde_json::to_writer(&mut remotebookmarks_bytes, &data.remote_bookmarks).unwrap();

        if data.timestamp.is_none() {
            InsertHistoryNoTimestamp::query(
                &self.connections.write_connection,
                &reponame,
                &workspace,
                &data.version,
                &heads_bytes,
                &bookmarks_bytes,
                &remotebookmarks_bytes,
            )
            .await?;
            Ok(())
        } else {
            InsertHistory::query(
                &self.connections.write_connection,
                &reponame,
                &workspace,
                &data.version,
                &heads_bytes,
                &bookmarks_bytes,
                &remotebookmarks_bytes,
                &data.timestamp.unwrap(),
            )
            .await?;
            Ok(())
        }
    }
}

#[async_trait]
impl Update<WorkspaceHistory> for SqlCommitCloud {
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
