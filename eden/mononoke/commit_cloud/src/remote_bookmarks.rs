/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use async_trait::async_trait;
use mercurial_types::HgChangesetId;
use sql_ext::mononoke_queries;

use crate::BasicOps;
use crate::SqlCommitCloud;

#[derive(Clone, Debug, PartialEq)]
pub struct WorkspaceRemoteBookmark {
    pub name: String,
    pub commit: HgChangesetId,
    pub remote: String,
}

pub struct RemoteBookmarkExtraArgs {
    pub remote: String,
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
impl BasicOps<WorkspaceRemoteBookmark> for SqlCommitCloud {
    type ExtraArgs = Option<RemoteBookmarkExtraArgs>;

    async fn get(
        &self,
        reponame: String,
        workspace: String,
        _extra_args: Self::ExtraArgs,
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

    async fn delete(
        &self,
        reponame: String,
        workspace: String,
        extra_args: Self::ExtraArgs,
    ) -> anyhow::Result<bool> {
        DeleteRemoteBookmark::query(
            &self.connections.write_connection,
            &reponame,
            &workspace,
            extra_args
                .expect("No removed commits list provided")
                .removed_bookmarks
                .as_slice(),
        )
        .await
        .map(|res| res.affected_rows() > 0)
    }

    async fn insert(
        &self,
        reponame: String,
        workspace: String,
        data: WorkspaceRemoteBookmark,
        _extra_args: Self::ExtraArgs,
    ) -> anyhow::Result<bool> {
        InsertRemoteBookmark::query(
            &self.connections.write_connection,
            &reponame,
            &workspace,
            &data.remote,
            &data.name,
            &data.commit,
        )
        .await
        .map(|res| res.affected_rows() > 0)
    }

    async fn update(
        &self,
        _reponame: String,
        _workspace: String,
        _extra_arg: Self::ExtraArgs,
    ) -> anyhow::Result<bool> {
        //To be implemented among other Update queries
        return Err(anyhow::anyhow!("Not implemented yet"));
    }
}
