/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use ::sql_ext::mononoke_queries;
use async_trait::async_trait;
use mercurial_types::HgChangesetId;

use crate::BasicOps;
use crate::SqlCommitCloud;

#[derive(Clone)]
pub struct WorkspaceSnapshot {
    pub commit: HgChangesetId,
}

pub struct SnapshotExtraArgs {
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
impl BasicOps<WorkspaceSnapshot> for SqlCommitCloud {
    type ExtraArgs = Option<SnapshotExtraArgs>;
    async fn get(
        &self,
        reponame: String,
        workspace: String,
        _extra_args: Self::ExtraArgs,
    ) -> anyhow::Result<Vec<WorkspaceSnapshot>> {
        let rows =
            GetSnapshots::query(&self.connections.read_connection, &reponame, &workspace).await?;
        rows.into_iter()
            .map(|(_reponame, commit)| Ok(WorkspaceSnapshot { commit }))
            .collect::<anyhow::Result<Vec<WorkspaceSnapshot>>>()
    }

    async fn delete(
        &self,
        reponame: String,
        workspace: String,
        extra_args: Option<SnapshotExtraArgs>,
    ) -> anyhow::Result<bool> {
        DeleteSnapshot::query(
            &self.connections.write_connection,
            &reponame,
            &workspace,
            extra_args
                .expect("No removed commits list provided")
                .removed_commits
                .as_slice(),
        )
        .await
        .map(|res| res.affected_rows() > 0)
    }

    async fn insert(
        &self,
        reponame: String,
        workspace: String,
        data: WorkspaceSnapshot,
        _extra_args: Option<SnapshotExtraArgs>,
    ) -> anyhow::Result<bool> {
        InsertSnapshot::query(
            &self.connections.write_connection,
            &reponame,
            &workspace,
            &data.commit,
        )
        .await
        .map(|res| res.affected_rows() > 0)
    }

    async fn update(
        &self,
        _reponame: String,
        _workspace: String,
        _extra_arg: Option<SnapshotExtraArgs>,
    ) -> anyhow::Result<bool> {
        //To be implemented among other Update queries
        return Err(anyhow::anyhow!("Not implemented yet"));
    }
}
