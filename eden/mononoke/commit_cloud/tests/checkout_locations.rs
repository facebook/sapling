/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::PathBuf;
use std::str::FromStr;

use commit_cloud::ctx::CommitCloudContext;
use commit_cloud::sql::builder::SqlCommitCloudBuilder;
use commit_cloud::sql::common::UpdateWorkspaceNameArgs;
use commit_cloud::sql::ops::Insert;
use commit_cloud::sql::ops::Update;
use commit_cloud_types::WorkspaceCheckoutLocation;
use context::CoreContext;
use fbinit::FacebookInit;
use mononoke_macros::mononoke;
use mononoke_types::Timestamp;
use mononoke_types::sha1_hash::Sha1;
use sql_construct::SqlConstruct;

#[mononoke::fbinit_test]
async fn test_checkout_locations(fb: FacebookInit) -> anyhow::Result<()> {
    use commit_cloud::sql::ops::Get;
    use commit_cloud_types::changeset::CloudChangesetId;
    let ctx = CoreContext::test_mock(fb);

    let sql = SqlCommitCloudBuilder::with_sqlite_in_memory()?.new();
    let reponame = "test_repo".to_owned();
    let workspace = "user_testuser_default".to_owned();
    let renamed_workspace = "user_testuser_default_renamed".to_owned();
    let cc_ctx = CommitCloudContext::new(&workspace.clone(), &reponame.clone())?;

    let args = WorkspaceCheckoutLocation {
        hostname: "testhost".to_owned(),
        commit: CloudChangesetId(
            Sha1::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536").unwrap(),
        ),
        checkout_path: PathBuf::from("checkout/path"),
        shared_path: PathBuf::from("shared/path"),
        timestamp: Timestamp::now(),
        unixname: "testuser".to_owned(),
    };
    let expected = args.clone();
    let mut txn = sql
        .connections
        .write_connection
        .start_transaction(ctx.sql_query_telemetry())
        .await?;

    txn = sql
        .insert(txn, &ctx, reponame.clone(), workspace.clone(), args)
        .await?;
    txn.commit().await?;

    let res: Vec<WorkspaceCheckoutLocation> = sql.get(&ctx, reponame.clone(), workspace).await?;
    assert_eq!(vec![expected], res);

    let new_name_args = UpdateWorkspaceNameArgs {
        new_workspace: renamed_workspace.clone(),
    };
    let txn = sql
        .connections
        .write_connection
        .start_transaction(ctx.sql_query_telemetry())
        .await?;
    let (txn, affected_rows) =
        Update::<WorkspaceCheckoutLocation>::update(&sql, txn, &ctx, cc_ctx, new_name_args).await?;
    txn.commit().await?;
    assert_eq!(affected_rows, 1);

    let res: Vec<WorkspaceCheckoutLocation> =
        sql.get(&ctx, reponame.clone(), renamed_workspace).await?;
    assert_eq!(res.len(), 1);

    Ok(())
}
