/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

//! Tests for the synced commits mapping.

#![deny(warnings)]

use async_unit;
use bytes::Bytes;
use fbinit::FacebookInit;
use futures::Future;
use maplit::btreemap;
use std::str::FromStr;
use std::sync::Arc;

use blobrepo::{save_bonsai_changesets, BlobRepo};
use blobrepo_factory;
use blobstore::Storable;
use bookmarks::{BookmarkName, BookmarkUpdateReason};
use context::CoreContext;
use failure_ext::{err_msg, Error};
use fixtures::linear;
use mercurial_types::HgChangesetId;
use mononoke_types::{
    BlobstoreValue, BonsaiChangesetMut, ChangesetId, DateTime, FileChange, FileContents, FileType,
    MPath, RepositoryId,
};
use synced_commit_mapping::{SqlConstructors, SqlSyncedCommitMapping, SyncedCommitMapping};

use cross_repo_sync::{sync_commit_compat, CommitSyncConfig, CommitSyncRepos};

fn create_initial_commit(ctx: CoreContext, repo: &BlobRepo) -> ChangesetId {
    let bookmark = BookmarkName::new("master").unwrap();

    let content = FileContents::new_bytes(Bytes::from(b"123" as &[u8]));
    let content_id = content
        .into_blob()
        .store(ctx.clone(), &repo.get_blobstore())
        .wait()
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
        file_changes: btreemap! {MPath::new("master_file").unwrap() => Some(file_change)},
    }
    .freeze()
    .unwrap();

    let bcs_id = bcs.get_changeset_id();
    save_bonsai_changesets(vec![bcs], ctx.clone(), repo.clone())
        .wait()
        .unwrap();

    let mut txn = repo.update_bookmark_transaction(ctx.clone());
    txn.force_set(
        &bookmark,
        bcs_id,
        BookmarkUpdateReason::TestMove {
            bundle_replay_data: None,
        },
    )
    .unwrap();
    txn.commit().wait().unwrap();
    bcs_id
}

fn create_empty_commit(ctx: CoreContext, repo: &BlobRepo) -> ChangesetId {
    let bookmark = BookmarkName::new("master").unwrap();
    let p1 = repo
        .get_bonsai_bookmark(ctx.clone(), &bookmark)
        .wait()
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
        .wait()
        .unwrap();

    let mut txn = repo.update_bookmark_transaction(ctx.clone());
    txn.force_set(
        &bookmark,
        bcs_id,
        BookmarkUpdateReason::TestMove {
            bundle_replay_data: None,
        },
    )
    .unwrap();
    txn.commit().wait().unwrap();
    bcs_id
}

fn sync_to_master<M>(
    ctx: CoreContext,
    config: &CommitSyncConfig<M>,
    source_bcs_id: ChangesetId,
) -> Result<Option<ChangesetId>, Error>
where
    M: SyncedCommitMapping + Clone + 'static,
{
    let bookmark_name = BookmarkName::new("master").unwrap();
    let source = config.get_source_repo();

    let source_bcs = source
        .get_bonsai_changeset(ctx.clone(), source_bcs_id)
        .wait()
        .unwrap();

    sync_commit_compat(ctx, source_bcs, config.clone(), bookmark_name).wait()
}

fn get_bcs_id<M>(
    ctx: CoreContext,
    config: &CommitSyncConfig<M>,
    source_hg_cs: HgChangesetId,
) -> ChangesetId
where
    M: SyncedCommitMapping + Clone + 'static,
{
    config
        .get_source_repo()
        .get_bonsai_from_hg(ctx, source_hg_cs)
        .wait()
        .unwrap()
        .unwrap()
}

fn check_mapping<M>(
    ctx: CoreContext,
    config: &CommitSyncConfig<M>,
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
            .get(
                ctx.clone(),
                source_repoid,
                source_bcs_id,
                destination_repoid,
            )
            .wait()
            .unwrap(),
        expected_bcs_id
    );
    expected_bcs_id.map(move |expected_bcs_id| {
        assert_eq!(
            mapping
                .get(
                    ctx.clone(),
                    destination_repoid,
                    expected_bcs_id,
                    source_repoid
                )
                .wait()
                .unwrap(),
            Some(source_bcs_id)
        )
    });
}

