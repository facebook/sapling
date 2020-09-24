/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Tests for the synced commits mapping.

#![deny(warnings)]

use anyhow::{bail, Error};
use ascii::AsciiString;
use bytes::Bytes;
use fbinit::FacebookInit;
use std::collections::BTreeMap;
use std::str::FromStr;
use std::sync::Arc;

use blobrepo::{save_bonsai_changesets, BlobRepo};
use blobrepo_factory;
use blobrepo_hg::BlobRepoHg;
use blobstore::{Loadable, Storable};
use bookmarks::{BookmarkName, BookmarkUpdateReason};
use context::CoreContext;
use cross_repo_sync_test_utils::rebase_root_on_master;
use fixtures::{linear, many_files_dirs};
use futures::compat::Future01CompatExt;
use live_commit_sync_config::TestLiveCommitSyncConfig;
use maplit::{btreemap, hashmap};
use mercurial_types::HgChangesetId;
use metaconfig_types::{
    CommitSyncConfig, CommitSyncConfigVersion, CommitSyncDirection,
    DefaultSmallToLargeCommitSyncPathAction, SmallRepoCommitSyncConfig,
};
use mononoke_types::{
    BlobstoreValue, BonsaiChangesetMut, ChangesetId, DateTime, FileChange, FileContents, FileType,
    MPath, RepositoryId,
};
use reachabilityindex::LeastCommonAncestorsHint;
use skiplist::SkiplistIndex;
use sql_construct::SqlConstruct;
use sql_ext::SqlConnections;
use synced_commit_mapping::{
    SqlSyncedCommitMapping, SyncedCommitMapping, SyncedCommitMappingEntry,
};

use cross_repo_sync::{
    get_plural_commit_sync_outcome,
    types::{Source, Target},
    CandidateSelectionHint, CommitSyncRepos, CommitSyncer, PluralCommitSyncOutcome,
};
use sql::rusqlite::Connection as SqliteConnection;

fn identity_renamer(b: &BookmarkName) -> Option<BookmarkName> {
    Some(b.clone())
}

fn mpath(p: &str) -> MPath {
    MPath::new(p).unwrap()
}

async fn move_bookmark(ctx: &CoreContext, repo: &BlobRepo, bookmark: &str, cs_id: ChangesetId) {
    let bookmark = BookmarkName::new(bookmark).unwrap();
    let mut txn = repo.update_bookmark_transaction(ctx.clone());
    txn.force_set(&bookmark, cs_id, BookmarkUpdateReason::TestMove, None)
        .unwrap();
    txn.commit().await.unwrap();
}

async fn get_bookmark(ctx: &CoreContext, repo: &BlobRepo, bookmark: &str) -> ChangesetId {
    let bookmark = BookmarkName::new(bookmark).unwrap();
    repo.get_bonsai_bookmark(ctx.clone(), &bookmark)
        .compat()
        .await
        .unwrap()
        .unwrap()
}

async fn create_initial_commit(ctx: CoreContext, repo: &BlobRepo) -> ChangesetId {
    let bookmark = BookmarkName::new("master").unwrap();

    let content = FileContents::new_bytes(Bytes::from(b"123" as &[u8]));
    let content_id = content
        .into_blob()
        .store(ctx.clone(), repo.blobstore())
        .await
        .unwrap();
    let file_change = FileChange::new(content_id, FileType::Regular, 3, None);

    let bcs = BonsaiChangesetMut {
        parents: vec![],
        author: "Test User <test@fb.com>".to_string(),
        author_date: DateTime::from_timestamp(1504040000, 0).unwrap(),
        committer: None,
        committer_date: None,
        message: "Initial commit to get going".to_string(),
        extra: btreemap! {},
        file_changes: btreemap! {mpath("master_file") => Some(file_change)},
    }
    .freeze()
    .unwrap();

    let bcs_id = bcs.get_changeset_id();
    save_bonsai_changesets(vec![bcs], ctx.clone(), repo.clone())
        .compat()
        .await
        .unwrap();

    let mut txn = repo.update_bookmark_transaction(ctx.clone());
    txn.force_set(&bookmark, bcs_id, BookmarkUpdateReason::TestMove, None)
        .unwrap();
    txn.commit().await.unwrap();
    bcs_id
}

async fn create_empty_commit(ctx: CoreContext, repo: &BlobRepo) -> ChangesetId {
    let bookmark = BookmarkName::new("master").unwrap();
    let p1 = repo
        .get_bonsai_bookmark(ctx.clone(), &bookmark)
        .compat()
        .await
        .unwrap()
        .unwrap();

    let bcs = BonsaiChangesetMut {
        parents: vec![p1],
        author: "Test User <test@fb.com>".to_string(),
        author_date: DateTime::from_timestamp(1504040001, 0).unwrap(),
        committer: None,
        committer_date: None,
        message: "Change master_file".to_string(),
        extra: btreemap! {},
        file_changes: btreemap! {},
    }
    .freeze()
    .unwrap();

    let bcs_id = bcs.get_changeset_id();
    save_bonsai_changesets(vec![bcs], ctx.clone(), repo.clone())
        .compat()
        .await
        .unwrap();

    let mut txn = repo.update_bookmark_transaction(ctx.clone());
    txn.force_set(&bookmark, bcs_id, BookmarkUpdateReason::TestMove, None)
        .unwrap();
    txn.commit().await.unwrap();
    bcs_id
}

async fn sync_to_master<M>(
    ctx: CoreContext,
    config: &CommitSyncer<M>,
    source_bcs_id: ChangesetId,
) -> Result<Option<ChangesetId>, Error>
where
    M: SyncedCommitMapping + Clone + 'static,
{
    let bookmark_name = BookmarkName::new("master").unwrap();
    let source_bcs = source_bcs_id
        .load(ctx.clone(), config.get_source_repo().blobstore())
        .await
        .unwrap();

    config
        .unsafe_sync_commit_pushrebase(
            ctx.clone(),
            source_bcs,
            bookmark_name,
            Target(Arc::new(SkiplistIndex::new())),
        )
        .await
}

