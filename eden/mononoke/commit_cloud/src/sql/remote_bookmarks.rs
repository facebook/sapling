/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use async_trait::async_trait;
use mercurial_types::HgChangesetId;
use serde::Deserialize;
use serde::Serialize;
use sql_ext::mononoke_queries;

use crate::sql::ops::Delete;
use crate::sql::ops::Get;
use crate::sql::ops::Insert;
use crate::sql::ops::SqlCommitCloud;
use crate::sql::ops::Update;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WorkspaceRemoteBookmark {
    pub name: String,
    pub commit: HgChangesetId,
    pub remote: String,
}

pub struct DeleteArgs {
    pub removed_bookmarks: Vec<String>,
}

mononoke_queries! {
    read GetRemoteBookmarks(reponame: String, workspace: String) -> (String, String, HgChangesetId){
        "SELECT `remote`, `name`, `commit` FROM `remotebookmarks`
        WHERE `reponame`={reponame} AND `workspace`={workspace}"
    }
    write DeleteRemoteBookmark(reponame: String, workspace: String,  >list removed_bookmarks: String) {
        none,
        mysql("DELETE FROM `remotebookmarks` WHERE `reponame`={reponame} AND `workspace`={workspace} AND CONCAT(`remote`, '/', `name`) IN {removed_bookmarks}")
        sqlite( "DELETE FROM `remotebookmarks` WHERE `reponame`={reponame} AND `workspace`={workspace} AND CAST(`remote`||'/'||`name` AS BLOB) IN {removed_bookmarks}")
    }
    write InsertRemoteBookmark(reponame: String, workspace: String, remote: String, name: String, commit: HgChangesetId) {
        none,
        "INSERT INTO `remotebookmarks` (`reponame`, `workspace`, `remote`,`name`, `commit` ) VALUES ({reponame}, {workspace}, {remote}, {name}, {commit})"
    }
}

#[async_trait]
impl Get<WorkspaceRemoteBookmark> for SqlCommitCloud {
    async fn get(
        &self,
        reponame: String,
        workspace: String,
    ) -> anyhow::Result<Vec<WorkspaceRemoteBookmark>> {
        let rows = GetRemoteBookmarks::query(
            &self.connections.read_connection,
            &reponame.clone(),
            &workspace,
        )
        .await?;
        rows.into_iter()
            .map(|(remote, name, commit)| {
                Ok(WorkspaceRemoteBookmark {
                    name,
                    commit,
                    remote,
                })
            })
            .collect::<anyhow::Result<Vec<WorkspaceRemoteBookmark>>>()
    }
}

#[async_trait]
impl Insert<WorkspaceRemoteBookmark> for SqlCommitCloud {
    async fn insert(
        &self,
        reponame: String,
        workspace: String,
        data: WorkspaceRemoteBookmark,
    ) -> anyhow::Result<()> {
        InsertRemoteBookmark::query(
            &self.connections.write_connection,
            &reponame,
            &workspace,
            &data.remote,
            &data.name,
            &data.commit,
        )
        .await?;
        Ok(())
    }
}

#[async_trait]
impl Update<WorkspaceRemoteBookmark> for SqlCommitCloud {
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
impl Delete<WorkspaceRemoteBookmark> for SqlCommitCloud {
    type DeleteArgs = DeleteArgs;
    async fn delete(
        &self,
        reponame: String,
        workspace: String,
        args: Self::DeleteArgs,
    ) -> anyhow::Result<()> {
        DeleteRemoteBookmark::query(
            &self.connections.write_connection,
            &reponame,
            &workspace,
            args.removed_bookmarks.as_slice(),
        )
        .await?;
        Ok(())
    }
}
