/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::str::FromStr;

use commit_cloud::ctx::CommitCloudContext;
use commit_cloud::references::remote_bookmarks::rbs_from_list;
use commit_cloud::references::remote_bookmarks::rbs_to_list;
use commit_cloud::sql::builder::SqlCommitCloudBuilder;
use commit_cloud::sql::common::UpdateWorkspaceNameArgs;
use commit_cloud::sql::ops::Delete;
use commit_cloud::sql::ops::GetAsMap;
use commit_cloud::sql::ops::Insert;
use commit_cloud::sql::ops::Update;
use commit_cloud_types::RemoteBookmarksMap;
use commit_cloud_types::WorkspaceRemoteBookmark;
use fbinit::FacebookInit;
use mercurial_types::HgChangesetId;
use mononoke_macros::mononoke;
use sql_construct::SqlConstruct;

#[mononoke::test]
fn test_remote_bookmark_creation() {
    let commit_id = HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536").unwrap();
    let bookmark =
        WorkspaceRemoteBookmark::new("origin".to_string(), "bookmark_name".to_string(), commit_id);
    assert!(bookmark.is_ok());
    let bookmark = bookmark.unwrap();
    assert_eq!(bookmark.name(), &"bookmark_name".to_string());
    assert_eq!(*bookmark.commit(), commit_id);
    assert_eq!(bookmark.remote(), &"origin".to_string());
}

#[mononoke::test]
fn test_remote_bookmark_creation_empty_name() {
    let commit_id = HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536").unwrap();
    let bookmark = WorkspaceRemoteBookmark::new("origin".to_string(), "".to_string(), commit_id);
    assert!(bookmark.is_err());
    assert_eq!(
        bookmark.unwrap_err().to_string(),
        "'commit cloud' failed: remote bookmark name cannot be empty"
    );
}

#[mononoke::test]
fn test_remote_bookmark_creation_empty_remote() {
    let commit_id = HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536").unwrap();
    let bookmark =
        WorkspaceRemoteBookmark::new("".to_string(), "bookmark_name".to_string(), commit_id);
    assert!(bookmark.is_err());
    assert_eq!(
        bookmark.unwrap_err().to_string(),
        "'commit cloud' failed: remote bookmark 'remote' part cannot be empty"
    );
}

#[mononoke::test]
fn test_rbs_from_list_valid() {
    let bookmarks = vec![vec![
        "origin".to_string(),
        "bookmark_name".to_string(),
        "2d7d4ba9ce0a6ffd222de7785b249ead9c51c536".to_string(),
    ]];
    let result = rbs_from_list(&bookmarks).unwrap();
    assert_eq!(result.len(), 1);
    let bookmark = &result[0];
    assert_eq!(bookmark.name(), &"bookmark_name".to_string());
    assert_eq!(
        *bookmark.commit(),
        HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536").unwrap()
    );
    assert_eq!(bookmark.remote(), &"origin".to_string());
}

#[mononoke::test]
fn test_rbs_from_list_invalid_format() {
    let bookmarks = vec![vec!["origin".to_string(), "bookmark_name".to_string()]];
    let result = rbs_from_list(&bookmarks);
    assert!(result.is_err());
    assert_eq!(
        result.unwrap_err().to_string(),
        "'commit cloud' failed: Invalid remote bookmark format for origin bookmark_name"
    );
}

#[mononoke::test]
fn test_rbs_to_list() {
    let commit_id = HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536").unwrap();
    let bookmark =
        WorkspaceRemoteBookmark::new("origin".to_string(), "bookmark_name".to_string(), commit_id)
            .unwrap();
    let bookmarks = vec![bookmark];
    let result = rbs_to_list(bookmarks);
    assert_eq!(result.len(), 1);
    let bookmark = &result[0];
    assert_eq!(bookmark.len(), 3);
    assert_eq!(bookmark[0], "origin");
    assert_eq!(bookmark[1], "bookmark_name");
    assert_eq!(bookmark[2], "2d7d4ba9ce0a6ffd222de7785b249ead9c51c536");
}

#[mononoke::fbinit_test]
async fn test_remote_bookmarks(_fb: FacebookInit) -> anyhow::Result<()> {
    use commit_cloud::sql::ops::Get;
    use commit_cloud::sql::remote_bookmarks_ops::DeleteArgs;

    let sql = SqlCommitCloudBuilder::with_sqlite_in_memory()?.new(false);
    let reponame = "test_repo".to_owned();
    let workspace = "user_testuser_default".to_owned();
    let renamed_workspace = "user_testuser_default_renamed".to_owned();
    let cc_ctx = CommitCloudContext::new(&workspace.clone(), &reponame.clone())?;
    let hgid1 = HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536").unwrap();
    let hgid2 = HgChangesetId::from_str("3e0e761030db6e479a7fb58b12881883f9f8c63f").unwrap();
    let bookmark1 =
        WorkspaceRemoteBookmark::new("remote".to_owned(), "my_bookmark1".to_owned(), hgid1)?;

    let bookmark2 =
        WorkspaceRemoteBookmark::new("remote".to_owned(), "my_bookmark2".to_owned(), hgid2)?;

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

    let res: Vec<WorkspaceRemoteBookmark> = sql.get(reponame.clone(), workspace.clone()).await?;
    assert_eq!(res.len(), 2);

    let res_map: RemoteBookmarksMap = sql.get_as_map(reponame.clone(), workspace.clone()).await?;
    assert_eq!(res_map.get(&hgid1).unwrap().to_vec(), vec![bookmark1]);
    assert_eq!(
        res_map.get(&hgid2).unwrap().to_vec(),
        vec![bookmark2.clone()]
    );

    let removed_bookmarks = vec!["remote/my_bookmark1".to_owned()];
    txn = sql.connections.write_connection.start_transaction().await?;
    txn = Delete::<WorkspaceRemoteBookmark>::delete(
        &sql,
        txn,
        None,
        reponame.clone(),
        workspace.clone(),
        DeleteArgs { removed_bookmarks },
    )
    .await?;
    txn.commit().await?;

    let res: Vec<WorkspaceRemoteBookmark> = sql.get(reponame.clone(), workspace.clone()).await?;
    assert_eq!(res, vec![bookmark2]);

    let new_name_args = UpdateWorkspaceNameArgs {
        new_workspace: renamed_workspace.clone(),
    };
    let txn = sql.connections.write_connection.start_transaction().await?;
    let (txn, affected_rows) =
        Update::<WorkspaceRemoteBookmark>::update(&sql, txn, None, cc_ctx, new_name_args).await?;
    txn.commit().await?;
    assert_eq!(affected_rows, 1);

    let res: Vec<WorkspaceRemoteBookmark> = sql.get(reponame.clone(), renamed_workspace).await?;
    assert_eq!(res.len(), 1);

    Ok(())
}