async fn get_bcs_id<M>(
    ctx: CoreContext,
    config: &CommitSyncer<M>,
    source_hg_cs: HgChangesetId,
) -> ChangesetId
where
    M: SyncedCommitMapping + Clone + 'static,
{
    config
        .get_source_repo()
        .get_bonsai_from_hg(ctx, source_hg_cs)
        .compat()
        .await
        .unwrap()
        .unwrap()
}

async fn check_mapping<M>(
    ctx: CoreContext,
    config: &CommitSyncer<M>,
    source_bcs_id: ChangesetId,
    expected_bcs_id: Option<ChangesetId>,
) where
    M: SyncedCommitMapping + Clone + 'static,
{
    let source_repoid = config.get_source_repo().get_repoid();
    let destination_repoid = config.get_target_repo().get_repoid();
    let mapping = config.get_mapping();
    assert_eq!(
        mapping
            .get_one(
                ctx.clone(),
                source_repoid,
                source_bcs_id,
                destination_repoid,
            )
            .compat()
            .await
            .unwrap()
            .map(|(cs, _maybe_version)| cs),
        expected_bcs_id
    );

    if let Some(expected_bcs_id) = expected_bcs_id {
        assert_eq!(
            mapping
                .get_one(
                    ctx.clone(),
                    destination_repoid,
                    expected_bcs_id,
                    source_repoid
                )
                .compat()
                .await
                .unwrap()
                .map(|(cs, _maybe_version)| cs),
            Some(source_bcs_id)
        );
    }
}

fn create_commit_sync_config(
    small_repo_id: RepositoryId,
    large_repo_id: RepositoryId,
    prefix: &str,
) -> Result<CommitSyncConfig, Error> {
    let small_repo_config = SmallRepoCommitSyncConfig {
        default_action: DefaultSmallToLargeCommitSyncPathAction::PrependPrefix(MPath::new(prefix)?),
        map: hashmap! {},
        bookmark_prefix: AsciiString::new(),
        direction: CommitSyncDirection::LargeToSmall,
    };

    Ok(CommitSyncConfig {
        large_repo_id,
        common_pushrebase_bookmarks: vec![],
        small_repos: hashmap! {
            small_repo_id => small_repo_config,
        },
        version_name: CommitSyncConfigVersion("TEST_VERSION_NAME".to_string()),
    })
}

fn create_small_to_large_commit_syncer(
    small_repo: BlobRepo,
    large_repo: BlobRepo,
    prefix: &str,
    mapping: SqlSyncedCommitMapping,
) -> Result<CommitSyncer<SqlSyncedCommitMapping>, Error> {
    let small_repo_id = small_repo.get_repoid();
    let large_repo_id = large_repo.get_repoid();

    let commit_sync_config = create_commit_sync_config(small_repo_id, large_repo_id, prefix)?;
    let repos = CommitSyncRepos::new(small_repo, large_repo, &commit_sync_config)?;

    let (sync_config, source) = TestLiveCommitSyncConfig::new_with_source();
    source.set_commit_sync_config(small_repo_id, commit_sync_config.clone());
    source.set_commit_sync_config(large_repo_id, commit_sync_config);

    let live_commit_sync_config = Arc::new(sync_config);
    Ok(CommitSyncer::new(mapping, repos, live_commit_sync_config))
}

fn create_large_to_small_commit_syncer(
    small_repo: BlobRepo,
    large_repo: BlobRepo,
    prefix: &str,
    mapping: SqlSyncedCommitMapping,
) -> Result<CommitSyncer<SqlSyncedCommitMapping>, Error> {
    let small_repo_id = small_repo.get_repoid();
    let large_repo_id = large_repo.get_repoid();

    let commit_sync_config = create_commit_sync_config(small_repo_id, large_repo_id, prefix)?;
    let repos = CommitSyncRepos::new(large_repo, small_repo, &commit_sync_config)?;

    let (sync_config, source) = TestLiveCommitSyncConfig::new_with_source();
    source.set_commit_sync_config(small_repo_id, commit_sync_config.clone());
    source.set_commit_sync_config(large_repo_id, commit_sync_config);

    let live_commit_sync_config = Arc::new(sync_config);
    Ok(CommitSyncer::new(mapping, repos, live_commit_sync_config))
}

#[fbinit::compat_test]
async fn test_sync_parentage(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let (small_repo, megarepo, mapping) = prepare_repos_and_mapping()?;
    linear::initrepo(fb, &small_repo).await;
    let config =
        create_small_to_large_commit_syncer(small_repo, megarepo.clone(), "linear", mapping)?;
    create_initial_commit(ctx.clone(), &megarepo).await;

    // Take 2d7d4ba9ce0a6ffd222de7785b249ead9c51c536 from linear, and rewrite it as a child of master
    // As this is the first commit from linear, it'll rewrite cleanly
    let linear_base_bcs_id = get_bcs_id(
        ctx.clone(),
        &config,
        HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536")?,
    )
    .await;
    let expected_bcs_id =
        ChangesetId::from_str("8966842d2031e69108028d6f0ce5812bca28cae53679d066368a8c1472a5bb9a")
            .ok();

    let megarepo_base_bcs_id =
        rebase_root_on_master(ctx.clone(), &config, linear_base_bcs_id).await?;
    // Confirm that we got the expected conversion
    assert_eq!(Some(megarepo_base_bcs_id), expected_bcs_id);
    check_mapping(ctx.clone(), &config, linear_base_bcs_id, expected_bcs_id).await;

    // Finally, sync another commit
    let linear_second_bcs_id = get_bcs_id(
        ctx.clone(),
        &config,
        HgChangesetId::from_str("3e0e761030db6e479a7fb58b12881883f9f8c63f").unwrap(),
    )
    .await;
    let expected_bcs_id =
        ChangesetId::from_str("95c03dcd3324e172275ce22a5628d7a501aecb51d9a198b33284887769537acf")?;
    let megarepo_second_bcs_id = sync_to_master(ctx.clone(), &config, linear_second_bcs_id).await?;
    // Confirm that we got the expected conversion
    assert_eq!(megarepo_second_bcs_id, Some(expected_bcs_id));
    // And check that the synced commit has correct parentage
    assert_eq!(
        megarepo
            .get_changeset_parents_by_bonsai(ctx.clone(), megarepo_second_bcs_id.unwrap())
            .compat()
            .await?,
        vec![megarepo_base_bcs_id]
    );

    Ok(())
}

