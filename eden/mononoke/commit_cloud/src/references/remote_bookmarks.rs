/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;

use anyhow::ensure;
use clientinfo::ClientRequestInfo;
use mercurial_types::HgChangesetId;
use serde::Deserialize;
use serde::Serialize;
use sql::Transaction;

use crate::references::RemoteBookmark;
use crate::sql::ops::Delete;
use crate::sql::ops::Insert;
use crate::sql::ops::SqlCommitCloud;
use crate::sql::remote_bookmarks_ops::DeleteArgs;
use crate::CommitCloudContext;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WorkspaceRemoteBookmark {
    name: String,
    commit: HgChangesetId,
    remote: String,
}

impl WorkspaceRemoteBookmark {
    pub fn new(remote: String, name: String, commit: HgChangesetId) -> anyhow::Result<Self> {
        ensure!(
            !name.is_empty(),
            "'commit cloud' failed: remote bookmark name cannot be empty"
        );
        ensure!(
            !remote.is_empty(),
            "'commit cloud' failed: remote bookmark 'remote' part cannot be empty"
        );
        Ok(Self {
            name,
            commit,
            remote,
        })
    }

    pub fn name(&self) -> &String {
        &self.name
    }

    pub fn commit(&self) -> &HgChangesetId {
        &self.commit
    }

    pub fn remote(&self) -> &String {
        &self.remote
    }
}

pub type RemoteBookmarksMap = HashMap<HgChangesetId, Vec<RemoteBookmark>>;

impl From<RemoteBookmark> for WorkspaceRemoteBookmark {
    fn from(bookmark: RemoteBookmark) -> Self {
        Self {
            name: bookmark.name,
            commit: bookmark.node.unwrap_or_default().into(),
            remote: bookmark.remote,
        }
    }
}

pub async fn update_remote_bookmarks(
    sql_commit_cloud: &SqlCommitCloud,
    mut txn: Transaction,
    cri: Option<&ClientRequestInfo>,
    ctx: &CommitCloudContext,
    updated_remote_bookmarks: Option<Vec<RemoteBookmark>>,
    removed_remote_bookmarks: Option<Vec<RemoteBookmark>>,
) -> anyhow::Result<Transaction> {
    if removed_remote_bookmarks
        .clone()
        .is_some_and(|x| !x.is_empty())
    {
        let removed_commits = removed_remote_bookmarks
            .unwrap()
            .into_iter()
            .map(|b| b.full_name())
            .collect::<Vec<_>>();
        let delete_args = DeleteArgs {
            removed_bookmarks: removed_commits,
        };

        txn = Delete::<WorkspaceRemoteBookmark>::delete(
            sql_commit_cloud,
            txn,
            cri,
            ctx.reponame.clone(),
            ctx.workspace.clone(),
            delete_args,
        )
        .await?;
    }
    for book in updated_remote_bookmarks.unwrap_or_default() {
        //TODO: Resolve remote bookmarks if no node available (e.g. master)
        txn = Insert::<WorkspaceRemoteBookmark>::insert(
            sql_commit_cloud,
            txn,
            cri,
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

    Ok(txn)
}
