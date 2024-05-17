/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use async_trait::async_trait;
use edenapi_types::cloud::RemoteBookmark;
use mercurial_types::HgChangesetId;
use serde::Deserialize;
use serde::Serialize;
use sql_ext::mononoke_queries;

use crate::sql::ops::Delete;
use crate::sql::ops::Get;
use crate::sql::ops::Insert;
use crate::sql::ops::SqlCommitCloud;
use crate::sql::ops::Update;
use crate::sql::utils::changeset_as_bytes;
use crate::sql::utils::changeset_from_bytes;
use crate::CommitCloudContext;
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
    read GetRemoteBookmarks(reponame: String, workspace: String) -> (String, String, Vec<u8>){
        mysql("SELECT `remote`, `name`, `node` FROM `remotebookmarks` WHERE `reponame`={reponame} AND `workspace`={workspace}")
        sqlite("SELECT `remote`, `name`, `commit` FROM `remotebookmarks` WHERE `reponame`={reponame} AND `workspace`={workspace}")
    }
    //TODO: Handle changesets as bytes (migth require an entirely different query)
    write DeleteRemoteBookmark(reponame: String, workspace: String,  >list removed_bookmarks: String) {
        none,
        mysql("DELETE FROM `remotebookmarks` WHERE `reponame`={reponame} AND `workspace`={workspace} AND CONCAT(`remote`, '/', `name`) IN {removed_bookmarks}")
        sqlite( "DELETE FROM `remotebookmarks` WHERE `reponame`={reponame} AND `workspace`={workspace} AND CAST(`remote`||'/'||`name` AS BLOB) IN {removed_bookmarks}")
    }
    write InsertRemoteBookmark(reponame: String, workspace: String, remote: String, name: String, commit:  Vec<u8>) {
        none,
        mysql("INSERT INTO `remotebookmarks` (`reponame`, `workspace`, `remote`,`name`, `node` ) VALUES ({reponame}, {workspace}, {remote}, {name}, {commit})")
        sqlite("INSERT INTO `remotebookmarks` (`reponame`, `workspace`, `remote`,`name`, `commit` ) VALUES ({reponame}, {workspace}, {remote}, {name}, {commit})")
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
                    commit: changeset_from_bytes(&commit, self.uses_mysql)?,
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
            &changeset_as_bytes(&data.commit, self.uses_mysql)?,
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

pub async fn update_remote_bookmarks(
    sql_commit_cloud: &SqlCommitCloud,
    ctx: CommitCloudContext,
    updated_remote_bookmarks: Option<Vec<RemoteBookmark>>,
    removed_remote_bookmarks: Option<Vec<RemoteBookmark>>,
) -> anyhow::Result<()> {
    if removed_remote_bookmarks
        .clone()
        .is_some_and(|x| !x.is_empty())
    {
        let removed_commits = removed_remote_bookmarks
            .unwrap()
            .into_iter()
            .map(|b| b.remote + &b.name)
            .collect::<Vec<_>>();
        let delete_args = DeleteArgs {
            removed_bookmarks: removed_commits,
        };

        Delete::<WorkspaceRemoteBookmark>::delete(
            sql_commit_cloud,
            ctx.reponame.clone(),
            ctx.workspace.clone(),
            delete_args,
        )
        .await?;
    }

    for book in updated_remote_bookmarks.unwrap_or_default() {
        //TODO: Resolve remote bookmarks if no node available (e.g. master)
        Insert::<WorkspaceRemoteBookmark>::insert(
            sql_commit_cloud,
            ctx.reponame.clone(),
            ctx.workspace.clone(),
            WorkspaceRemoteBookmark {
                name: book.name,
                commit: book.node.unwrap_or_default().into(),
                remote: book.remote,
            },
        )
        .await?;
    }

    Ok(())
}