async fn create_commit_from_parent_and_changes<'a>(
    ctx: &'a CoreContext,
    repo: &'a BlobRepo,
    p1: ChangesetId,
    changes: BTreeMap<&'static str, Option<&'static str>>,
) -> ChangesetId {
    let mut proper_changes: BTreeMap<MPath, Option<FileChange>> = BTreeMap::new();
    for (path, maybe_content) in changes.into_iter() {
        let mpath = MPath::new(path).unwrap();
        match maybe_content {
            None => {
                proper_changes.insert(mpath, None);
            }
            Some(content) => {
                let file_contents = FileContents::new_bytes(content.as_bytes());
                let content_id = file_contents
                    .into_blob()
                    .store(ctx.clone(), repo.blobstore())
                    .await
                    .unwrap();
                let file_change =
                    FileChange::new(content_id, FileType::Regular, content.len() as u64, None);

                proper_changes.insert(mpath, Some(file_change));
            }
        }
    }

    let bcs = BonsaiChangesetMut {
        parents: vec![p1],
        author: "Test User <test@fb.com>".to_string(),
        author_date: DateTime::from_timestamp(1504040001, 0).unwrap(),
        committer: None,
        committer_date: None,
        message: "ababagalamaga".to_string(),
        extra: btreemap! {},
        file_changes: proper_changes,
    }
    .freeze()
    .unwrap();

    let bcs_id = bcs.get_changeset_id();
    save_bonsai_changesets(vec![bcs], ctx.clone(), repo.clone())
        .compat()
        .await
        .unwrap();

    bcs_id
}

async fn update_master_file(ctx: CoreContext, repo: &BlobRepo) -> ChangesetId {
    let bookmark = BookmarkName::new("master").unwrap();
    let p1 = repo
        .get_bonsai_bookmark(ctx.clone(), &bookmark)
        .compat()
        .await
        .unwrap()
        .unwrap();

    let content = FileContents::new_bytes(Bytes::from(b"456" as &[u8]));
    let content_id = content
        .into_blob()
        .store(ctx.clone(), repo.blobstore())
        .await
        .unwrap();
    let file_change = FileChange::new(content_id, FileType::Regular, 3, None);

    let bcs = BonsaiChangesetMut {
        parents: vec![p1],
        author: "Test User <test@fb.com>".to_string(),
        author_date: DateTime::from_timestamp(1504040001, 0).unwrap(),
        committer: None,
        committer_date: None,
        message: "Change master_file".to_string(),
        extra: btreemap! {},
        file_changes: btreemap! {mpath("master_file") => Some(file_change)},
    }
    .freeze()
    .unwrap();

    let bcs_id = bcs.get_changeset_id();
    save_bonsai_changesets(vec![bcs], ctx.clone(), repo.clone())
        .compat()
        .await
        .unwrap();

    let mut txn = repo.update_bookmark_transaction(ctx.clone());
    txn.force_set(&bookmark, bcs_id, BookmarkUpdateReason::TestMove, None)
        .unwrap();
    txn.commit().await.unwrap();
    bcs_id
}

#[fbinit::compat_test]
async fn test_sync_causes_conflict(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let megarepo = blobrepo_factory::new_memblob_empty_with_id(None, RepositoryId::new(1))?;

    let mapping = SqlSyncedCommitMapping::with_sqlite_in_memory()?;
    let linear = linear::getrepo(fb).await;
    let linear_config = create_small_to_large_commit_syncer(
        linear.clone(),
        megarepo.clone(),
        "linear",
        mapping.clone(),
    )?;

    let master_file_config =
        create_small_to_large_commit_syncer(linear, megarepo.clone(), "master_file", mapping)?;

    create_initial_commit(ctx.clone(), &megarepo).await;

    // Take 2d7d4ba9ce0a6ffd222de7785b249ead9c51c536 from linear, and rewrite it as a child of master
    let linear_base_bcs_id = get_bcs_id(
        ctx.clone(),
        &linear_config,
        HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536")?,
    )
    .await;
    rebase_root_on_master(ctx.clone(), &linear_config, linear_base_bcs_id).await?;

    // Change master_file
    update_master_file(ctx.clone(), &megarepo).await;

    // Finally, sync another commit over master_file - this should fail
    let linear_second_bcs_id = get_bcs_id(
        ctx.clone(),
        &master_file_config,
        HgChangesetId::from_str("3e0e761030db6e479a7fb58b12881883f9f8c63f")?,
    )
    .await;
    let megarepo_fail_bcs_id =
        sync_to_master(ctx.clone(), &master_file_config, linear_second_bcs_id).await;
    // Confirm the syncing failed
    assert!(
        megarepo_fail_bcs_id.is_err(),
        format!("{:?}", megarepo_fail_bcs_id)
    );

    check_mapping(ctx.clone(), &master_file_config, linear_second_bcs_id, None).await;
    Ok(())
}

