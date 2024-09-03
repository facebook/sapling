/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use clientinfo::ClientRequestInfo;
use commit_cloud_types::WorkspaceSnapshot;
use mercurial_types::HgChangesetId;
use sql::Transaction;

use crate::sql::ops::Delete;
use crate::sql::ops::Insert;
use crate::sql::snapshots_ops::DeleteArgs;
use crate::CommitCloudContext;
use crate::SqlCommitCloud;

pub async fn update_snapshots(
    sql_commit_cloud: &SqlCommitCloud,
    mut txn: Transaction,
    cri: Option<&ClientRequestInfo>,
    ctx: &CommitCloudContext,
    new_snapshots: Vec<HgChangesetId>,
    removed_snapshots: Vec<HgChangesetId>,
) -> anyhow::Result<Transaction> {
    if !removed_snapshots.is_empty() {
        let delete_args = DeleteArgs {
            removed_commits: removed_snapshots,
        };

        txn = Delete::<WorkspaceSnapshot>::delete(
            sql_commit_cloud,
            txn,
            cri,
            ctx.reponame.clone(),
            ctx.workspace.clone(),
            delete_args,
        )
        .await?;
    }
    for snapshot in new_snapshots {
        txn = Insert::<WorkspaceSnapshot>::insert(
            sql_commit_cloud,
            txn,
            cri,
            ctx.reponame.clone(),
            ctx.workspace.clone(),
            WorkspaceSnapshot { commit: snapshot },
        )
        .await?;
    }

    Ok(txn)
}
