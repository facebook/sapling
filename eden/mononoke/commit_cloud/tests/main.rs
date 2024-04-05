/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::PathBuf;
use std::str::FromStr;

use commit_cloud::builder::SqlCommitCloudBuilder;
use commit_cloud::checkout_locations::WorkspaceCheckoutLocation;
use commit_cloud::heads::HeadExtraArgs;
use commit_cloud::heads::WorkspaceHead;
use commit_cloud::local_bookmarks::LocalBookmarkExtraArgs;
use commit_cloud::local_bookmarks::WorkspaceLocalBookmark;
use commit_cloud::remote_bookmarks::RemoteBookmarkExtraArgs;
use commit_cloud::remote_bookmarks::WorkspaceRemoteBookmark;
use commit_cloud::snapshots::SnapshotExtraArgs;
use commit_cloud::snapshots::WorkspaceSnapshot;
use commit_cloud::versions::WorkspaceVersion;
use commit_cloud::BasicOps;
use fbinit::FacebookInit;
use mercurial_types::HgChangesetId;
use mononoke_types::Timestamp;
use sql_construct::SqlConstruct;
#[fbinit::test]
async fn test_checkout_locations(_fb: FacebookInit) -> anyhow::Result<()> {
    let sql = SqlCommitCloudBuilder::with_sqlite_in_memory()?.new();
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

    assert!(
        sql.insert(reponame.clone(), workspace.clone(), args, ())
            .await?
    );

    let res: Vec<WorkspaceCheckoutLocation> = sql.get(reponame, workspace, ()).await?;

    assert_eq!(vec![expected], res);
    Ok(())
}

#[fbinit::test]
async fn test_snapshots(_fb: FacebookInit) -> anyhow::Result<()> {
    let sql = SqlCommitCloudBuilder::with_sqlite_in_memory()?.new();
    let reponame = "test_repo".to_owned();
    let workspace = "user_testuser_default".to_owned();

    let snapshot1 = WorkspaceSnapshot {
        commit: HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536").unwrap(),
    };

    let snapshot2 = WorkspaceSnapshot {
        commit: HgChangesetId::from_str("3e0e761030db6e479a7fb58b12881883f9f8c63f").unwrap(),
    };

    assert!(
        sql.insert(reponame.clone(), workspace.clone(), snapshot1.clone(), None)
            .await?
    );
    assert!(
        sql.insert(reponame.clone(), workspace.clone(), snapshot2.clone(), None)
            .await?
    );

    let res: Vec<WorkspaceSnapshot> = sql.get(reponame.clone(), workspace.clone(), None).await?;
    assert!(res.len() == 2);

    let removed_commits = vec![snapshot1.commit];
    assert!(
        BasicOps::<WorkspaceSnapshot>::delete(
            &sql,
            reponame.clone(),
            workspace.clone(),
            Some(SnapshotExtraArgs { removed_commits })
        )
        .await?
    );
    let res: Vec<WorkspaceSnapshot> = sql.get(reponame, workspace.clone(), None).await?;

    assert_eq!(res, vec![snapshot2]);

    Ok(())
}

#[fbinit::test]
async fn test_heads(_fb: FacebookInit) -> anyhow::Result<()> {
    let sql = SqlCommitCloudBuilder::with_sqlite_in_memory()?.new();
    let reponame = "test_repo".to_owned();
    let workspace = "user_testuser_default".to_owned();

    let head1 = WorkspaceHead {
        commit: HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536").unwrap(),
    };

    let head2 = WorkspaceHead {
        commit: HgChangesetId::from_str("3e0e761030db6e479a7fb58b12881883f9f8c63f").unwrap(),
    };

    assert!(
        sql.insert(reponame.clone(), workspace.clone(), head1.clone(), None)
            .await?
    );
    assert!(
        sql.insert(reponame.clone(), workspace.clone(), head2.clone(), None)
            .await?
    );

    let res: Vec<WorkspaceHead> = sql.get(reponame.clone(), workspace.clone(), None).await?;
    assert!(res.len() == 2);
    let removed_commits = vec![head1.commit];
    assert!(
        BasicOps::<WorkspaceHead>::delete(
            &sql,
            reponame.clone(),
            workspace.clone(),
            Some(HeadExtraArgs { removed_commits })
        )
        .await?
    );
    let res: Vec<WorkspaceHead> = sql.get(reponame, workspace.clone(), None).await?;

    assert_eq!(res, vec![head2]);

    Ok(())
}