fn sync_parentage(fb: FacebookInit) {
    let ctx = CoreContext::test_mock(fb);
    let megarepo = blobrepo_factory::new_memblob_empty_with_id(None, RepositoryId::new(1)).unwrap();
    let linear = linear::getrepo(fb);
    let linear_path_in_megarepo = MPath::new("linear").unwrap();
    let repos = CommitSyncRepos::SmallToLarge {
        small_repo: linear.clone(),
        large_repo: megarepo.clone(),
        mover: Arc::new(move |path: &MPath| Ok(Some(linear_path_in_megarepo.join(path)))),
    };

    let mapping = SqlSyncedCommitMapping::with_sqlite_in_memory().unwrap();

    let config = CommitSyncConfig::new(mapping, repos);

    create_initial_commit(ctx.clone(), &megarepo);

    // Take 2d7d4ba9ce0a6ffd222de7785b249ead9c51c536 from linear, and rewrite it as a child of master
    // As this is the first commit from linear, it'll rewrite cleanly
    let linear_base_bcs_id = get_bcs_id(
        ctx.clone(),
        &config,
        HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536").unwrap(),
    );
    let expected_bcs_id =
        ChangesetId::from_str("8966842d2031e69108028d6f0ce5812bca28cae53679d066368a8c1472a5bb9a")
            .ok();
    let megarepo_base_bcs_id = sync_to_master(ctx.clone(), &config, linear_base_bcs_id).unwrap();

    // Confirm that we got the expected conversion
    assert_eq!(megarepo_base_bcs_id, expected_bcs_id);
    check_mapping(ctx.clone(), &config, linear_base_bcs_id, expected_bcs_id);

    // Finally, sync another commit
    let linear_second_bcs_id = get_bcs_id(
        ctx.clone(),
        &config,
        HgChangesetId::from_str("3e0e761030db6e479a7fb58b12881883f9f8c63f").unwrap(),
    );
    let expected_bcs_id =
        ChangesetId::from_str("95c03dcd3324e172275ce22a5628d7a501aecb51d9a198b33284887769537acf")
            .unwrap();
    let megarepo_second_bcs_id =
        sync_to_master(ctx.clone(), &config, linear_second_bcs_id).unwrap();
    // Confirm that we got the expected conversion
    assert_eq!(megarepo_second_bcs_id, Some(expected_bcs_id));
    // And check that the synced commit has correct parentage
    assert_eq!(
        megarepo
            .get_changeset_parents_by_bonsai(ctx.clone(), megarepo_second_bcs_id.unwrap())
            .wait()
            .unwrap(),
        vec![megarepo_base_bcs_id.unwrap()]
    );
}

#[fbinit::test]
fn test_sync_parentage(fb: FacebookInit) {
    async_unit::tokio_unit_test(move || sync_parentage(fb))
}

fn sync_removes_commit(fb: FacebookInit) {
    let ctx = CoreContext::test_mock(fb);
    let megarepo = blobrepo_factory::new_memblob_empty_with_id(None, RepositoryId::new(1)).unwrap();
    let linear = linear::getrepo(fb);
    let repos = CommitSyncRepos::SmallToLarge {
        small_repo: linear.clone(),
        large_repo: megarepo.clone(),
        mover: Arc::new(move |_path: &MPath| Ok(None)),
    };

    let mapping = SqlSyncedCommitMapping::with_sqlite_in_memory().unwrap();

    let config = CommitSyncConfig::new(mapping, repos);

    // Create a commit with one file called "master" in the blobrepo, and set the bookmark
    create_initial_commit(ctx.clone(), &megarepo);

    // Take 2d7d4ba9ce0a6ffd222de7785b249ead9c51c536 from linear, and rewrite it as a child of master
    // As this is the first commit from linear, it'll rewrite cleanly, but it should end up being removed
    let linear_base_bcs_id = get_bcs_id(
        ctx.clone(),
        &config,
        HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536").unwrap(),
    );
    let megarepo_base_bcs_id = sync_to_master(ctx.clone(), &config, linear_base_bcs_id).unwrap();

    // Confirm the commit was dropped
    assert_eq!(megarepo_base_bcs_id, None);
    check_mapping(ctx.clone(), &config, linear_base_bcs_id, None);
}

#[fbinit::test]
fn test_sync_removes_commit(fb: FacebookInit) {
    async_unit::tokio_unit_test(move || sync_removes_commit(fb))
}

