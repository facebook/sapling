/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::str::FromStr;

use commit_cloud::references::remote_bookmarks::WorkspaceRemoteBookmark;
use commit_cloud::sql::builder::SqlCommitCloudBuilder;
use commit_cloud::sql::ops::Delete;
use commit_cloud::sql::ops::GetAsMap;
use commit_cloud::sql::ops::Insert;
use fbinit::FacebookInit;
use mercurial_types::HgChangesetId;
use sql_construct::SqlConstruct;

#[fbinit::test]
async fn test_remote_bookmarks(_fb: FacebookInit) -> anyhow::Result<()> {
    use commit_cloud::references::remote_bookmarks::RemoteBookmarksMap;
    use commit_cloud::sql::ops::Get;
    use commit_cloud::sql::remote_bookmarks_ops::DeleteArgs;

    let sql = SqlCommitCloudBuilder::with_sqlite_in_memory()?.new(false);
    let reponame = "test_repo".to_owned();
    let workspace = "user_testuser_default".to_owned();
    let hgid1 = HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536").unwrap();
    let hgid2 = HgChangesetId::from_str("3e0e761030db6e479a7fb58b12881883f9f8c63f").unwrap();
    let bookmark1 = WorkspaceRemoteBookmark {
        name: "my_bookmark1".to_owned(),
        commit: hgid1,
        remote: "remote".to_owned(),
    };

    let bookmark2 = WorkspaceRemoteBookmark {
        name: "my_bookmark2".to_owned(),
        commit: hgid2,
        remote: "remote".to_owned(),
    };

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
    assert_eq!(
        res_map
            .get(&hgid1)
            .unwrap()
            .iter()
            .map(|b| b.clone().into())
            .collect::<Vec<WorkspaceRemoteBookmark>>(),
        vec![bookmark1]
    );
    assert_eq!(
        res_map
            .get(&hgid2)
            .unwrap()
            .iter()
            .map(|b| b.clone().into())
            .collect::<Vec<WorkspaceRemoteBookmark>>(),
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

    Ok(())
}
