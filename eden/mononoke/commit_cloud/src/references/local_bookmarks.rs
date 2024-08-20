/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::str::FromStr;

use anyhow::ensure;
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
    name: String,
    commit: HgChangesetId,
}

impl WorkspaceLocalBookmark {
    pub fn new(name: String, commit: HgChangesetId) -> anyhow::Result<Self> {
        ensure!(
            !name.is_empty(),
            "'commit cloud' failed: Local bookmark name cannot be empty"
        );

        Ok(Self { name, commit })
    }

    pub fn name(&self) -> &String {
        &self.name
    }

    pub fn commit(&self) -> &HgChangesetId {
        &self.commit
    }
}

pub fn lbs_from_map(map: &HashMap<String, String>) -> anyhow::Result<Vec<WorkspaceLocalBookmark>> {
    map.iter()
        .map(|(name, commit)| {
            HgChangesetId::from_str(commit).map(|commit_id| WorkspaceLocalBookmark {
                name: name.to_string(),
                commit: commit_id,
            })
        })
        .collect()
}

pub fn lbs_to_map(list: Vec<WorkspaceLocalBookmark>) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for bookmark in list {
        map.insert(bookmark.name, bookmark.commit.to_string());
    }
    map
}

pub type LocalBookmarksMap = HashMap<HgChangesetId, Vec<String>>;

pub async fn update_bookmarks(
    sql_commit_cloud: &SqlCommitCloud,
    mut txn: Transaction,
    cri: Option<&ClientRequestInfo>,
    ctx: &CommitCloudContext,
    updated_bookmarks: HashMap<String, HgId>,
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
            WorkspaceLocalBookmark::new(name, book.into())?,
        )
        .await?;
    }

    Ok(txn)
}
