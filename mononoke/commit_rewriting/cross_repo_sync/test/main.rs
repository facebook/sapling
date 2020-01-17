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
use std::collections::BTreeMap;
use std::str::FromStr;
use std::sync::Arc;

use anyhow::{bail, Error};
use blobrepo::{save_bonsai_changesets, BlobRepo};
use blobrepo_factory;
use blobstore::Storable;
use bookmarks::{BookmarkName, BookmarkUpdateReason};
use context::CoreContext;
use cross_repo_sync_test_utils::rebase_root_on_master;

use fixtures::{linear, many_files_dirs};
use mercurial_types::HgChangesetId;
use mononoke_types::{
    BlobstoreValue, BonsaiChangesetMut, ChangesetId, DateTime, FileChange, FileContents, FileType,
    MPath, RepositoryId,
};
use movers::Mover;
use synced_commit_mapping::{
    SqlConstructors, SqlSyncedCommitMapping, SyncedCommitMapping, SyncedCommitMappingEntry,
};

use cross_repo_sync::{CommitSyncRepos, CommitSyncer};
use sql::rusqlite::Connection as SqliteConnection;

fn identity_renamer(b: &BookmarkName) -> Option<BookmarkName> {
    Some(b.clone())
}

fn mpath(p: &str) -> MPath {
    MPath::new(p).unwrap()
}

fn create_initial_commit(ctx: CoreContext, repo: &BlobRepo) -> ChangesetId {
    let bookmark = BookmarkName::new("master").unwrap();

    let content = FileContents::new_bytes(Bytes::from(b"123" as &[u8]));
    let content_id = content
        .into_blob()
        .store(ctx.clone(), repo.blobstore())
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
        file_changes: btreemap! {mpath("master_file") => Some(file_change)},
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
    config: &CommitSyncer<M>,
    source_bcs_id: ChangesetId,
) -> Result<Option<ChangesetId>, Error>
where
    M: SyncedCommitMapping + Clone + 'static,
{
    let bookmark_name = BookmarkName::new("master").unwrap();
    let source_bcs = config
        .get_source_repo()
        .get_bonsai_changeset(ctx.clone(), source_bcs_id)
        .wait()
        .unwrap();
    config
        .clone()
        .sync_commit_pushrebase_compat(ctx.clone(), source_bcs, bookmark_name)
        .wait()
}

fn get_bcs_id<M>(
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
        .wait()
        .unwrap()
        .unwrap()
}

fn check_mapping<M>(
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

fn prefix_mover(prefix: &str) -> Mover {
    let prefix = mpath(prefix);
    Arc::new(move |path: &MPath| Ok(Some(prefix.join(path))))
}

fn reverse_prefix_mover(prefix: &str) -> Mover {
    let prefix = mpath(prefix);
    Arc::new(move |path: &MPath| Ok(path.remove_prefix_component(&prefix)))
}

fn sync_parentage(fb: FacebookInit) {
    let ctx = CoreContext::test_mock(fb);
    let (small_repo, megarepo, mapping) = prepare_repos_and_mapping().unwrap();
    linear::initrepo(fb, &small_repo);
    let linear = small_repo;
    let repos = CommitSyncRepos::SmallToLarge {
        small_repo: linear.clone(),
        large_repo: megarepo.clone(),
        mover: prefix_mover("linear"),
        reverse_mover: reverse_prefix_mover("linear"),
        bookmark_renamer: Arc::new(identity_renamer),
        reverse_bookmark_renamer: Arc::new(identity_renamer),
    };

    let config = CommitSyncer::new(mapping, repos);

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

    let megarepo_base_bcs_id =
        rebase_root_on_master(ctx.clone(), &config, linear_base_bcs_id).unwrap();
    // Confirm that we got the expected conversion
    assert_eq!(Some(megarepo_base_bcs_id), expected_bcs_id);
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
        vec![megarepo_base_bcs_id]
    );
}

