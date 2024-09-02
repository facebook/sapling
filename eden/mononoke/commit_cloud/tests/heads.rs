/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::str::FromStr;

use commit_cloud::ctx::CommitCloudContext;
use commit_cloud::references::heads::heads_from_list;
use commit_cloud::references::heads::heads_to_list;
use commit_cloud::sql::builder::SqlCommitCloudBuilder;
use commit_cloud::sql::common::UpdateWorkspaceNameArgs;
use commit_cloud::sql::ops::Delete;
use commit_cloud::sql::ops::Insert;
use commit_cloud::sql::ops::Update;
use commit_cloud_types::WorkspaceHead;
use fbinit::FacebookInit;
use mercurial_types::HgChangesetId;
use mononoke_macros::mononoke;
use sql_construct::SqlConstruct;

#[mononoke::test]
fn test_heads_from_list_valid() {
    let input = vec!["2d7d4ba9ce0a6ffd222de7785b249ead9c51c536".to_string()];
    let expected = vec![WorkspaceHead {
        commit: HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536").unwrap(),
    }];
    let result = heads_from_list(&input).unwrap();
    assert_eq!(result, expected);
}

#[mononoke::test]
fn test_heads_from_list_invalid() {
    let input = vec!["invalid".to_string()];
    let result = heads_from_list(&input);
    assert!(result.is_err());
}

#[mononoke::test]
fn test_heads_to_list() {
    let input = vec![WorkspaceHead {
        commit: HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536").unwrap(),
    }];
    let expected = vec!["2d7d4ba9ce0a6ffd222de7785b249ead9c51c536".to_string()];
    let result = heads_to_list(&input);
    assert_eq!(result, expected);
}

#[mononoke::fbinit_test]
async fn test_heads(_fb: FacebookInit) -> anyhow::Result<()> {
    use commit_cloud::sql::heads_ops::DeleteArgs;
    use commit_cloud::sql::ops::Get;
    let sql = SqlCommitCloudBuilder::with_sqlite_in_memory()?.new(false);
    let reponame = "test_repo".to_owned();
    let workspace = "user_testuser_default".to_owned();
    let renamed_workspace = "user_testuser_default_renamed".to_owned();
    let cc_ctx = CommitCloudContext::new(&workspace.clone(), &reponame.clone())?;
    let head1 = WorkspaceHead {
        commit: HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536").unwrap(),
    };

    let head2 = WorkspaceHead {
        commit: HgChangesetId::from_str("3e0e761030db6e479a7fb58b12881883f9f8c63f").unwrap(),
    };
    let mut txn = sql.connections.write_connection.start_transaction().await?;
    txn = sql
        .insert(
            txn,
            None,
            reponame.clone(),
            workspace.clone(),
            head1.clone(),
        )
        .await?;

    txn = sql
        .insert(
            txn,
            None,
            reponame.clone(),
            workspace.clone(),
            head2.clone(),
        )
        .await?;
    txn.commit().await?;

    let res: Vec<WorkspaceHead> = sql.get(reponame.clone(), workspace.clone()).await?;
    assert_eq!(res.len(), 2);
    let removed_commits = vec![head1.commit];
    txn = sql.connections.write_connection.start_transaction().await?;
    txn = Delete::<WorkspaceHead>::delete(
        &sql,
        txn,
        None,
        reponame.clone(),
        workspace.clone(),
        DeleteArgs { removed_commits },
    )
    .await?;
    txn.commit().await?;

    let res: Vec<WorkspaceHead> = sql.get(reponame.clone(), workspace.clone()).await?;
    assert_eq!(res, vec![head2]);

    let new_name_args = UpdateWorkspaceNameArgs {
        new_workspace: renamed_workspace.clone(),
    };
    let txn = sql.connections.write_connection.start_transaction().await?;
    let (txn, affected_rows) =
        Update::<WorkspaceHead>::update(&sql, txn, None, cc_ctx, new_name_args).await?;
    txn.commit().await?;
    assert_eq!(affected_rows, 1);

    let res: Vec<WorkspaceHead> = sql.get(reponame.clone(), renamed_workspace).await?;
    assert_eq!(res.len(), 1);

    Ok(())
}
