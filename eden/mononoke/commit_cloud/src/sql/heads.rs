/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use ::sql_ext::mononoke_queries;
use async_trait::async_trait;
use mercurial_types::HgChangesetId;
use serde::Deserialize;
use serde::Serialize;

use crate::sql::ops::Delete;
use crate::sql::ops::Get;
use crate::sql::ops::Insert;
use crate::sql::ops::SqlCommitCloud;
use crate::sql::ops::Update;
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WorkspaceHead {
    pub commit: HgChangesetId,
}

pub struct DeleteArgs {
    pub removed_commits: Vec<HgChangesetId>,
}

mononoke_queries! {
    read GetHeads(reponame: String, workspace: String) -> (String, HgChangesetId){
        "SELECT `reponame`, `commit` FROM `heads`
        WHERE `reponame`={reponame} AND `workspace`={workspace}
        ORDER BY `seq`"
    }

    write DeleteHead(reponame: String, workspace: String, >list commits: HgChangesetId) {
        none,
        "DELETE FROM `heads` WHERE `reponame`={reponame} AND `workspace`={workspace} AND `commit` IN {commits}"
    }

    write InsertHead(reponame: String, workspace: String, commit: HgChangesetId) {
        none,
        "INSERT INTO `heads` (`reponame`, `workspace`, `commit`) VALUES ({reponame}, {workspace}, {commit})"
    }
}

#[async_trait]
impl Get<WorkspaceHead> for SqlCommitCloud {
    async fn get(&self, reponame: String, workspace: String) -> anyhow::Result<Vec<WorkspaceHead>> {
        let rows =
            GetHeads::query(&self.connections.read_connection, &reponame, &workspace).await?;
        rows.into_iter()
            .map(|(_reponame, commit)| Ok(WorkspaceHead { commit }))
            .collect::<anyhow::Result<Vec<WorkspaceHead>>>()
    }
}

#[async_trait]
impl Insert<WorkspaceHead> for SqlCommitCloud {
    async fn insert(
        &self,
        reponame: String,
        workspace: String,
        data: WorkspaceHead,
    ) -> anyhow::Result<()> {
        InsertHead::query(
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
impl Update<WorkspaceHead> for SqlCommitCloud {
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
impl Delete<WorkspaceHead> for SqlCommitCloud {
    type DeleteArgs = DeleteArgs;
    async fn delete(
        &self,
        reponame: String,
        workspace: String,
        args: Self::DeleteArgs,
    ) -> anyhow::Result<()> {
        DeleteHead::query(
            &self.connections.write_connection,
            &reponame,
            &workspace,
            args.removed_commits.as_slice(),
        )
        .await?;
        Ok(())
    }
}