fn prepare_repos_and_mapping() -> Result<(BlobRepo, BlobRepo, SqlSyncedCommitMapping), Error> {
    let sqlite_con = SqliteConnection::open_in_memory()?;
    sqlite_con.execute_batch(SqlSyncedCommitMapping::CREATION_QUERY)?;
    let (megarepo, con) = blobrepo_factory::new_memblob_with_sqlite_connection_with_id(
        sqlite_con,
        RepositoryId::new(1),
    )?;

    let (small_repo, _) =
        blobrepo_factory::new_memblob_with_connection_with_id(con.clone(), RepositoryId::new(0))?;
    let mapping = SqlSyncedCommitMapping::from_sql_connections(SqlConnections::new_single(con));
    Ok((small_repo, megarepo, mapping))
}

#[fbinit::compat_test]
async fn test_sync_empty_commit(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let (small_repo, megarepo, mapping) = prepare_repos_and_mapping()?;
    linear::initrepo(fb, &small_repo).await;
    let linear = small_repo;

    let stl_config = create_small_to_large_commit_syncer(
        linear.clone(),
        megarepo.clone(),
        "linear",
        mapping.clone(),
    )?;
    let lts_config = create_large_to_small_commit_syncer(
        linear.clone(),
        megarepo.clone(),
        "linear",
        mapping.clone(),
    )?;

    create_initial_commit(ctx.clone(), &megarepo).await;

    // Take 2d7d4ba9ce0a6ffd222de7785b249ead9c51c536 from linear, and rewrite it as a child of master
    // As this is the first commit from linear, it'll rewrite cleanly
    let linear_base_bcs_id = get_bcs_id(
        ctx.clone(),
        &stl_config,
        HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536")?,
    )
    .await;
    rebase_root_on_master(ctx.clone(), &stl_config, linear_base_bcs_id).await?;

    // Sync an empty commit back to linear
    let megarepo_empty_bcs_id = create_empty_commit(ctx.clone(), &megarepo).await;
    let linear_empty_bcs_id =
        sync_to_master(ctx.clone(), &lts_config, megarepo_empty_bcs_id).await?;

    let expected_bcs_id =
        ChangesetId::from_str("dad900d07c885c21d4361a11590c220cc65c287d52fe1e0f4df61242c7c03f07")
            .ok();
    assert_eq!(linear_empty_bcs_id, expected_bcs_id);
    check_mapping(
        ctx.clone(),
        &lts_config,
        megarepo_empty_bcs_id,
        linear_empty_bcs_id,
    )
    .await;

    Ok(())
}

async fn megarepo_copy_file(
    ctx: CoreContext,
    repo: &BlobRepo,
    linear_bcs_id: ChangesetId,
) -> ChangesetId {
    let bookmark = BookmarkName::new("master").unwrap();
    let p1 = repo
        .get_bonsai_bookmark(ctx.clone(), &bookmark)
        .compat()
        .await
        .unwrap()
        .unwrap();

    let content = FileContents::new_bytes(Bytes::from(b"99\n" as &[u8]));
    let content_id = content
        .into_blob()
        .store(ctx.clone(), repo.blobstore())
        .await
        .unwrap();
    let file_change = FileChange::new(
        content_id,
        FileType::Regular,
        3,
        Some((MPath::new(b"linear/1").unwrap(), linear_bcs_id)),
    );

    let bcs = BonsaiChangesetMut {
        parents: vec![p1],
        author: "Test User <test@fb.com>".to_string(),
        author_date: DateTime::from_timestamp(1504040055, 0).unwrap(),
        committer: None,
        committer_date: None,
        message: "Change 1".to_string(),
        extra: btreemap! {},
        file_changes: btreemap! {mpath("linear/new_file") => Some(file_change)},
    }
    .freeze()
    .unwrap();

    let bcs_id = bcs.get_changeset_id();
    save_bonsai_changesets(vec![bcs], ctx.clone(), repo.clone())
        .compat()
        .await
        .unwrap();

    let mut txn = repo.update_bookmark_transaction(ctx.clone());
    txn.force_set(&bookmark, bcs_id, BookmarkUpdateReason::TestMove, None)
        .unwrap();
    txn.commit().await.unwrap();
    bcs_id
}

#[fbinit::compat_test]
async fn test_sync_copyinfo(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let (small_repo, megarepo, mapping) = prepare_repos_and_mapping().unwrap();
    linear::initrepo(fb, &small_repo).await;
    let linear = small_repo;

    let stl_config = create_small_to_large_commit_syncer(
        linear.clone(),
        megarepo.clone(),
        "linear",
        mapping.clone(),
    )?;
    let lts_config =
        create_large_to_small_commit_syncer(linear.clone(), megarepo.clone(), "linear", mapping)?;

    create_initial_commit(ctx.clone(), &megarepo).await;

    // Take 2d7d4ba9ce0a6ffd222de7785b249ead9c51c536 from linear, and rewrite it as a child of master
    // As this is the first commit from linear, it'll rewrite cleanly
    let linear_base_bcs_id = get_bcs_id(
        ctx.clone(),
        &stl_config,
        HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536")?,
    )
    .await;
    let megarepo_linear_base_bcs_id =
        rebase_root_on_master(ctx.clone(), &stl_config, linear_base_bcs_id).await?;

    // Fetch master from linear - the pushrebase in a remap will change copyinfo
    let linear_master_bcs_id = {
        let bookmark = BookmarkName::new("master").unwrap();
        linear
            .get_bonsai_bookmark(ctx.clone(), &bookmark)
            .compat()
            .await?
            .unwrap()
    };

    let megarepo_copyinfo_commit =
        megarepo_copy_file(ctx.clone(), &megarepo, megarepo_linear_base_bcs_id).await;
    let linear_copyinfo_bcs_id =
        sync_to_master(ctx.clone(), &lts_config, megarepo_copyinfo_commit).await?;

    let expected_bcs_id =
        ChangesetId::from_str("68e495f850e16cd4a6b372d27f18f59931139242b5097c137afa1d738769cc60")
            .ok();
    assert_eq!(linear_copyinfo_bcs_id, expected_bcs_id);
    check_mapping(
        ctx.clone(),
        &lts_config,
        megarepo_copyinfo_commit,
        linear_copyinfo_bcs_id,
    )
    .await;

    // Fetch commit from linear by its new ID, and confirm that it has the correct copyinfo
    let linear_bcs = linear_copyinfo_bcs_id
        .unwrap()
        .load(ctx.clone(), linear.blobstore())
        .await?;

    let file_changes: Vec<_> = linear_bcs.file_changes().collect();
    assert!(file_changes.len() == 1, "Wrong file change count");
    let (path, copy_info) = file_changes.first().unwrap();
    assert_eq!(**path, mpath("new_file"));
    let (copy_source, copy_bcs) = copy_info.unwrap().copy_from().unwrap();
    assert_eq!(*copy_source, mpath("1"));
    assert_eq!(*copy_bcs, linear_master_bcs_id);

    Ok(())
}

