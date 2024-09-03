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
use commit_cloud_types::references::WorkspaceRemoteBookmark;
use commit_cloud_types::WorkspaceHead;
use commit_cloud_types::WorkspaceLocalBookmark;
use fbinit::FacebookInit;
use mercurial_types::HgChangesetId;
use mononoke_macros::mononoke;
use mononoke_types::Timestamp;
use sql_construct::SqlConstruct;

#[mononoke::fbinit_test]
async fn test_history(_fb: FacebookInit) -> anyhow::Result<()> {
    use commit_cloud::references::history::WorkspaceHistory;
    use commit_cloud::sql::history_ops::DeleteArgs;
    use commit_cloud::sql::history_ops::GetOutput;
    use commit_cloud::sql::history_ops::GetType;
    use commit_cloud::sql::ops::GenericGet;

    // Create a workspace with heads and bookmarks
    let sql = SqlCommitCloudBuilder::with_sqlite_in_memory()?.new(false);
    let reponame = "test_repo".to_owned();
    let workspace = "user_testuser_default".to_owned();
    let renamed_workspace = "user_testuser_default_renamed".to_owned();
    let cc_ctx = CommitCloudContext::new(&workspace.clone(), &reponame.clone())?;
    let timestamp = Timestamp::now_as_secs();

    let head1 = WorkspaceHead {
        commit: HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536").unwrap(),
    };

    let remote_bookmark1 = WorkspaceRemoteBookmark::new(
        "remote".to_owned(),
        "my_bookmark1".to_owned(),
        HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536").unwrap(),
    )?;

    let local_bookmark1 = WorkspaceLocalBookmark::new(
        "my_bookmark1".to_owned(),
        HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536").unwrap(),
    )?;

    let args1 = WorkspaceHistory {
        version: 1,
        timestamp: Some(Timestamp::now_as_secs()),
        heads: vec![head1.clone()],
        local_bookmarks: vec![local_bookmark1.clone()],
        remote_bookmarks: vec![remote_bookmark1.clone()],
    };

    let mut txn = sql.connections.write_connection.start_transaction().await?;
    // Insert a history entry, retrieve it and cast it to Rust struct
    txn = sql
        .insert(
            txn,
            None,
            reponame.clone(),
            workspace.clone(),
            args1.clone(),
        )
        .await?;
    txn.commit().await?;

    let res: Vec<GetOutput> = sql
        .get(
            reponame.clone(),
            workspace.clone(),
            GetType::GetHistoryVersion { version: 1 },
        )
        .await?;

    let res_as_history: Vec<WorkspaceHistory> = res
        .into_iter()
        .map(|output| match output {
            GetOutput::WorkspaceHistory(history) => history,
            _ => panic!("Output doesn't match query type"),
        })
        .collect::<Vec<WorkspaceHistory>>();

    assert_eq!(vec![args1], res_as_history);

    // Insert a new history entry
    let args2 = WorkspaceHistory {
        version: 2,
        timestamp: Some(Timestamp::now_as_secs()),
        heads: vec![head1],
        local_bookmarks: vec![local_bookmark1],
        remote_bookmarks: vec![remote_bookmark1],
    };
    txn = sql.connections.write_connection.start_transaction().await?;
    txn = sql
        .insert(
            txn,
            None,
            reponame.clone(),
            workspace.clone(),
            args2.clone(),
        )
        .await?;
    txn.commit().await?;

    // Delete first history entry, validate only second entry is left
    txn = sql.connections.write_connection.start_transaction().await?;
    txn = Delete::<WorkspaceHistory>::delete(
        &sql,
        txn,
        None,
        reponame.clone(),
        workspace.clone(),
        DeleteArgs {
            keep_days: 0,
            keep_version: 1,
            delete_limit: 1,
        },
    )
    .await?;
    txn.commit().await?;

    let res: Vec<GetOutput> = sql
        .get(
            reponame.clone(),
            workspace.clone(),
            GetType::GetHistoryDate {
                timestamp,
                limit: 2,
            },
        )
        .await?;

    let res_as_history: Vec<WorkspaceHistory> = res
        .into_iter()
        .map(|output| match output {
            GetOutput::WorkspaceHistory(history) => history,
            _ => panic!("Output doesn't match query type"),
        })
        .collect::<Vec<WorkspaceHistory>>();

    assert_eq!(vec![args2], res_as_history);

    let new_name_args = UpdateWorkspaceNameArgs {
        new_workspace: renamed_workspace.clone(),
    };
    let txn = sql.connections.write_connection.start_transaction().await?;
    let (txn, affected_rows) =
        Update::<WorkspaceHistory>::update(&sql, txn, None, cc_ctx, new_name_args).await?;
    txn.commit().await?;
    assert_eq!(affected_rows, 1);

    let res: Vec<GetOutput> = sql
        .get(
            reponame.clone(),
            renamed_workspace.clone(),
            GetType::GetHistoryDate {
                timestamp,
                limit: 2,
            },
        )
        .await?;
    assert_eq!(res.len(), 1);

    Ok(())
}
