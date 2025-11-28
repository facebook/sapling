/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use commit_cloud_types::WorkspaceSnapshot;
use commit_cloud_types::changeset::CloudChangesetId;
use context::CoreContext;
use sql_ext::Transaction;

use crate::CommitCloudContext;
use crate::SqlCommitCloud;
use crate::sql::ops::Delete;
use crate::sql::ops::Insert;
use crate::sql::snapshots_ops::DeleteArgs;

pub async fn update_snapshots(
    sql_commit_cloud: &SqlCommitCloud,
    mut txn: Transaction,
    ctx: &CoreContext,
    cc_ctx: &CommitCloudContext,
    new_snapshots: Vec<CloudChangesetId>,
    removed_snapshots: Vec<CloudChangesetId>,
) -> anyhow::Result<Transaction> {
    if !removed_snapshots.is_empty() {
        let delete_args = DeleteArgs { removed_snapshots };

        txn = Delete::<WorkspaceSnapshot>::delete(
            sql_commit_cloud,
            txn,
            ctx,
            cc_ctx.reponame.clone(),
            cc_ctx.workspace.clone(),
            delete_args,
        )
        .await?;
    }
    for snapshot in new_snapshots {
        txn = Insert::<WorkspaceSnapshot>::insert(
            sql_commit_cloud,
            txn,
            ctx,
            cc_ctx.reponame.clone(),
            cc_ctx.workspace.clone(),
            WorkspaceSnapshot { commit: snapshot },
        )
        .await?;
    }

    Ok(txn)
}