#[fbinit::compat_test]
async fn test_sync_remap_failure(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let megarepo = blobrepo_factory::new_memblob_empty_with_id(None, RepositoryId::new(1))?;
    let linear = linear::getrepo(fb).await;
    let mapping = SqlSyncedCommitMapping::with_sqlite_in_memory()?;

    let mut fail_config = create_large_to_small_commit_syncer(
        linear.clone(),
        megarepo.clone(),
        // This is ignored
        "linear",
        mapping.clone(),
    )?;
    let fail_repos = CommitSyncRepos::LargeToSmall {
        small_repo: linear.clone(),
        large_repo: megarepo.clone(),
        mover: Arc::new(move |_path: &MPath| bail!("This always fails")),
        reverse_mover: Arc::new(move |_path: &MPath| bail!("This always fails")),
        bookmark_renamer: Arc::new(identity_renamer),
        reverse_bookmark_renamer: Arc::new(identity_renamer),
        version_name: CommitSyncConfigVersion("TEST_VERSION_NAME".to_string()),
    };
    fail_config.repos = fail_repos;

    let stl_config = create_small_to_large_commit_syncer(
        linear.clone(),
        megarepo.clone(),
        "linear",
        mapping.clone(),
    )?;

    let mut copyfrom_fail_config = create_large_to_small_commit_syncer(
        linear.clone(),
        megarepo.clone(),
        // This is ignored
        "linear",
        mapping.clone(),
    )?;
    let linear_path_in_megarepo = mpath("linear");
    let copyfrom_fail_repos = CommitSyncRepos::LargeToSmall {
        small_repo: linear.clone(),
        large_repo: megarepo.clone(),
        mover: Arc::new(move |path: &MPath| {
            match path.basename().as_ref() {
                b"1" => bail!("This only fails if the file is named '1'"),
                _ => Ok(path.remove_prefix_component(&linear_path_in_megarepo)),
            }
        }),
        reverse_mover: Arc::new(move |path: &MPath| {
            match path.basename().as_ref() {
                b"1" => bail!("This only fails if the file is named '1'"),
                _ => Ok(Some(mpath("linear").join(path))),
            }
        }),
        bookmark_renamer: Arc::new(identity_renamer),
        reverse_bookmark_renamer: Arc::new(identity_renamer),
        version_name: CommitSyncConfigVersion("TEST_VERSION_NAME".to_string()),
    };
    copyfrom_fail_config.repos = copyfrom_fail_repos;

    create_initial_commit(ctx.clone(), &megarepo).await;

    // Take 2d7d4ba9ce0a6ffd222de7785b249ead9c51c536 from linear, and rewrite it as a child of master
    // As this is the first commit from linear, it'll rewrite cleanly
    let linear_base_bcs_id = get_bcs_id(
        ctx.clone(),
        &stl_config,
        HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536").unwrap(),
    )
    .await;
    let megarepo_linear_base_bcs_id =
        rebase_root_on_master(ctx.clone(), &stl_config, linear_base_bcs_id).await?;

    let megarepo_copyinfo_commit =
        megarepo_copy_file(ctx.clone(), &megarepo, megarepo_linear_base_bcs_id).await;

    let always_fail = sync_to_master(ctx.clone(), &fail_config, megarepo_copyinfo_commit).await;
    assert!(always_fail.is_err());

    let copyfrom_fail =
        sync_to_master(ctx.clone(), &copyfrom_fail_config, megarepo_copyinfo_commit).await;
    assert!(copyfrom_fail.is_err(), "{:#?}", copyfrom_fail);

    Ok(())
}

fn maybe_replace_prefix(
    path: &MPath,
    potential_prefix: &MPath,
    replacement: &MPath,
) -> Option<MPath> {
    if potential_prefix.is_prefix_of(path) {
        let elements: Vec<_> = path
            .into_iter()
            .skip(potential_prefix.num_components())
            .collect();
        Some(replacement.join(elements))
    } else {
        None
    }
}

