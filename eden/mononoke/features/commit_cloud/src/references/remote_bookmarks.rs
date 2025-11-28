/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::str::FromStr;

use anyhow::ensure;
use commit_cloud_types::WorkspaceRemoteBookmark;
use commit_cloud_types::changeset::CloudChangesetId;
use context::CoreContext;
use mononoke_types::sha1_hash::Sha1;
use sql_ext::Transaction;

use crate::CommitCloudContext;
use crate::sql::ops::Delete;
use crate::sql::ops::Insert;
use crate::sql::ops::SqlCommitCloud;
use crate::sql::remote_bookmarks_ops::DeleteArgs;

// This must stay as-is to work with serde
#[allow(clippy::ptr_arg)]
pub fn rbs_from_list(bookmarks: &Vec<Vec<String>>) -> anyhow::Result<Vec<WorkspaceRemoteBookmark>> {
    let bookmarks: anyhow::Result<Vec<WorkspaceRemoteBookmark>> = bookmarks
        .iter()
        .map(|bookmark| {
            ensure!(
                bookmark.len() == 3,
                "'commit cloud' failed: Invalid remote bookmark format for {}",
                bookmark.join(" ")
            );
            Sha1::from_str(&bookmark[2]).and_then(|commit_id| {
                WorkspaceRemoteBookmark::new(
                    bookmark[0].clone(),
                    bookmark[1].clone(),
                    CloudChangesetId(commit_id),
                )
            })
        })
        .collect();
    bookmarks
}

pub fn rbs_to_list(lbs: Vec<WorkspaceRemoteBookmark>) -> Vec<Vec<String>> {
    lbs.into_iter()
        .map(|lb| {
            vec![
                lb.remote().clone(),
                lb.name().clone(),
                lb.commit().to_string(),
            ]
        })
        .collect()
}

pub async fn update_remote_bookmarks(
    sql_commit_cloud: &SqlCommitCloud,
    mut txn: Transaction,
    ctx: &CoreContext,
    cc_ctx: &CommitCloudContext,
    updated_remote_bookmarks: Option<Vec<WorkspaceRemoteBookmark>>,
    removed_remote_bookmarks: Option<Vec<WorkspaceRemoteBookmark>>,
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
            ctx,
            cc_ctx.reponame.clone(),
            cc_ctx.workspace.clone(),
            delete_args,
        )
        .await?;
    }
    for book in updated_remote_bookmarks.unwrap_or_default() {
        //TODO: Resolve remote bookmarks if no node available (e.g. master)
        txn = Insert::<WorkspaceRemoteBookmark>::insert(
            sql_commit_cloud,
            txn,
            ctx,
            cc_ctx.reponame.clone(),
            cc_ctx.workspace.clone(),
            book,
        )
        .await?;
    }

    Ok(txn)
}
