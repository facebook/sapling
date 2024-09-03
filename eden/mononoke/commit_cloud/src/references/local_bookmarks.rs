/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::str::FromStr;

use clientinfo::ClientRequestInfo;
use commit_cloud_types::WorkspaceLocalBookmark;
use mercurial_types::HgChangesetId;
use sql::Transaction;

use crate::sql::local_bookmarks_ops::DeleteArgs;
use crate::sql::ops::Delete;
use crate::sql::ops::Insert;
use crate::sql::ops::SqlCommitCloud;
use crate::CommitCloudContext;

pub fn lbs_from_map(map: &HashMap<String, String>) -> anyhow::Result<Vec<WorkspaceLocalBookmark>> {
    map.iter()
        .map(|(name, commit)| {
            HgChangesetId::from_str(commit)
                .and_then(|commit_id| WorkspaceLocalBookmark::new(name.to_string(), commit_id))
        })
        .collect()
}

pub fn lbs_to_map(list: Vec<WorkspaceLocalBookmark>) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for bookmark in list {
        map.insert(bookmark.name().clone(), bookmark.commit().to_string());
    }
    map
}

pub async fn update_bookmarks(
    sql_commit_cloud: &SqlCommitCloud,
    mut txn: Transaction,
    cri: Option<&ClientRequestInfo>,
    ctx: &CommitCloudContext,
    updated_bookmarks: HashMap<String, HgChangesetId>,
    removed_bookmarks: Vec<String>,
) -> anyhow::Result<Transaction> {
    if !removed_bookmarks.is_empty() {
        let delete_args = DeleteArgs { removed_bookmarks };

        txn = Delete::<WorkspaceLocalBookmark>::delete(
            sql_commit_cloud,
            txn,
            cri,
            ctx.reponame.clone(),
            ctx.workspace.clone(),
            delete_args,
        )
        .await?;
    }
    for (name, book) in updated_bookmarks {
        txn = Insert::<WorkspaceLocalBookmark>::insert(
            sql_commit_cloud,
            txn,
            cri,
            ctx.reponame.clone(),
            ctx.workspace.clone(),
            WorkspaceLocalBookmark::new(name, book)?,
        )
        .await?;
    }

    Ok(txn)
}
