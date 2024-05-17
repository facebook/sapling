/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use edenapi_types::HgId;
use mercurial_types::HgChangesetId;

use crate::sql::ops::Delete;
use crate::sql::ops::Insert;
use crate::sql::snapshots_ops::DeleteArgs;
use crate::CommitCloudContext;
use crate::SqlCommitCloud;

#[derive(Clone, Debug, PartialEq)]
pub struct WorkspaceSnapshot {
    pub commit: HgChangesetId,
}

pub async fn update_snapshots(
    sql_commit_cloud: &SqlCommitCloud,
    ctx: CommitCloudContext,
    new_snapshots: Vec<HgId>,
    removed_snapshots: Vec<HgId>,
) -> anyhow::Result<()> {
    if !removed_snapshots.is_empty() {
        let delete_args = DeleteArgs {
            removed_commits: removed_snapshots
                .into_iter()
                .map(|id| id.into())
                .collect::<Vec<HgChangesetId>>(),
        };

        Delete::<WorkspaceSnapshot>::delete(
            sql_commit_cloud,
            ctx.reponame.clone(),
            ctx.workspace.clone(),
            delete_args,
        )
        .await?;
    }

    for snapshot in new_snapshots {
        Insert::<WorkspaceSnapshot>::insert(
            sql_commit_cloud,
            ctx.reponame.clone(),
            ctx.workspace.clone(),
            WorkspaceSnapshot {
                commit: snapshot.into(),
            },
        )
        .await?;
    }

    Ok(())
}
