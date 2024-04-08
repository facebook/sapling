/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use ::sql_ext::mononoke_queries;
use async_trait::async_trait;
use mercurial_types::HgChangesetId;
use sql::Connection;

use crate::sql::ops::BasicOps;
use crate::sql::ops::SqlCommitCloud;

#[derive(Clone, Debug, PartialEq)]
pub struct WorkspaceLocalBookmark {
    pub name: String,
    pub commit: HgChangesetId,
}

pub struct LocalBookmarkExtraArgs {
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
impl BasicOps<WorkspaceLocalBookmark> for SqlCommitCloud {
    type ExtraArgs = Option<LocalBookmarkExtraArgs>;
    async fn get(
        &self,
        reponame: String,
        workspace: String,
        _extra_args: Self::ExtraArgs,
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

    async fn delete(
        &self,
        reponame: String,
        workspace: String,
        extra_args: Self::ExtraArgs,
    ) -> anyhow::Result<bool> {
        DeleteLocalBookmark::query(
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
        data: WorkspaceLocalBookmark,
        _extra_args: Self::ExtraArgs,
    ) -> anyhow::Result<bool> {
        InsertLocalBookmark::query(
            &self.connections.write_connection,
            &reponame,
            &workspace,
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
