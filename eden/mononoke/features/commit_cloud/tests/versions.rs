/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use commit_cloud::ctx::CommitCloudContext;
use commit_cloud::references::versions::WorkspaceVersion;
use commit_cloud::sql::builder::SqlCommitCloudBuilder;
use commit_cloud::sql::ops::Get;
use commit_cloud::sql::ops::Insert;
use commit_cloud::sql::ops::Update;
use commit_cloud::sql::versions_ops::UpdateVersionArgs;
use commit_cloud::sql::versions_ops::get_other_repo_workspaces;
use commit_cloud::sql::versions_ops::get_version_by_prefix;
use context::CoreContext;
use fbinit::FacebookInit;
use mononoke_macros::mononoke;
use mononoke_types::Timestamp;
use sql_construct::SqlConstruct;

#[mononoke::fbinit_test]
async fn test_versions(fb: FacebookInit) -> anyhow::Result<()> {
    let ctx = CoreContext::test_mock(fb);

    let sql = SqlCommitCloudBuilder::with_sqlite_in_memory()?.new();
    let reponame = "test_repo".to_owned();
    let reponame2 = "test_repo2".to_owned();
    let workspace = "user/testuser/default".to_owned();
    let renamed_workspace = "user/testuser/renamed_workspace".to_owned();
    let initial_timestamp = Timestamp::now_as_secs();
    let args = WorkspaceVersion {
        workspace: workspace.clone(),
        reponame: reponame.clone(),
        version: 1,
        timestamp: initial_timestamp,
        archived: false,
    };

    let args2 = WorkspaceVersion {
        workspace: workspace.clone(),
        reponame: reponame2.clone(),
        version: 1,
        timestamp: initial_timestamp,
        archived: false,
    };

    let mut txn = sql
        .connections
        .write_connection
        .start_transaction(ctx.sql_query_telemetry())
        .await?;
    txn = sql
        .insert(txn, &ctx, reponame.clone(), workspace.clone(), args.clone())
        .await?;
    txn = sql
        .insert(
            txn,
            &ctx,
            reponame2.clone(),
            workspace.clone(),
            args.clone(),
        )
        .await?;
    txn.commit().await?;

    let res: Vec<WorkspaceVersion> = sql.get(&ctx, reponame.clone(), workspace.clone()).await?;
    assert_eq!(vec![args.clone()], res);

    let res_prefix = get_version_by_prefix(
        &ctx,
        &sql.connections,
        reponame.clone(),
        "user/testuser/".to_string(),
    )
    .await?;
    assert_eq!(vec![args.clone()], res_prefix);

    let res_other_repo =
        get_other_repo_workspaces(&ctx, &sql.connections, workspace.clone()).await?;
    assert_eq!(vec![args, args2], res_other_repo);

    // Test version conflict
    let args2 = WorkspaceVersion {
        workspace: workspace.clone(),
        reponame: reponame.clone(),
        version: 2,
        timestamp: Timestamp::now_as_secs(),
        archived: false,
    };

    txn = sql
        .connections
        .write_connection
        .start_transaction(ctx.sql_query_telemetry())
        .await?;
    txn = sql
        .insert(
            txn,
            &ctx,
            reponame.clone(),
            workspace.clone(),
            args2.clone(),
        )
        .await?;
    txn.commit().await?;
    let res2: Vec<WorkspaceVersion> = sql.get(&ctx, reponame.clone(), workspace.clone()).await?;
    assert!(res2[0].timestamp >= initial_timestamp);

    let cc_ctx = CommitCloudContext::new(&workspace.clone(), &reponame.clone())?;
    let archive_args = UpdateVersionArgs::Archive(true);
    txn = sql
        .connections
        .write_connection
        .start_transaction(ctx.sql_query_telemetry())
        .await?;
    let (txn, affected_rows) =
        Update::<WorkspaceVersion>::update(&sql, txn, &ctx, cc_ctx.clone(), archive_args).await?;
    txn.commit().await?;
    assert_eq!(affected_rows, 1);
    let res3: Vec<WorkspaceVersion> = sql.get(&ctx, reponame.clone(), workspace.clone()).await?;
    assert!(res3[0].archived);

    let new_name_args = UpdateVersionArgs::WorkspaceName(renamed_workspace.clone());
    let txn = sql
        .connections
        .write_connection
        .start_transaction(ctx.sql_query_telemetry())
        .await?;
    let (txn, affected_rows) =
        Update::<WorkspaceVersion>::update(&sql, txn, &ctx, cc_ctx, new_name_args).await?;
    txn.commit().await?;
    assert_eq!(affected_rows, 1);

    let res4: Vec<WorkspaceVersion> = sql.get(&ctx, reponame.clone(), renamed_workspace).await?;
    assert_eq!(res4.len(), 1);

    Ok(())
}
