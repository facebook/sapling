/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::str::FromStr;

use commit_cloud_types::WorkspaceHead;
use commit_cloud_types::changeset::CloudChangesetId;
use context::CoreContext;
use mononoke_types::sha1_hash::Sha1;
use sql_ext::Transaction;

use crate::CommitCloudContext;
use crate::SqlCommitCloud;
use crate::sql::heads_ops::DeleteArgs;
use crate::sql::ops::Delete;
use crate::sql::ops::Insert;

#[allow(clippy::ptr_arg)]
pub fn heads_from_list(s: &Vec<String>) -> anyhow::Result<Vec<WorkspaceHead>> {
    s.iter()
        .map(|s| {
            Sha1::from_str(s).map(|commit_id| WorkspaceHead {
                commit: CloudChangesetId(commit_id),
                author_date: None,
            })
        })
        .collect()
}

#[allow(clippy::ptr_arg)]
pub fn heads_to_list(heads: &Vec<WorkspaceHead>) -> Vec<String> {
    heads.iter().map(|head| head.commit.to_string()).collect()
}

pub async fn update_heads(
    sql_commit_cloud: &SqlCommitCloud,
    mut txn: Transaction,
    _ctx: &CoreContext,
    cc_ctx: &CommitCloudContext,
    removed_heads: Vec<CloudChangesetId>,
    new_heads: Vec<CloudChangesetId>,
    head_author_dates: &HashMap<CloudChangesetId, i64>,
) -> anyhow::Result<Transaction> {
    if !removed_heads.is_empty() {
        let delete_args = DeleteArgs {
            removed_commits: removed_heads,
        };

        txn = Delete::<WorkspaceHead>::delete(
            sql_commit_cloud,
            txn,
            cc_ctx.reponame.clone(),
            cc_ctx.workspace.clone(),
            delete_args,
        )
        .await?;
    }
    if !new_heads.is_empty() {
        for head in new_heads {
            txn = Insert::<WorkspaceHead>::insert(
                sql_commit_cloud,
                txn,
                cc_ctx.reponame.clone(),
                cc_ctx.workspace.clone(),
                WorkspaceHead {
                    commit: head,
                    author_date: head_author_dates.get(&head).copied(),
                },
            )
            .await?;
        }
    }

    Ok(txn)
}