#[fbinit::compat_test]
async fn test_sync_implicit_deletes(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let (small_repo, megarepo, mapping) = prepare_repos_and_mapping().unwrap();
    many_files_dirs::initrepo(fb, &small_repo).await;
    let repo = small_repo;

    let mut commit_syncer = create_small_to_large_commit_syncer(
        repo.clone(),
        megarepo.clone(),
        "linear",
        mapping.clone(),
    )?;

    // Note: this mover relies on non-prefix-free path map, which may
    // or may not be allowed in repo configs. We want commit syncing to work
    // in this case, regardless of whether such config is allowed
    let mover = Arc::new(move |path: &MPath| -> Result<Option<MPath>, Error> {
        let longer_path = mpath("dir1/subdir1/subsubdir1");
        let prefix1: MPath = mpath("prefix1");
        let shorter_path = mpath("dir1");
        let prefix2: MPath = mpath("prefix2");
        if let Some(changed_path) = maybe_replace_prefix(path, &longer_path, &prefix1) {
            return Ok(Some(changed_path));
        }
        if let Some(changed_path) = maybe_replace_prefix(path, &shorter_path, &prefix2) {
            return Ok(Some(changed_path));
        }
        Ok(Some(path.clone()))
    });

    let reverse_mover = Arc::new(move |path: &MPath| -> Result<Option<MPath>, Error> {
        let longer_path = mpath("dir1/subdir1/subsubdir1");
        let prefix1: MPath = mpath("prefix1");
        let shorter_path = mpath("dir1");
        let prefix2: MPath = mpath("prefix2");

        if let Some(changed_path) = maybe_replace_prefix(path, &prefix1, &longer_path) {
            return Ok(Some(changed_path));
        }
        if let Some(changed_path) = maybe_replace_prefix(path, &prefix2, &shorter_path) {
            return Ok(Some(changed_path));
        }
        Ok(Some(path.clone()))
    });

    let commit_sync_repos = CommitSyncRepos::SmallToLarge {
        small_repo: repo.clone(),
        large_repo: megarepo.clone(),
        mover,
        reverse_mover,
        bookmark_renamer: Arc::new(identity_renamer),
        reverse_bookmark_renamer: Arc::new(identity_renamer),
        version_name: CommitSyncConfigVersion("TEST_VERSION_NAME".to_string()),
    };
    commit_syncer.repos = commit_sync_repos;

    let megarepo_initial_bcs_id = create_initial_commit(ctx.clone(), &megarepo).await;

    // Insert a fake mapping entry, so that syncs succeed
    let repo_initial_bcs_id = get_bcs_id(
        ctx.clone(),
        &commit_syncer,
        HgChangesetId::from_str("2f866e7e549760934e31bf0420a873f65100ad63").unwrap(),
    )
    .await;
    let entry = SyncedCommitMappingEntry::new(
        megarepo.get_repoid(),
        megarepo_initial_bcs_id,
        repo.get_repoid(),
        repo_initial_bcs_id,
        None,
    );
    mapping.add(ctx.clone(), entry).compat().await?;

    // d261bc7900818dea7c86935b3fb17a33b2e3a6b4 from "many_files_dirs" should sync cleanly
    // on top of master. Among others, it introduces the following files:
    // - "dir1/subdir1/subsubdir1/file_1"
    // - "dir1/subdir1/subsubdir2/file_1"
    // - "dir1/subdir1/subsubdir2/file_2"
    let repo_base_bcs_id = get_bcs_id(
        ctx.clone(),
        &commit_syncer,
        HgChangesetId::from_str("d261bc7900818dea7c86935b3fb17a33b2e3a6b4").unwrap(),
    )
    .await;

    sync_to_master(ctx.clone(), &commit_syncer, repo_base_bcs_id)
        .await?
        .expect("Unexpectedly rewritten into nothingness");

    // 051946ed218061e925fb120dac02634f9ad40ae2 from "many_files_dirs" replaces the
    // entire "dir1" directory with a file, which implicitly deletes
    // "dir1/subdir1/subsubdir1" and "dir1/subdir1/subsubdir2".
    let repo_implicit_delete_bcs_id = get_bcs_id(
        ctx.clone(),
        &commit_syncer,
        HgChangesetId::from_str("051946ed218061e925fb120dac02634f9ad40ae2").unwrap(),
    )
    .await;
    let megarepo_implicit_delete_bcs_id =
        sync_to_master(ctx.clone(), &commit_syncer, repo_implicit_delete_bcs_id)
            .await?
            .expect("Unexpectedly rewritten into nothingness");

    let megarepo_implicit_delete_bcs = megarepo_implicit_delete_bcs_id
        .load(ctx.clone(), megarepo.blobstore())
        .await?;
    let file_changes: BTreeMap<MPath, _> = megarepo_implicit_delete_bcs
        .file_changes()
        .map(|(a, b)| (a.clone(), b.clone()))
        .collect();

    // "dir1" was rewrtitten as "prefix2" and explicitly replaced with file, so the file
    // change should be `Some`
    assert!(file_changes[&mpath("prefix2")].is_some());
    // "dir1/subdir1/subsubdir1/file_1" was rewritten as "prefix1/file_1", and became
    // an implicit delete
    assert!(file_changes[&mpath("prefix1/file_1")].is_none());
    // there are no other entries in `file_changes` as other implicit deletes where
    // removed by the minimization process
    assert_eq!(file_changes.len(), 2);

    Ok(())
}

async fn update_linear_1_file(ctx: CoreContext, repo: &BlobRepo) -> ChangesetId {
    let bookmark = BookmarkName::new("master").unwrap();
    let p1 = repo
        .get_bonsai_bookmark(ctx.clone(), &bookmark)
        .compat()
        .await
        .unwrap()
        .unwrap();

    let content = FileContents::new_bytes(Bytes::from(b"999" as &[u8]));
    let content_id = content
        .into_blob()
        .store(ctx.clone(), repo.blobstore())
        .await
        .unwrap();
    let file_change = FileChange::new(content_id, FileType::Regular, 3, None);

    let bcs = BonsaiChangesetMut {
        parents: vec![p1],
        author: "Test User <test@fb.com>".to_string(),
        author_date: DateTime::from_timestamp(1504040002, 0).unwrap(),
        committer: None,
        committer_date: None,
        message: "Change linear/1".to_string(),
        extra: btreemap! {},
        file_changes: btreemap! {mpath("linear/1") => Some(file_change)},
    }
    .freeze()
    .unwrap();

    let bcs_id = bcs.get_changeset_id();
    save_bonsai_changesets(vec![bcs], ctx.clone(), repo.clone())
        .compat()
        .await
        .unwrap();

    let mut txn = repo.update_bookmark_transaction(ctx.clone());
    txn.force_set(&bookmark, bcs_id, BookmarkUpdateReason::TestMove, None)
        .unwrap();
    txn.commit().await.unwrap();

    bcs_id
}

