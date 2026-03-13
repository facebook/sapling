/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

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
use crate::sql::ops::InsertMany;

#[allow(clippy::ptr_arg)]
pub fn heads_from_list(s: &Vec<String>) -> anyhow::Result<Vec<WorkspaceHead>> {
    s.iter()
        .map(|s| {
            Sha1::from_str(s).map(|commit_id| WorkspaceHead {
                commit: CloudChangesetId(commit_id),
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
        if justknobs::eval("scm/mononoke:commitcloud_bulk_inserts", None, None)? {
            let heads: Vec<WorkspaceHead> = new_heads
                .into_iter()
                .map(|commit| WorkspaceHead { commit })
                .collect();
            txn = InsertMany::<WorkspaceHead>::insert_many(
                sql_commit_cloud,
                txn,
                cc_ctx.reponame.clone(),
                cc_ctx.workspace.clone(),
                heads,
            )
            .await?;
        } else {
            for head in new_heads {
                txn = Insert::<WorkspaceHead>::insert(
                    sql_commit_cloud,
                    txn,
                    cc_ctx.reponame.clone(),
                    cc_ctx.workspace.clone(),
                    WorkspaceHead { commit: head },
                )
                .await?;
            }
        }
    }

    Ok(txn)
}