fn update_master_file(ctx: CoreContext, repo: &BlobRepo) -> ChangesetId {
    let bookmark = BookmarkName::new("master").unwrap();
    let p1 = repo
        .get_bonsai_bookmark(ctx.clone(), &bookmark)
        .wait()
        .unwrap()
        .unwrap();

    let content = FileContents::new_bytes(Bytes::from(b"456" as &[u8]));
    let content_id = content
        .into_blob()
        .store(ctx.clone(), &repo.get_blobstore())
        .wait()
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
        file_changes: btreemap! {MPath::new("master_file").unwrap() => Some(file_change)},
    }
    .freeze()
    .unwrap();

    let bcs_id = bcs.get_changeset_id();
    save_bonsai_changesets(vec![bcs], ctx.clone(), repo.clone())
        .wait()
        .unwrap();

    let mut txn = repo.update_bookmark_transaction(ctx.clone());
    txn.force_set(
        &bookmark,
        bcs_id,
        BookmarkUpdateReason::TestMove {
            bundle_replay_data: None,
        },
    )
    .unwrap();
    txn.commit().wait().unwrap();
    bcs_id
}

fn sync_causes_conflict(fb: FacebookInit) {
    let ctx = CoreContext::test_mock(fb);
    let megarepo = blobrepo_factory::new_memblob_empty_with_id(None, RepositoryId::new(1)).unwrap();
    let linear = linear::getrepo(fb);
    let linear_path_in_megarepo = MPath::new("linear").unwrap();
    let linear_repos = CommitSyncRepos::SmallToLarge {
        small_repo: linear.clone(),
        large_repo: megarepo.clone(),
        mover: Arc::new(move |path: &MPath| Ok(Some(linear_path_in_megarepo.join(path)))),
    };

    let master_file_path_in_megarepo = MPath::new("master_file").unwrap();
    let master_file_repos = CommitSyncRepos::SmallToLarge {
        small_repo: linear.clone(),
        large_repo: megarepo.clone(),
        mover: Arc::new(move |path: &MPath| Ok(Some(master_file_path_in_megarepo.join(path)))),
    };

    let mapping = SqlSyncedCommitMapping::with_sqlite_in_memory().unwrap();

    let linear_config = CommitSyncConfig::new(mapping.clone(), linear_repos);
    let master_file_config = CommitSyncConfig::new(mapping, master_file_repos);

    create_initial_commit(ctx.clone(), &megarepo);

    // Take 2d7d4ba9ce0a6ffd222de7785b249ead9c51c536 from linear, and rewrite it as a child of master
    // As this is the first commit from linear, it'll rewrite cleanly - note that it *cannot* have
    // path conflicts, definitionally, as it will simply overwrite files/dirs in master if needed.
    let linear_base_bcs_id = get_bcs_id(
        ctx.clone(),
        &linear_config,
        HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536").unwrap(),
    );
    sync_to_master(ctx.clone(), &linear_config, linear_base_bcs_id).unwrap();

    // Change master_file
    update_master_file(ctx.clone(), &megarepo);

    // Finally, sync another commit over master_file - this should fail
    let linear_second_bcs_id = get_bcs_id(
        ctx.clone(),
        &master_file_config,
        HgChangesetId::from_str("3e0e761030db6e479a7fb58b12881883f9f8c63f").unwrap(),
    );
    let megarepo_fail_bcs_id =
        sync_to_master(ctx.clone(), &master_file_config, linear_second_bcs_id);
    // Confirm the syncing failed
    assert!(
        megarepo_fail_bcs_id.is_err(),
        format!("{:?}", megarepo_fail_bcs_id)
    );

    check_mapping(ctx.clone(), &master_file_config, linear_second_bcs_id, None);
}

#[fbinit::test]
fn test_sync_causes_conflict(fb: FacebookInit) {
    async_unit::tokio_unit_test(move || sync_causes_conflict(fb))
}