#[fbinit::compat_test]
async fn test_sync_parent_search(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let (small_repo, megarepo, mapping) = prepare_repos_and_mapping()?;
    linear::initrepo(fb, &small_repo).await;
    let linear = small_repo;

    let config = create_small_to_large_commit_syncer(
        linear.clone(),
        megarepo.clone(),
        "linear",
        mapping.clone(),
    )?;
    let reverse_config =
        create_large_to_small_commit_syncer(linear.clone(), megarepo.clone(), "linear", mapping)?;

    create_initial_commit(ctx.clone(), &megarepo).await;

    // Take 2d7d4ba9ce0a6ffd222de7785b249ead9c51c536 from linear, and rewrite it as a child of master
    let linear_base_bcs_id = get_bcs_id(
        ctx.clone(),
        &config,
        HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536").unwrap(),
    )
    .await;
    rebase_root_on_master(ctx.clone(), &config, linear_base_bcs_id).await?;

    // Change master_file
    let master_file_cs_id = update_master_file(ctx.clone(), &megarepo).await;
    sync_to_master(ctx.clone(), &reverse_config, master_file_cs_id).await?;
    // And change a file in linear
    let new_commit = update_linear_1_file(ctx.clone(), &megarepo).await;

    // Now sync it back to linear
    let sync_success_bcs_id = sync_to_master(ctx.clone(), &reverse_config, new_commit).await?;

    // Confirm the syncing succeeded
    let expected_bcs_id =
        ChangesetId::from_str("9cd24566d5a4b7e7b4cc3a62b412d58ea60804434255863d3bfac1282a2d44fa")
            .ok();
    assert_eq!(sync_success_bcs_id, expected_bcs_id);

    check_mapping(
        ctx.clone(),
        &reverse_config,
        new_commit,
        sync_success_bcs_id,
    )
    .await;
    // And validate that the mapping is correct when looked at the other way round
    check_mapping(
        ctx.clone(),
        &config,
        sync_success_bcs_id.unwrap(),
        Some(new_commit),
    )
    .await;

    Ok(())
}

async fn check_rewritten_multiple(
    ctx: &CoreContext,
    source_repo: &BlobRepo,
    target_repo: &BlobRepo,
    mapping: &SqlSyncedCommitMapping,
    cs_id: ChangesetId,
    expected_rewrite_count: usize,
) -> Result<(), Error> {
    let plural_commit_sync_outcome = get_plural_commit_sync_outcome(
        ctx,
        Source(source_repo.get_repoid()),
        Target(target_repo.get_repoid()),
        Source(cs_id),
        mapping,
    )
    .await?
    .expect("should've been remapped");
    if let PluralCommitSyncOutcome::RewrittenAs(v) = plural_commit_sync_outcome {
        assert_eq!(v.len(), expected_rewrite_count);
    } else {
        panic!(
            "incorrect remapping of {}: {:?}",
            cs_id, plural_commit_sync_outcome
        );
    }

    Ok(())
}

/// Prepare two repos with small repo master remapping to
/// two commits in the large repo:
/// ```text
/// master     master
///   | other   |
///   | branch  |
///   |  |      |
///   D'.D''....D
///   |  |      |
///   B  C      |
///   | /       |
///   A'........A
///   |         |
/// LARGE      SMALL
/// ```
/// (horizontal dots represent `RewrittenAs` relationship)
async fn get_multiple_master_mapping_setup(
    fb: FacebookInit,
) -> Result<
    (
        CoreContext,
        BlobRepo,
        BlobRepo,
        ChangesetId,
        ChangesetId,
        CommitSyncer<SqlSyncedCommitMapping>,
    ),
    Error,
> {
    let ctx = CoreContext::test_mock(fb);
    let (small_repo, megarepo, mapping) = prepare_repos_and_mapping().unwrap();
    linear::initrepo(fb, &small_repo).await;
    let small_to_large_syncer = create_small_to_large_commit_syncer(
        small_repo.clone(),
        megarepo.clone(),
        "prefix",
        mapping.clone(),
    )?;

    create_initial_commit(ctx.clone(), &megarepo).await;

    let megarepo_lca_hint: Arc<dyn LeastCommonAncestorsHint> = Arc::new(SkiplistIndex::new());
    let megarepo_master_cs_id = get_bookmark(&ctx, &megarepo, "master").await;
    let small_repo_master_cs_id = get_bookmark(&ctx, &small_repo, "master").await;
    // Masters map to each other before we even do any syncs
    mapping
        .add(
            ctx.clone(),
            SyncedCommitMappingEntry::new(
                megarepo.get_repoid(),
                megarepo_master_cs_id,
                small_repo.get_repoid(),
                small_repo_master_cs_id,
                None,
            ),
        )
        .compat()
        .await?;

    // 1. Create two commits in megarepo, on separate branches,
    // neither touching small repo files.
    let b1 = create_commit_from_parent_and_changes(
        &ctx,
        &megarepo,
        megarepo_master_cs_id,
        btreemap! {"unrelated_1" => Some("unrelated")},
    )
    .await;
    let b2 = create_commit_from_parent_and_changes(
        &ctx,
        &megarepo,
        megarepo_master_cs_id,
        btreemap! {"unrelated_2" => Some("unrelated")},
    )
    .await;

    move_bookmark(&ctx, &megarepo, "other_branch", b2).await;
    move_bookmark(&ctx, &megarepo, "master", b1).await;

    // 2. Create a small repo commit and sync it onto both branches
    let small_repo_master_cs_id = create_commit_from_parent_and_changes(
        &ctx,
        &small_repo,
        small_repo_master_cs_id,
        btreemap! {"small_repo_file" => Some("content")},
    )
    .await;
    move_bookmark(&ctx, &small_repo, "master", small_repo_master_cs_id).await;

    let small_cs = small_repo_master_cs_id
        .load(ctx.clone(), small_repo.blobstore())
        .await?;
    small_to_large_syncer
        .unsafe_sync_commit_pushrebase(
            ctx.clone(),
            small_cs.clone(),
            BookmarkName::new("master").unwrap(),
            Target(megarepo_lca_hint.clone()),
        )
        .await
        .expect("sync should have succeeded");

    small_to_large_syncer
        .unsafe_sync_commit_pushrebase(
            ctx.clone(),
            small_cs.clone(),
            BookmarkName::new("other_branch").unwrap(),
            Target(megarepo_lca_hint.clone()),
        )
        .await
        .expect("sync should have succeeded");

    // 3. Sanity-check that the small repo master is indeed rewritten
    // into two different commits in the large repo
    check_rewritten_multiple(
        &ctx,
        &small_repo,
        &megarepo,
        &mapping,
        small_repo_master_cs_id,
        2,
    )
    .await?;

    // Re-query megarepo master bookmark, as its localtion has changed due
    // to a cross-repo sync
    let megarepo_master_cs_id = get_bookmark(&ctx, &megarepo, "master").await;
    Ok((
        ctx,
        small_repo,
        megarepo,
        megarepo_master_cs_id,
        small_repo_master_cs_id,
        small_to_large_syncer,
    ))
}

