/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use ::sql_ext::mononoke_queries;
use async_trait::async_trait;
use mononoke_types::Timestamp;
use sql::Connection;

use crate::sql::ops::Get;
use crate::sql::ops::Insert;
use crate::sql::ops::SqlCommitCloud;
use crate::sql::ops::Update;

#[derive(Clone, Debug, PartialEq)]
pub struct WorkspaceVersion {
    pub workspace: String,
    pub version: u64,
    pub timestamp: Timestamp,
    pub archived: bool,
}

mononoke_queries! {
    read GetVersion(reponame: String, workspace: String) -> (String, u64, bool, Timestamp){
        "SELECT workspace, version, archived, timestamp FROM versions WHERE reponame={reponame} AND workspace={workspace}"
    }

    // We have to check the version again inside the transaction because in rare case
    // it could be modified by another transaction fail the transaction in such cases
    write InsertVersion(reponame: String, workspace: String, version: u64, timestamp: Timestamp) {
        none,
        "INSERT INTO versions (`reponame`, `workspace`, `version`, `timestamp`)
        VALUES ({reponame}, {workspace}, {version}, {timestamp})
        ON CONFLICT(`reponame`, `workspace`)  DO UPDATE SET`timestamp` = CURRENT_TIMESTAMP,
        `version` = CASE
            WHEN `version` + 1 = {version} THEN {version}
            ELSE
                /* hack: the query below always generates runtime error this is a way to raise an exception (err 1242) */
                (SELECT name FROM sqlite_master WHERE type='table' LIMIT 2)
            END"
    }
}

#[async_trait]
impl Get<WorkspaceVersion> for SqlCommitCloud {
    async fn get(
        &self,
        reponame: String,
        workspace: String,
    ) -> anyhow::Result<Vec<WorkspaceVersion>> {
        let rows =
            GetVersion::query(&self.connections.read_connection, &reponame, &workspace).await?;
        rows.into_iter()
            .map(|(workspace, version, archived, timestamp)| {
                Ok(WorkspaceVersion {
                    workspace,
                    version,
                    archived,
                    timestamp,
                })
            })
            .collect::<anyhow::Result<Vec<WorkspaceVersion>>>()
    }
}

#[async_trait]
impl Insert<WorkspaceVersion> for SqlCommitCloud {
    async fn insert(
        &self,
        reponame: String,
        workspace: String,
        data: WorkspaceVersion,
    ) -> anyhow::Result<()> {
        InsertVersion::query(
            &self.connections.write_connection,
            &reponame,
            &workspace,
            &data.version,
            &data.timestamp,
        )
        .await?;
        Ok(())
    }
}

#[async_trait]
impl Update<WorkspaceVersion> for SqlCommitCloud {
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
