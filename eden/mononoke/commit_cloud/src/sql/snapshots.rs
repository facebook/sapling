/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use ::sql_ext::mononoke_queries;
use async_trait::async_trait;
use mercurial_types::HgChangesetId;

use crate::sql::ops::Delete;
use crate::sql::ops::Get;
use crate::sql::ops::Insert;
use crate::sql::ops::SqlCommitCloud;
use crate::sql::ops::Update;

#[derive(Clone, Debug, PartialEq)]
pub struct WorkspaceSnapshot {
    pub commit: HgChangesetId,
}

pub struct DeleteArgs {
    pub removed_commits: Vec<HgChangesetId>,
}

mononoke_queries! {
    read GetSnapshots(reponame: String, workspace: String) -> (String, HgChangesetId){
        "SELECT `reponame`, `commit` FROM snapshots
        WHERE `reponame`={reponame} AND `workspace`={workspace}
        ORDER BY `seq`"
    }

    write DeleteSnapshot(reponame: String, workspace: String, >list commits: HgChangesetId) {
        none,
        "DELETE FROM `snapshots` WHERE `reponame`={reponame} AND `workspace`={workspace} AND `commit` IN {commits}"
    }

    write InsertSnapshot(reponame: String, workspace: String, commit: HgChangesetId) {
        none,
        "INSERT INTO `snapshots` (`reponame`, `workspace`, `commit`) VALUES ({reponame}, {workspace}, {commit})"
    }
}

#[async_trait]
impl Get<WorkspaceSnapshot> for SqlCommitCloud {
    async fn get(
        &self,
        reponame: String,
        workspace: String,
    ) -> anyhow::Result<Vec<WorkspaceSnapshot>> {
        let rows =
            GetSnapshots::query(&self.connections.read_connection, &reponame, &workspace).await?;
        rows.into_iter()
            .map(|(_reponame, commit)| Ok(WorkspaceSnapshot { commit }))
            .collect::<anyhow::Result<Vec<WorkspaceSnapshot>>>()
    }
}
#[async_trait]
impl Insert<WorkspaceSnapshot> for SqlCommitCloud {
    async fn insert(
        &self,
        reponame: String,
        workspace: String,
        data: WorkspaceSnapshot,
    ) -> anyhow::Result<()> {
        InsertSnapshot::query(
            &self.connections.write_connection,
            &reponame,
            &workspace,
            &data.commit,
        )
        .await?;
        Ok(())
    }
}

#[async_trait]
impl Update<WorkspaceSnapshot> for SqlCommitCloud {
    type UpdateArgs = ();
    async fn update(
        &self,
        _reponame: String,
        _workspace: String,
        _extra_arg: Self::UpdateArgs,
    ) -> anyhow::Result<()> {
        //To be implemented among other Update queries
        return Err(anyhow::anyhow!("Not implemented yet"));
    }
}

#[async_trait]
impl Delete<WorkspaceSnapshot> for SqlCommitCloud {
    type DeleteArgs = DeleteArgs;
    async fn delete(
        &self,
        reponame: String,
        workspace: String,
        args: Self::DeleteArgs,
    ) -> anyhow::Result<()> {
        DeleteSnapshot::query(
            &self.connections.write_connection,
            &reponame,
            &workspace,
            args.removed_commits.as_slice(),
        )
        .await?;
        Ok(())
    }
}
