/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::str::FromStr;

use commit_cloud::ctx::CommitCloudContext;
use commit_cloud::references::local_bookmarks::lbs_from_map;
use commit_cloud::references::local_bookmarks::lbs_to_map;
use commit_cloud::sql::builder::SqlCommitCloudBuilder;
use commit_cloud::sql::common::UpdateWorkspaceNameArgs;
use commit_cloud::sql::local_bookmarks_ops::DeleteArgs;
use commit_cloud::sql::ops::Delete;
use commit_cloud::sql::ops::Get;
use commit_cloud::sql::ops::GetAsMap;
use commit_cloud::sql::ops::Insert;
use commit_cloud::sql::ops::Update;
use commit_cloud_types::LocalBookmarksMap;
use commit_cloud_types::WorkspaceLocalBookmark;
use fbinit::FacebookInit;
use mercurial_types::HgChangesetId;
use mononoke_macros::mononoke;
use sql_construct::SqlConstruct;

#[mononoke::test]
fn test_local_bookmarks_success() {
    let commit_id = HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536").unwrap();
    let bookmark = WorkspaceLocalBookmark::new("valid_name".to_string(), commit_id);
    assert!(bookmark.is_ok());
}
#[mononoke::test]
fn test_local_bookmarks_empty_name() {
    let commit_id = HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536").unwrap();
    let bookmark = WorkspaceLocalBookmark::new("".to_string(), commit_id);
    assert!(bookmark.is_err());
    assert_eq!(
        bookmark.unwrap_err().to_string(),
        "'commit cloud' failed: Local bookmark name cannot be empty"
    );
}
#[mononoke::test]
fn test_local_bookmarks_invalid_commit() {
    let commit_id = HgChangesetId::from_str("invalidlength");
    assert!(commit_id.is_err());
}

#[mononoke::test]
fn test_lbs_from_map_valid() {
    let mut map = HashMap::new();
    map.insert(
        "bookmark1".to_string(),
        "2d7d4ba9ce0a6ffd222de7785b249ead9c51c536".to_string(),
    );
    let expected = vec![
        WorkspaceLocalBookmark::new(
            "bookmark1".to_string(),
            HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536").unwrap(),
        )
        .unwrap(),
    ];
    let result = lbs_from_map(&map).unwrap();
    assert_eq!(result, expected);
}

#[mononoke::test]
fn test_lbs_from_map_invalid_value() {
    let mut map = HashMap::new();
    map.insert("bookmark1".to_string(), "invalid".to_string());
    let result = lbs_from_map(&map);
    assert!(result.is_err());
}

#[mononoke::test]
fn test_lbs_to_map() {
    let list = vec![
        WorkspaceLocalBookmark::new(
            "bookmark1".to_string(),
            HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536").unwrap(),
        )
        .unwrap(),
    ];
    let mut expected = HashMap::new();
    expected.insert(
        "bookmark1".to_string(),
        "2d7d4ba9ce0a6ffd222de7785b249ead9c51c536".to_string(),
    );
    let result = lbs_to_map(list);
    assert_eq!(result, expected);
}

#[mononoke::fbinit_test]
async fn test_local_bookmarks(_fb: FacebookInit) -> anyhow::Result<()> {
    let sql = SqlCommitCloudBuilder::with_sqlite_in_memory()?.new(false);
    let reponame = "test_repo".to_owned();
    let workspace = "user_testuser_default".to_owned();
    let renamed_workspace = "user_testuser_default_renamed".to_owned();
    let cc_ctx = CommitCloudContext::new(&workspace.clone(), &reponame.clone())?;

    let hgid1 = HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536").unwrap();
    let hgid2 = HgChangesetId::from_str("3e0e761030db6e479a7fb58b12881883f9f8c63f").unwrap();

    let bookmark1 = WorkspaceLocalBookmark::new("my_bookmark1".to_owned(), hgid1)?;
    let bookmark2 = WorkspaceLocalBookmark::new("my_bookmark2".to_owned(), hgid2)?;

    let mut txn = sql.connections.write_connection.start_transaction().await?;
    txn = sql
        .insert(
            txn,
            None,
            reponame.clone(),
            workspace.clone(),
            bookmark1.clone(),
        )
        .await?;

    txn = sql
        .insert(
            txn,
            None,
            reponame.clone(),
            workspace.clone(),
            bookmark2.clone(),
        )
        .await?;
    txn.commit().await?;

    let res: Vec<WorkspaceLocalBookmark> = sql.get(reponame.clone(), workspace.clone()).await?;
    assert_eq!(res.len(), 2);

    let res_map: LocalBookmarksMap = sql.get_as_map(reponame.clone(), workspace.clone()).await?;
    assert_eq!(
        res_map.get(&hgid1).unwrap().to_owned(),
        vec!["my_bookmark1"]
    );
    assert_eq!(
        res_map.get(&hgid2).unwrap().to_owned(),
        vec!["my_bookmark2"]
    );

    let removed_bookmarks = vec![bookmark1.name().clone()];
    txn = sql.connections.write_connection.start_transaction().await?;
    txn = Delete::<WorkspaceLocalBookmark>::delete(
        &sql,
        txn,
        None,
        reponame.clone(),
        workspace.clone(),
        DeleteArgs { removed_bookmarks },
    )
    .await?;
    txn.commit().await?;
    let res: Vec<WorkspaceLocalBookmark> = sql.get(reponame.clone(), workspace.clone()).await?;
    assert_eq!(res, vec![bookmark2]);

    let new_name_args = UpdateWorkspaceNameArgs {
        new_workspace: renamed_workspace.clone(),
    };
    let txn = sql.connections.write_connection.start_transaction().await?;
    let (txn, affected_rows) =
        Update::<WorkspaceLocalBookmark>::update(&sql, txn, None, cc_ctx, new_name_args).await?;
    txn.commit().await?;
    assert_eq!(affected_rows, 1);

    let res: Vec<WorkspaceLocalBookmark> = sql.get(reponame.clone(), renamed_workspace).await?;
    assert_eq!(res.len(), 1);

    Ok(())
}
