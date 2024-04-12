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
use sql::Connection;

use crate::sql::ops::Delete;
use crate::sql::ops::Get;
use crate::sql::ops::Insert;
use crate::sql::ops::SqlCommitCloud;
use crate::sql::ops::Update;

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub struct WorkspaceLocalBookmark {
    pub name: String,
    pub commit: HgChangesetId,
}

pub struct DeleteArgs {
    pub removed_bookmarks: Vec<HgChangesetId>,
}

mononoke_queries! {
    read GetLocalBookmarks(reponame: String, workspace: String) -> (String, HgChangesetId){
        "SELECT `name`, `commit` FROM `workspacebookmarks`
        WHERE `reponame`={reponame} AND `workspace`={workspace}"
    }
    write DeleteLocalBookmark(reponame: String, workspace: String, >list removed_bookmarks: HgChangesetId) {
        none,
        "DELETE FROM `workspacebookmarks` WHERE `reponame`={reponame} AND `workspace`={workspace} AND `commit` IN {removed_bookmarks}"
    }
    write InsertLocalBookmark(reponame: String, workspace: String, name: String, commit: HgChangesetId) {
        none,
        "INSERT INTO `workspacebookmarks` (`reponame`, `workspace`, `name`, `commit`) VALUES ({reponame}, {workspace}, {name}, {commit})"
    }
}

#[async_trait]
impl Get<WorkspaceLocalBookmark> for SqlCommitCloud {
    async fn get(
        &self,
        reponame: String,
        workspace: String,
    ) -> anyhow::Result<Vec<WorkspaceLocalBookmark>> {
        let rows = GetLocalBookmarks::query(
            &self.connections.read_connection,
            &reponame.clone(),
            &workspace,
        )
        .await?;
        rows.into_iter()
            .map(|(name, commit)| Ok(WorkspaceLocalBookmark { name, commit }))
            .collect::<anyhow::Result<Vec<WorkspaceLocalBookmark>>>()
    }
}

#[async_trait]
impl Insert<WorkspaceLocalBookmark> for SqlCommitCloud {
    async fn insert(
        &self,
        reponame: String,
        workspace: String,
        data: WorkspaceLocalBookmark,
    ) -> anyhow::Result<()> {
        InsertLocalBookmark::query(
            &self.connections.write_connection,
            &reponame,
            &workspace,
            &data.name,
            &data.commit,
        )
        .await?;
        Ok(())
    }
}

#[async_trait]
impl Update<WorkspaceLocalBookmark> for SqlCommitCloud {
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
impl Delete<WorkspaceLocalBookmark> for SqlCommitCloud {
    type DeleteArgs = DeleteArgs;
    async fn delete(
        &self,
        reponame: String,
        workspace: String,
        args: Self::DeleteArgs,
    ) -> anyhow::Result<()> {
        DeleteLocalBookmark::query(
            &self.connections.write_connection,
            &reponame,
            &workspace,
            args.removed_bookmarks.as_slice(),
        )
        .await?;
        Ok(())
    }
}
