/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::PathBuf;
use std::str::FromStr;

use commit_cloud::sql::builder::SqlCommitCloudBuilder;
use commit_cloud::sql::checkout_locations_ops::WorkspaceCheckoutLocation;
use commit_cloud::sql::ops::Insert;
use fbinit::FacebookInit;
use mercurial_types::HgChangesetId;
use mononoke_types::Timestamp;
use sql_construct::SqlConstruct;

#[fbinit::test]
async fn test_checkout_locations(_fb: FacebookInit) -> anyhow::Result<()> {
    use commit_cloud::sql::ops::Get;
    let sql = SqlCommitCloudBuilder::with_sqlite_in_memory()?.new(false);
    let reponame = "test_repo".to_owned();
    let workspace = "user_testuser_default".to_owned();

    let args = WorkspaceCheckoutLocation {
        hostname: "testhost".to_owned(),
        commit: HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536").unwrap(),
        checkout_path: PathBuf::from("checkout/path"),
        shared_path: PathBuf::from("shared/path"),
        timestamp: Timestamp::now(),
        unixname: "testuser".to_owned(),
    };
    let expected = args.clone();
    let mut txn = sql.connections.write_connection.start_transaction().await?;

    txn = sql
        .insert(txn, None, reponame.clone(), workspace.clone(), args)
        .await?;
    txn.commit().await?;

    let res: Vec<WorkspaceCheckoutLocation> = sql.get(reponame, workspace).await?;

    assert_eq!(vec![expected], res);
    Ok(())
}