#[fbinit::test]
fn test_sync_parentage(fb: FacebookInit) {
    async_unit::tokio_unit_test(move || sync_parentage(fb))
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
        .store(ctx.clone(), repo.blobstore())
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
        file_changes: btreemap! {mpath("master_file") => Some(file_change)},
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
    let linear_repos = CommitSyncRepos::SmallToLarge {
        small_repo: linear.clone(),
        large_repo: megarepo.clone(),
        mover: prefix_mover("linear"),
        reverse_mover: reverse_prefix_mover("linear"),
        bookmark_renamer: Arc::new(identity_renamer),
        reverse_bookmark_renamer: Arc::new(identity_renamer),
    };

    let master_file_repos = CommitSyncRepos::SmallToLarge {
        small_repo: linear.clone(),
        large_repo: megarepo.clone(),
        mover: prefix_mover("master_file"),
        reverse_mover: reverse_prefix_mover("master_file"),
        bookmark_renamer: Arc::new(identity_renamer),
        reverse_bookmark_renamer: Arc::new(identity_renamer),
    };

    let mapping = SqlSyncedCommitMapping::with_sqlite_in_memory().unwrap();

    let linear_config = CommitSyncer::new(mapping.clone(), linear_repos);
    let master_file_config = CommitSyncer::new(mapping, master_file_repos);

    create_initial_commit(ctx.clone(), &megarepo);

    // Take 2d7d4ba9ce0a6ffd222de7785b249ead9c51c536 from linear, and rewrite it as a child of master
    let linear_base_bcs_id = get_bcs_id(
        ctx.clone(),
        &linear_config,
        HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536").unwrap(),
    );
    rebase_root_on_master(ctx.clone(), &linear_config, linear_base_bcs_id).unwrap();

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

fn prepare_repos_and_mapping() -> Result<(BlobRepo, BlobRepo, SqlSyncedCommitMapping), Error> {
    let sqlite_con = SqliteConnection::open_in_memory()?;
    sqlite_con.execute_batch(SqlSyncedCommitMapping::get_up_query())?;
    let (megarepo, con) = blobrepo_factory::new_memblob_with_sqlite_connection_with_id(
        sqlite_con,
        RepositoryId::new(1),
    )?;

    let (small_repo, _) =
        blobrepo_factory::new_memblob_with_connection_with_id(con.clone(), RepositoryId::new(0))?;
    let mapping = SqlSyncedCommitMapping::from_connections(con.clone(), con.clone(), con.clone());
    Ok((small_repo, megarepo, mapping))
}

fn sync_empty_commit(fb: FacebookInit) {
    let ctx = CoreContext::test_mock(fb);
    let (small_repo, megarepo, mapping) = prepare_repos_and_mapping().unwrap();
    linear::initrepo(fb, &small_repo);
    let linear = small_repo;
    let lts_repos = CommitSyncRepos::LargeToSmall {
        small_repo: linear.clone(),
        large_repo: megarepo.clone(),
        mover: reverse_prefix_mover("linear"),
        reverse_mover: prefix_mover("linear"),
        bookmark_renamer: Arc::new(identity_renamer),
        reverse_bookmark_renamer: Arc::new(identity_renamer),
    };
    let stl_repos = CommitSyncRepos::SmallToLarge {
        small_repo: linear.clone(),
        large_repo: megarepo.clone(),
        mover: prefix_mover("linear"),
        reverse_mover: reverse_prefix_mover("linear"),
        bookmark_renamer: Arc::new(identity_renamer),
        reverse_bookmark_renamer: Arc::new(identity_renamer),
    };

    let lts_config = CommitSyncer::new(mapping.clone(), lts_repos);
    let stl_config = CommitSyncer::new(mapping, stl_repos);

    create_initial_commit(ctx.clone(), &megarepo);

    // Take 2d7d4ba9ce0a6ffd222de7785b249ead9c51c536 from linear, and rewrite it as a child of master
    // As this is the first commit from linear, it'll rewrite cleanly
    let linear_base_bcs_id = get_bcs_id(
        ctx.clone(),
        &stl_config,
        HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536").unwrap(),
    );
    rebase_root_on_master(ctx.clone(), &stl_config, linear_base_bcs_id).unwrap();

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
        .store(ctx.clone(), repo.blobstore())
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
        file_changes: btreemap! {mpath("linear/new_file") => Some(file_change)},
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
    let (small_repo, megarepo, mapping) = prepare_repos_and_mapping().unwrap();
    linear::initrepo(fb, &small_repo);
    let linear = small_repo;
    let lts_repos = CommitSyncRepos::LargeToSmall {
        small_repo: linear.clone(),
        large_repo: megarepo.clone(),
        mover: reverse_prefix_mover("linear"),
        reverse_mover: prefix_mover("linear"),
        bookmark_renamer: Arc::new(identity_renamer),
        reverse_bookmark_renamer: Arc::new(identity_renamer),
    };
    let stl_repos = CommitSyncRepos::SmallToLarge {
        small_repo: linear.clone(),
        large_repo: megarepo.clone(),
        mover: prefix_mover("linear"),
        reverse_mover: reverse_prefix_mover("linear"),
        bookmark_renamer: Arc::new(identity_renamer),
        reverse_bookmark_renamer: Arc::new(identity_renamer),
    };

    let stl_config = CommitSyncer::new(mapping.clone(), stl_repos);
    let lts_config = CommitSyncer::new(mapping, lts_repos);

    create_initial_commit(ctx.clone(), &megarepo);

    // Take 2d7d4ba9ce0a6ffd222de7785b249ead9c51c536 from linear, and rewrite it as a child of master
    // As this is the first commit from linear, it'll rewrite cleanly
    let linear_base_bcs_id = get_bcs_id(
        ctx.clone(),
        &stl_config,
        HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536").unwrap(),
    );
    let megarepo_linear_base_bcs_id =
        rebase_root_on_master(ctx.clone(), &stl_config, linear_base_bcs_id).unwrap();

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
        megarepo_copy_file(ctx.clone(), &megarepo, megarepo_linear_base_bcs_id);
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
    assert_eq!(**path, mpath("new_file"));
    let (copy_source, copy_bcs) = copy_info.unwrap().copy_from().unwrap();
    assert_eq!(*copy_source, mpath("1"));
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
        mover: Arc::new(move |_path: &MPath| bail!("This always fails")),
        reverse_mover: Arc::new(move |_path: &MPath| bail!("This always fails")),
        bookmark_renamer: Arc::new(identity_renamer),
        reverse_bookmark_renamer: Arc::new(identity_renamer),
    };
    let stl_repos = CommitSyncRepos::SmallToLarge {
        small_repo: linear.clone(),
        large_repo: megarepo.clone(),
        mover: prefix_mover("linear"),
        reverse_mover: reverse_prefix_mover("linear"),
        bookmark_renamer: Arc::new(identity_renamer),
        reverse_bookmark_renamer: Arc::new(identity_renamer),
    };
    let linear_path_in_megarepo = mpath("linear");
    let copyfrom_fail_repos = CommitSyncRepos::LargeToSmall {
        small_repo: linear.clone(),
        large_repo: megarepo.clone(),
        mover: Arc::new(
            move |path: &MPath| match path.basename().to_bytes().as_ref() {
                b"1" => bail!("This only fails if the file is named '1'"),
                _ => Ok(path.remove_prefix_component(&linear_path_in_megarepo)),
            },
        ),
        reverse_mover: Arc::new(
            move |path: &MPath| match path.basename().to_bytes().as_ref() {
                b"1" => bail!("This only fails if the file is named '1'"),
                _ => Ok(Some(mpath("linear").join(path))),
            },
        ),
        bookmark_renamer: Arc::new(identity_renamer),
        reverse_bookmark_renamer: Arc::new(identity_renamer),
    };

    let mapping = SqlSyncedCommitMapping::with_sqlite_in_memory().unwrap();
    let fail_config = CommitSyncer::new(mapping.clone(), fail_repos);
    let stl_config = CommitSyncer::new(mapping.clone(), stl_repos);
    let copyfrom_fail_config = CommitSyncer::new(mapping, copyfrom_fail_repos);

    create_initial_commit(ctx.clone(), &megarepo);

    // Take 2d7d4ba9ce0a6ffd222de7785b249ead9c51c536 from linear, and rewrite it as a child of master
    // As this is the first commit from linear, it'll rewrite cleanly
    let linear_base_bcs_id = get_bcs_id(
        ctx.clone(),
        &stl_config,
        HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536").unwrap(),
    );
    let megarepo_linear_base_bcs_id =
        rebase_root_on_master(ctx.clone(), &stl_config, linear_base_bcs_id).unwrap();

    let megarepo_copyinfo_commit =
        megarepo_copy_file(ctx.clone(), &megarepo, megarepo_linear_base_bcs_id);

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

fn sync_implicit_deletes(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let (small_repo, megarepo, mapping) = prepare_repos_and_mapping().unwrap();
    many_files_dirs::initrepo(fb, &small_repo);
    let repo = small_repo;

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
    };

    let commit_syncer = CommitSyncer::new(mapping.clone(), commit_sync_repos);

    let megarepo_initial_bcs_id = create_initial_commit(ctx.clone(), &megarepo);

    // Insert a fake mapping entry, so that syncs succeed
    let repo_initial_bcs_id = get_bcs_id(
        ctx.clone(),
        &commit_syncer,
        HgChangesetId::from_str("2f866e7e549760934e31bf0420a873f65100ad63").unwrap(),
    );
    let entry = SyncedCommitMappingEntry::new(
        megarepo.get_repoid(),
        megarepo_initial_bcs_id,
        repo.get_repoid(),
        repo_initial_bcs_id,
    );
    mapping.add(ctx.clone(), entry).wait()?;

    // d261bc7900818dea7c86935b3fb17a33b2e3a6b4 from "many_files_dirs" should sync cleanly
    // on top of master. Among others, it introduces the following files:
    // - "dir1/subdir1/subsubdir1/file_1"
    // - "dir1/subdir1/subsubdir2/file_1"
    // - "dir1/subdir1/subsubdir2/file_2"
    let repo_base_bcs_id = get_bcs_id(
        ctx.clone(),
        &commit_syncer,
        HgChangesetId::from_str("d261bc7900818dea7c86935b3fb17a33b2e3a6b4").unwrap(),
    );

    sync_to_master(ctx.clone(), &commit_syncer, repo_base_bcs_id)
        .expect("Unexpectedly failed to rewrite 1")
        .expect("Unexpectedly rewritten into nothingness");

    // 051946ed218061e925fb120dac02634f9ad40ae2 from "many_files_dirs" replaces the
    // entire "dir1" directory with a file, which implicitly deletes
    // "dir1/subdir1/subsubdir1" and "dir1/subdir1/subsubdir2".
    let repo_implicit_delete_bcs_id = get_bcs_id(
        ctx.clone(),
        &commit_syncer,
        HgChangesetId::from_str("051946ed218061e925fb120dac02634f9ad40ae2").unwrap(),
    );
    let megarepo_implicit_delete_bcs_id =
        sync_to_master(ctx.clone(), &commit_syncer, repo_implicit_delete_bcs_id)
            .expect("Unexpectedly failed to rewrite 2")
            .expect("Unexpectedly rewritten into nothingness");

    let megarepo_implicit_delete_bcs = megarepo
        .get_bonsai_changeset(ctx.clone(), megarepo_implicit_delete_bcs_id)
        .wait()
        .unwrap();
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

#[fbinit::test]
fn test_sync_implicit_deletes(fb: FacebookInit) {
    async_unit::tokio_unit_test(move || sync_implicit_deletes(fb).unwrap())
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
        .store(ctx.clone(), repo.blobstore())
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
        file_changes: btreemap! {mpath("linear/1") => Some(file_change)},
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
    let (small_repo, megarepo, mapping) = prepare_repos_and_mapping().unwrap();
    linear::initrepo(fb, &small_repo);
    let linear = small_repo;
    let repos = CommitSyncRepos::SmallToLarge {
        small_repo: linear.clone(),
        large_repo: megarepo.clone(),
        mover: prefix_mover("linear"),
        reverse_mover: reverse_prefix_mover("linear"),
        bookmark_renamer: Arc::new(identity_renamer),
        reverse_bookmark_renamer: Arc::new(identity_renamer),
    };
    let reverse_repos = CommitSyncRepos::LargeToSmall {
        small_repo: linear.clone(),
        large_repo: megarepo.clone(),
        mover: reverse_prefix_mover("linear"),
        reverse_mover: prefix_mover("linear"),
        bookmark_renamer: Arc::new(identity_renamer),
        reverse_bookmark_renamer: Arc::new(identity_renamer),
    };
    let config = CommitSyncer::new(mapping.clone(), repos);
    let reverse_config = CommitSyncer::new(mapping, reverse_repos);

    create_initial_commit(ctx.clone(), &megarepo);

    // Take 2d7d4ba9ce0a6ffd222de7785b249ead9c51c536 from linear, and rewrite it as a child of master
    let linear_base_bcs_id = get_bcs_id(
        ctx.clone(),
        &config,
        HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536").unwrap(),
    );
    rebase_root_on_master(ctx.clone(), &config, linear_base_bcs_id).unwrap();

    // Change master_file
    let master_file_cs_id = update_master_file(ctx.clone(), &megarepo);
    sync_to_master(ctx.clone(), &reverse_config, master_file_cs_id).unwrap();
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