#[fbinit::test]
async fn test_local_bookmarks(_fb: FacebookInit) -> anyhow::Result<()> {
    let sql = SqlCommitCloudBuilder::with_sqlite_in_memory()?.new();
    let reponame = "test_repo".to_owned();
    let workspace = "user_testuser_default".to_owned();

    let bookmark1 = WorkspaceLocalBookmark {
        name: "my_bookmark1".to_owned(),
        commit: HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536").unwrap(),
    };

    let bookmark2 = WorkspaceLocalBookmark {
        name: "my_bookmark2".to_owned(),
        commit: HgChangesetId::from_str("3e0e761030db6e479a7fb58b12881883f9f8c63f").unwrap(),
    };

    assert!(
        sql.insert(reponame.clone(), workspace.clone(), bookmark1.clone(), None)
            .await?
    );
    assert!(
        sql.insert(reponame.clone(), workspace.clone(), bookmark2.clone(), None)
            .await?
    );

    let res: Vec<WorkspaceLocalBookmark> =
        sql.get(reponame.clone(), workspace.clone(), None).await?;
    assert_eq!(res.len(), 2);

    let removed_bookmarks = vec![bookmark1.commit.clone()];
    assert!(
        BasicOps::<WorkspaceLocalBookmark>::delete(
            &sql,
            reponame.clone(),
            workspace.clone(),
            Some(LocalBookmarkExtraArgs { removed_bookmarks })
        )
        .await?
    );
    let res: Vec<WorkspaceLocalBookmark> = sql.get(reponame, workspace.clone(), None).await?;
    assert_eq!(res, vec![bookmark2]);

    Ok(())
}

#[fbinit::test]
async fn test_remote_bookmarks(_fb: FacebookInit) -> anyhow::Result<()> {
    let sql = SqlCommitCloudBuilder::with_sqlite_in_memory()?.new();
    let reponame = "test_repo".to_owned();
    let workspace = "user_testuser_default".to_owned();

    let bookmark1 = WorkspaceRemoteBookmark {
        name: "my_bookmark1".to_owned(),
        commit: HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536").unwrap(),
        remote: "remote".to_owned(),
    };

    let bookmark2 = WorkspaceRemoteBookmark {
        name: "my_bookmark2".to_owned(),
        commit: HgChangesetId::from_str("3e0e761030db6e479a7fb58b12881883f9f8c63f").unwrap(),
        remote: "remote".to_owned(),
    };

    assert!(
        sql.insert(reponame.clone(), workspace.clone(), bookmark1.clone(), None)
            .await?
    );
    assert!(
        sql.insert(reponame.clone(), workspace.clone(), bookmark2.clone(), None)
            .await?
    );

    let res: Vec<WorkspaceRemoteBookmark> =
        sql.get(reponame.clone(), workspace.clone(), None).await?;

    assert_eq!(res.len(), 2);

    let removed_bookmarks = vec!["remote/my_bookmark1".to_owned()];

    assert!(
        BasicOps::<WorkspaceRemoteBookmark>::delete(
            &sql,
            reponame.clone(),
            workspace.clone(),
            Some(RemoteBookmarkExtraArgs {
                removed_bookmarks,
                remote: "remote".to_owned()
            })
        )
        .await?
    );

    let res: Vec<WorkspaceRemoteBookmark> = sql.get(reponame, workspace.clone(), None).await?;

    assert_eq!(res, vec![bookmark2]);

    Ok(())
}

#[fbinit::test]
async fn test_versions(_fb: FacebookInit) -> anyhow::Result<()> {
    let sql = SqlCommitCloudBuilder::with_sqlite_in_memory()?.new();
    let reponame = "test_repo".to_owned();
    let workspace = "user_testuser_default".to_owned();

    let args = WorkspaceVersion {
        workspace: workspace.clone(),
        version: 1,
        timestamp: Timestamp::now(),
        archived: false,
    };

    assert!(
        sql.insert(reponame.clone(), workspace.clone(), args.clone(), ())
            .await?
    );

    let res: Vec<WorkspaceVersion> = sql.get(reponame.clone(), workspace.clone(), ()).await?;
    assert_eq!(vec![args], res);

    let delete_res =
        BasicOps::<WorkspaceVersion>::delete(&sql, reponame.clone(), workspace.clone(), ()).await;

    assert_eq!(
        format!("{}", delete_res.unwrap_err()),
        "Deleting workspace versions is not supported"
    );

    Ok(())
}
