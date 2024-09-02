/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::str::FromStr;

use commit_cloud::ctx::CommitCloudContext;
use commit_cloud::sql::builder::SqlCommitCloudBuilder;
use commit_cloud::sql::common::UpdateWorkspaceNameArgs;
use commit_cloud::sql::ops::Delete;
use commit_cloud::sql::ops::Insert;
use commit_cloud::sql::ops::Update;
use commit_cloud_types::WorkspaceSnapshot;
use fbinit::FacebookInit;
use mercurial_types::HgChangesetId;
use mononoke_macros::mononoke;
use sql_construct::SqlConstruct;

#[mononoke::fbinit_test]
async fn test_snapshots(_fb: FacebookInit) -> anyhow::Result<()> {
    use commit_cloud::sql::ops::Get;
    use commit_cloud::sql::snapshots_ops::DeleteArgs;

    let sql = SqlCommitCloudBuilder::with_sqlite_in_memory()?.new(false);

    let reponame = "test_repo".to_owned();
    let workspace = "user_testuser_default".to_owned();
    let renamed_workspace = "user_testuser_renamed_workspace".to_owned();
    let cc_ctx = CommitCloudContext::new(&workspace, &reponame)?;

    let snapshot1 = WorkspaceSnapshot {
        commit: HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536").unwrap(),
    };

    let snapshot2 = WorkspaceSnapshot {
        commit: HgChangesetId::from_str("3e0e761030db6e479a7fb58b12881883f9f8c63f").unwrap(),
    };
    let mut txn = sql.connections.write_connection.start_transaction().await?;
    txn = sql
        .insert(
            txn,
            None,
            reponame.clone(),
            workspace.clone(),
            snapshot1.clone(),
        )
        .await?;

    txn = sql
        .insert(
            txn,
            None,
            reponame.clone(),
            workspace.clone(),
            snapshot2.clone(),
        )
        .await?;
    txn.commit().await?;

    let res: Vec<WorkspaceSnapshot> = sql.get(reponame.clone(), workspace.clone()).await?;
    assert_eq!(res.len(), 2);

    let removed_commits = vec![snapshot1.commit];
    txn = sql.connections.write_connection.start_transaction().await?;
    txn = Delete::<WorkspaceSnapshot>::delete(
        &sql,
        txn,
        None,
        reponame.clone(),
        workspace.clone(),
        DeleteArgs { removed_commits },
    )
    .await?;
    txn.commit().await?;

    let res: Vec<WorkspaceSnapshot> = sql.get(reponame.clone(), workspace.clone()).await?;
    assert_eq!(res, vec![snapshot2.clone()]);

    let new_name_args = UpdateWorkspaceNameArgs {
        new_workspace: renamed_workspace.clone(),
    };
    let txn = sql.connections.write_connection.start_transaction().await?;
    let (txn, affected_rows) =
        Update::<WorkspaceSnapshot>::update(&sql, txn, None, cc_ctx, new_name_args).await?;
    txn.commit().await?;
    assert_eq!(affected_rows, 1);

    let res: Vec<WorkspaceSnapshot> = sql.get(reponame.clone(), renamed_workspace).await?;
    assert_eq!(res.len(), 1);

    Ok(())
}
