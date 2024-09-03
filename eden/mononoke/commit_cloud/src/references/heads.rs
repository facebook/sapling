/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::str::FromStr;

use clientinfo::ClientRequestInfo;
use commit_cloud_types::WorkspaceHead;
use mercurial_types::HgChangesetId;
use sql::Transaction;

use crate::sql::heads_ops::DeleteArgs;
use crate::sql::ops::Delete;
use crate::sql::ops::Insert;
use crate::CommitCloudContext;
use crate::SqlCommitCloud;

#[allow(clippy::ptr_arg)]
pub fn heads_from_list(s: &Vec<String>) -> anyhow::Result<Vec<WorkspaceHead>> {
    s.iter()
        .map(|s| HgChangesetId::from_str(s).map(|commit| WorkspaceHead { commit }))
        .collect()
}

#[allow(clippy::ptr_arg)]
pub fn heads_to_list(heads: &Vec<WorkspaceHead>) -> Vec<String> {
    heads.iter().map(|head| head.commit.to_string()).collect()
}

pub async fn update_heads(
    sql_commit_cloud: &SqlCommitCloud,
    mut txn: Transaction,
    cri: Option<&ClientRequestInfo>,
    ctx: &CommitCloudContext,
    removed_heads: Vec<HgChangesetId>,
    new_heads: Vec<HgChangesetId>,
) -> anyhow::Result<Transaction> {
    if !removed_heads.is_empty() {
        let delete_args = DeleteArgs {
            removed_commits: removed_heads,
        };

        txn = Delete::<WorkspaceHead>::delete(
            sql_commit_cloud,
            txn,
            cri,
            ctx.reponame.clone(),
            ctx.workspace.clone(),
            delete_args,
        )
        .await?;
    }
    for head in new_heads {
        txn = Insert::<WorkspaceHead>::insert(
            sql_commit_cloud,
            txn,
            cri,
            ctx.reponame.clone(),
            ctx.workspace.clone(),
            WorkspaceHead { commit: head },
        )
        .await?;
    }

    Ok(txn)
}
