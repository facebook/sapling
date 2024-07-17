/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use clientinfo::ClientRequestInfo;
use edenapi_types::HgId;
use mercurial_types::HgChangesetId;
use serde::Deserialize;
use serde::Serialize;
use sql::Transaction;

use crate::sql::heads_ops::DeleteArgs;
use crate::sql::ops::Delete;
use crate::sql::ops::Insert;
use crate::CommitCloudContext;
use crate::SqlCommitCloud;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WorkspaceHead {
    pub commit: HgChangesetId,
}

pub async fn update_heads(
    sql_commit_cloud: &SqlCommitCloud,
    mut txn: Transaction,
    cri: Option<&ClientRequestInfo>,
    ctx: &CommitCloudContext,
    removed_heads: Vec<HgId>,
    new_heads: Vec<HgId>,
) -> anyhow::Result<Transaction> {
    if !removed_heads.is_empty() {
        let delete_args = DeleteArgs {
            removed_commits: removed_heads
                .into_iter()
                .map(|id| id.into())
                .collect::<Vec<HgChangesetId>>(),
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
            WorkspaceHead {
                commit: head.into(),
            },
        )
        .await?;
    }

    Ok(txn)
}