#[fbinit::compat_test]
async fn test_sync_parent_has_multiple_mappings(fb: FacebookInit) -> Result<(), Error> {
    let (
        ctx,
        small_repo,
        megarepo,
        _megarepo_master_cs_id,
        small_repo_master_cs_id,
        small_to_large_syncer,
    ) = get_multiple_master_mapping_setup(fb).await?;

    // Create a small repo commit on top of master
    let to_sync = create_commit_from_parent_and_changes(
        &ctx,
        &small_repo,
        small_repo_master_cs_id,
        btreemap! {"foo" => Some("bar")},
    )
    .await;

    // Cannot sync without a hint
    let e = small_to_large_syncer
        .unsafe_sync_commit(ctx.clone(), to_sync, CandidateSelectionHint::Only)
        .await
        .expect_err("sync should have failed");
    assert!(format!("{:?}", e).contains("Too many rewritten candidates for"));


    // Can sync with a bookmark-based hint
    let book = Target(BookmarkName::new("master").unwrap());
    let lca_hint: Target<Arc<dyn LeastCommonAncestorsHint>> =
        Target(Arc::new(SkiplistIndex::new()));
    small_to_large_syncer
        .unsafe_sync_commit(
            ctx.clone(),
            to_sync,
            CandidateSelectionHint::OnlyOrAncestorOfBookmark(
                book,
                Target(megarepo.clone()),
                lca_hint,
            ),
        )
        .await
        .expect("sync should have succeeded");

    Ok(())
}

#[fbinit::compat_test]
async fn test_sync_no_op_pushrebase_has_multiple_mappings(fb: FacebookInit) -> Result<(), Error> {
    let (
        ctx,
        small_repo,
        _megarepo,
        _megarepo_master_cs_id,
        small_repo_master_cs_id,
        small_to_large_syncer,
    ) = get_multiple_master_mapping_setup(fb).await?;

    // Create a small repo commit on top of master
    let to_sync_id = create_commit_from_parent_and_changes(
        &ctx,
        &small_repo,
        small_repo_master_cs_id,
        btreemap! {"foo" => Some("bar")},
    )
    .await;
    let to_sync = to_sync_id.load(ctx.clone(), small_repo.blobstore()).await?;

    let lca_hint: Target<Arc<dyn LeastCommonAncestorsHint>> =
        Target(Arc::new(SkiplistIndex::new()));
    small_to_large_syncer
        .unsafe_sync_commit_pushrebase(
            ctx.clone(),
            to_sync,
            BookmarkName::new("master").unwrap(),
            lca_hint,
        )
        .await
        .expect("sync should have succeeded");

    Ok(())
}

#[fbinit::compat_test]
async fn test_sync_real_pushrebase_has_multiple_mappings(fb: FacebookInit) -> Result<(), Error> {
    let (
        ctx,
        small_repo,
        megarepo,
        megarepo_master_cs_id,
        small_repo_master_cs_id,
        small_to_large_syncer,
    ) = get_multiple_master_mapping_setup(fb).await?;

    // Advance megarepo master
    let cs_id = create_commit_from_parent_and_changes(
        &ctx,
        &megarepo,
        megarepo_master_cs_id,
        btreemap! {"unrelated_3" => Some("unrelated")},
    )
    .await;
    move_bookmark(&ctx, &megarepo, "master", cs_id).await;

    // Create a small repo commit on top of master
    let to_sync_id = create_commit_from_parent_and_changes(
        &ctx,
        &small_repo,
        small_repo_master_cs_id,
        btreemap! {"foo" => Some("bar")},
    )
    .await;
    let to_sync = to_sync_id.load(ctx.clone(), small_repo.blobstore()).await?;

    let lca_hint: Target<Arc<dyn LeastCommonAncestorsHint>> =
        Target(Arc::new(SkiplistIndex::new()));
    small_to_large_syncer
        .unsafe_sync_commit_pushrebase(
            ctx.clone(),
            to_sync,
            BookmarkName::new("master").unwrap(),
            lca_hint,
        )
        .await
        .expect("sync should have succeeded");

    Ok(())
}
