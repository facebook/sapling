/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use mercurial_types::HgChangesetId;
use serde::Deserialize;
use serde::Serialize;

use crate::references::RemoteBookmark;
use crate::sql::ops::Delete;
use crate::sql::ops::Insert;
use crate::sql::ops::SqlCommitCloud;
use crate::sql::remote_bookmarks_ops::DeleteArgs;
use crate::CommitCloudContext;
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WorkspaceRemoteBookmark {
    pub name: String,
    pub commit: HgChangesetId,
    pub remote: String,
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
