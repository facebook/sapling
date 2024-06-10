/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;

use clientinfo::ClientRequestInfo;
use edenapi_types::HgId;
use mercurial_types::HgChangesetId;
use serde::Deserialize;
use serde::Serialize;
use sql::Transaction;

use crate::sql::local_bookmarks_ops::DeleteArgs;
use crate::sql::ops::Delete;
use crate::sql::ops::Insert;
use crate::sql::ops::SqlCommitCloud;
use crate::CommitCloudContext;

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub struct WorkspaceLocalBookmark {
    pub name: String,
    pub commit: HgChangesetId,
}

pub async fn update_bookmarks(
    sql_commit_cloud: &SqlCommitCloud,
    mut txn: Transaction,
    cri: Option<&ClientRequestInfo>,
    ctx: CommitCloudContext,
    updated_bookmarks: HashMap<String, HgId>,
    removed_bookmarks: Vec<HgId>,
) -> anyhow::Result<Transaction> {
    if !removed_bookmarks.is_empty() {
        let removed_commits = removed_bookmarks
            .into_iter()
            .map(|id| id.into())
            .collect::<Vec<HgChangesetId>>();
        let delete_args = DeleteArgs {
            removed_bookmarks: removed_commits,
        };

        Delete::<WorkspaceLocalBookmark>::delete(
            sql_commit_cloud,
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
            WorkspaceLocalBookmark {
                name,
                commit: book.into(),
            },
        )
        .await?;
    }

    Ok(txn)
}