fn sync_empty_commit(fb: FacebookInit) {
    let ctx = CoreContext::test_mock(fb);
    let megarepo = blobrepo_factory::new_memblob_empty_with_id(None, RepositoryId::new(1)).unwrap();
    let linear = linear::getrepo(fb);
    let linear_path_in_megarepo = MPath::new("linear").unwrap();
    let lts_repos = CommitSyncRepos::LargeToSmall {
        small_repo: linear.clone(),
        large_repo: megarepo.clone(),
        mover: Arc::new(move |path: &MPath| {
            Ok(path.remove_prefix_component(&linear_path_in_megarepo))
        }),
    };
    let linear_path_in_megarepo = MPath::new("linear").unwrap();
    let stl_repos = CommitSyncRepos::SmallToLarge {
        small_repo: linear.clone(),
        large_repo: megarepo.clone(),
        mover: Arc::new(move |path: &MPath| Ok(Some(linear_path_in_megarepo.join(path)))),
    };

    let mapping = SqlSyncedCommitMapping::with_sqlite_in_memory().unwrap();

    let lts_config = CommitSyncConfig::new(mapping.clone(), lts_repos);
    let stl_config = CommitSyncConfig::new(mapping, stl_repos);

    create_initial_commit(ctx.clone(), &megarepo);

    // Take 2d7d4ba9ce0a6ffd222de7785b249ead9c51c536 from linear, and rewrite it as a child of master
    // As this is the first commit from linear, it'll rewrite cleanly
    let linear_base_bcs_id = get_bcs_id(
        ctx.clone(),
        &stl_config,
        HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536").unwrap(),
    );
    sync_to_master(ctx.clone(), &stl_config, linear_base_bcs_id).unwrap();

    // Sync an empty commit back to linear
    let megarepo_empty_bcs_id = create_empty_commit(ctx.clone(), &megarepo);
    let linear_empty_bcs_id =
        sync_to_master(ctx.clone(), &lts_config, megarepo_empty_bcs_id).unwrap();

    let expected_bcs_id =
        ChangesetId::from_str("dad900d07c885c21d4361a11590c220cc65c287d52fe1e0f4df61242c7c03f07")
            .ok();
    assert_eq!(linear_empty_bcs_id, expected_bcs_id);
    check_mapping(
        ctx.clone(),
        &lts_config,
        megarepo_empty_bcs_id,
        linear_empty_bcs_id,
    );
}

#[fbinit::test]
fn test_sync_empty_commit(fb: FacebookInit) {
    async_unit::tokio_unit_test(move || sync_empty_commit(fb))
}

fn megarepo_copy_file(
    ctx: CoreContext,
    repo: &BlobRepo,
    linear_bcs_id: ChangesetId,
) -> ChangesetId {
    let bookmark = BookmarkName::new("master").unwrap();
    let p1 = repo
        .get_bonsai_bookmark(ctx.clone(), &bookmark)
        .wait()
        .unwrap()
        .unwrap();

    let content = FileContents::new_bytes(Bytes::from(b"99\n" as &[u8]));
    let content_id = content
        .into_blob()
        .store(ctx.clone(), &repo.get_blobstore())
        .wait()
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
        file_changes: btreemap! {MPath::new("linear/new_file").unwrap() => Some(file_change)},
    }
    .freeze()
    .unwrap();

    let bcs_id = bcs.get_changeset_id();
    save_bonsai_changesets(vec![bcs], ctx.clone(), repo.clone())
        .wait()
        .unwrap();

    let mut txn = repo.update_bookmark_transaction(ctx.clone());
    txn.force_set(
        &bookmark,
        bcs_id,
        BookmarkUpdateReason::TestMove {
            bundle_replay_data: None,
        },
    )
    .unwrap();
    txn.commit().wait().unwrap();
    bcs_id
}

fn sync_copyinfo(fb: FacebookInit) {
    let ctx = CoreContext::test_mock(fb);
    let megarepo = blobrepo_factory::new_memblob_empty_with_id(None, RepositoryId::new(1)).unwrap();
    let linear = linear::getrepo(fb);
    let linear_path_in_megarepo = MPath::new("linear").unwrap();
    let lts_repos = CommitSyncRepos::LargeToSmall {
        small_repo: linear.clone(),
        large_repo: megarepo.clone(),
        mover: Arc::new(move |path: &MPath| {
            Ok(path.remove_prefix_component(&linear_path_in_megarepo))
        }),
    };
    let linear_path_in_megarepo = MPath::new("linear").unwrap();
    let stl_repos = CommitSyncRepos::SmallToLarge {
        small_repo: linear.clone(),
        large_repo: megarepo.clone(),
        mover: Arc::new(move |path: &MPath| Ok(Some(linear_path_in_megarepo.join(path)))),
    };

    let mapping = SqlSyncedCommitMapping::with_sqlite_in_memory().unwrap();
    let stl_config = CommitSyncConfig::new(mapping.clone(), stl_repos);
    let lts_config = CommitSyncConfig::new(mapping, lts_repos);

    create_initial_commit(ctx.clone(), &megarepo);

    // Take 2d7d4ba9ce0a6ffd222de7785b249ead9c51c536 from linear, and rewrite it as a child of master
    // As this is the first commit from linear, it'll rewrite cleanly
    let linear_base_bcs_id = get_bcs_id(
        ctx.clone(),
        &stl_config,
        HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536").unwrap(),
    );
    let megarepo_linear_base_bcs_id =
        sync_to_master(ctx.clone(), &stl_config, linear_base_bcs_id).unwrap();

    // Fetch master from linear - the pushrebase in a remap will change copyinfo
    let linear_master_bcs_id = {
        let bookmark = BookmarkName::new("master").unwrap();
        linear
            .get_bonsai_bookmark(ctx.clone(), &bookmark)
            .wait()
            .unwrap()
            .unwrap()
    };

    let megarepo_copyinfo_commit =
        megarepo_copy_file(ctx.clone(), &megarepo, megarepo_linear_base_bcs_id.unwrap());
    let linear_copyinfo_bcs_id =
        sync_to_master(ctx.clone(), &lts_config, megarepo_copyinfo_commit).unwrap();

    let expected_bcs_id =
        ChangesetId::from_str("68e495f850e16cd4a6b372d27f18f59931139242b5097c137afa1d738769cc60")
            .ok();
    assert_eq!(linear_copyinfo_bcs_id, expected_bcs_id);
    check_mapping(
        ctx.clone(),
        &lts_config,
        megarepo_copyinfo_commit,
        linear_copyinfo_bcs_id,
    );

    // Fetch commit from linear by its new ID, and confirm that it has the correct copyinfo
    let linear_bcs = linear
        .get_bonsai_changeset(ctx.clone(), linear_copyinfo_bcs_id.unwrap())
        .wait()
        .unwrap();

    let file_changes: Vec<_> = linear_bcs.file_changes().collect();
    assert!(file_changes.len() == 1, "Wrong file change count");
    let (path, copy_info) = file_changes.first().unwrap();
    assert_eq!(**path, MPath::new("new_file").unwrap());
    let (copy_source, copy_bcs) = copy_info.unwrap().copy_from().unwrap();
    assert_eq!(*copy_source, MPath::new("1").unwrap());
    assert_eq!(*copy_bcs, linear_master_bcs_id);
}

#[fbinit::test]
fn test_sync_copyinfo(fb: FacebookInit) {
    async_unit::tokio_unit_test(move || sync_copyinfo(fb))
}

fn sync_remap_failure(fb: FacebookInit) {
    let ctx = CoreContext::test_mock(fb);
    let megarepo = blobrepo_factory::new_memblob_empty_with_id(None, RepositoryId::new(1)).unwrap();
    let linear = linear::getrepo(fb);
    let fail_repos = CommitSyncRepos::LargeToSmall {
        small_repo: linear.clone(),
        large_repo: megarepo.clone(),
        mover: Arc::new(move |_path: &MPath| Err(err_msg("This always fails"))),
    };
    let linear_path_in_megarepo = MPath::new("linear").unwrap();
    let stl_repos = CommitSyncRepos::SmallToLarge {
        small_repo: linear.clone(),
        large_repo: megarepo.clone(),
        mover: Arc::new(move |path: &MPath| Ok(Some(linear_path_in_megarepo.join(path)))),
    };
    let linear_path_in_megarepo = MPath::new("linear").unwrap();
    let copyfrom_fail_repos = CommitSyncRepos::LargeToSmall {
        small_repo: linear.clone(),
        large_repo: megarepo.clone(),
        mover: Arc::new(
            move |path: &MPath| match path.basename().to_bytes().as_ref() {
                b"1" => Err(err_msg("This only fails if the file is named '1'")),
                _ => Ok(path.remove_prefix_component(&linear_path_in_megarepo)),
            },
        ),
    };

    let mapping = SqlSyncedCommitMapping::with_sqlite_in_memory().unwrap();
    let fail_config = CommitSyncConfig::new(mapping.clone(), fail_repos);
    let stl_config = CommitSyncConfig::new(mapping.clone(), stl_repos);
    let copyfrom_fail_config = CommitSyncConfig::new(mapping, copyfrom_fail_repos);

    create_initial_commit(ctx.clone(), &megarepo);

    // Take 2d7d4ba9ce0a6ffd222de7785b249ead9c51c536 from linear, and rewrite it as a child of master
    // As this is the first commit from linear, it'll rewrite cleanly
    let linear_base_bcs_id = get_bcs_id(
        ctx.clone(),
        &stl_config,
        HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536").unwrap(),
    );
    let megarepo_linear_base_bcs_id =
        sync_to_master(ctx.clone(), &stl_config, linear_base_bcs_id).unwrap();

    let megarepo_copyinfo_commit =
        megarepo_copy_file(ctx.clone(), &megarepo, megarepo_linear_base_bcs_id.unwrap());

    let always_fail = sync_to_master(ctx.clone(), &fail_config, megarepo_copyinfo_commit);
    assert!(always_fail.is_err());

    let copyfrom_fail =
        sync_to_master(ctx.clone(), &copyfrom_fail_config, megarepo_copyinfo_commit);
    assert!(copyfrom_fail.is_err(), "{:#?}", copyfrom_fail);
}

#[fbinit::test]
fn test_sync_remap_failure(fb: FacebookInit) {
    async_unit::tokio_unit_test(move || sync_remap_failure(fb))
}

fn update_linear_1_file(ctx: CoreContext, repo: &BlobRepo) -> ChangesetId {
    let bookmark = BookmarkName::new("master").unwrap();
    let p1 = repo
        .get_bonsai_bookmark(ctx.clone(), &bookmark)
        .wait()
        .unwrap()
        .unwrap();

    let content = FileContents::new_bytes(Bytes::from(b"999" as &[u8]));
    let content_id = content
        .into_blob()
        .store(ctx.clone(), &repo.get_blobstore())
        .wait()
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
        file_changes: btreemap! {MPath::new("linear/1").unwrap() => Some(file_change)},
    }
    .freeze()
    .unwrap();

    let bcs_id = bcs.get_changeset_id();
    save_bonsai_changesets(vec![bcs], ctx.clone(), repo.clone())
        .wait()
        .unwrap();

    let mut txn = repo.update_bookmark_transaction(ctx.clone());
    txn.force_set(
        &bookmark,
        bcs_id,
        BookmarkUpdateReason::TestMove {
            bundle_replay_data: None,
        },
    )
    .unwrap();
    txn.commit().wait().unwrap();

    bcs_id
}

fn sync_parent_search(fb: FacebookInit) {
    let ctx = CoreContext::test_mock(fb);
    let megarepo = blobrepo_factory::new_memblob_empty_with_id(None, RepositoryId::new(1)).unwrap();
    let linear = linear::getrepo(fb);
    let linear_path_in_megarepo = MPath::new("linear").unwrap();
    let repos = CommitSyncRepos::SmallToLarge {
        small_repo: linear.clone(),
        large_repo: megarepo.clone(),
        mover: Arc::new(move |path: &MPath| Ok(Some(linear_path_in_megarepo.join(path)))),
    };
    let linear_path_in_megarepo = MPath::new("linear").unwrap();
    let reverse_repos = CommitSyncRepos::LargeToSmall {
        small_repo: linear.clone(),
        large_repo: megarepo.clone(),
        mover: Arc::new(move |path: &MPath| {
            Ok(path.remove_prefix_component(&linear_path_in_megarepo))
        }),
    };
    let mapping = SqlSyncedCommitMapping::with_sqlite_in_memory().unwrap();
    let config = CommitSyncConfig::new(mapping.clone(), repos);
    let reverse_config = CommitSyncConfig::new(mapping, reverse_repos);

    create_initial_commit(ctx.clone(), &megarepo);

    // Take 2d7d4ba9ce0a6ffd222de7785b249ead9c51c536 from linear, and rewrite it as a child of master
    // As this is the first commit from linear, it'll rewrite cleanly - note that it *cannot* have
    // path conflicts, definitionally, as it will simply overwrite files/dirs in master if needed.
    let linear_base_bcs_id = get_bcs_id(
        ctx.clone(),
        &config,
        HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536").unwrap(),
    );
    sync_to_master(ctx.clone(), &config, linear_base_bcs_id).unwrap();

    // Change master_file
    update_master_file(ctx.clone(), &megarepo);
    // And change a file in linear
    let new_commit = update_linear_1_file(ctx.clone(), &megarepo);

    // Now sync it back to linear
    let sync_success_bcs_id = sync_to_master(ctx.clone(), &reverse_config, new_commit).unwrap();

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
    );
    // And validate that the mapping is correct when looked at the other way round
    check_mapping(
        ctx.clone(),
        &config,
        sync_success_bcs_id.unwrap(),
        Some(new_commit),
    );
}

#[fbinit::test]
fn test_sync_parent_search(fb: FacebookInit) {
    async_unit::tokio_unit_test(move || sync_parent_search(fb))
}
