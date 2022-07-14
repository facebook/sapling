/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Tests for the synced commits mapping.

use anyhow::anyhow;
use anyhow::Error;
use ascii::AsciiString;
use assert_matches::assert_matches;
use bytes::Bytes;
use fbinit::FacebookInit;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;

use blobrepo::save_bonsai_changesets;
use blobrepo::BlobRepo;
use blobstore::Loadable;
use blobstore::Storable;
use bookmarks::BookmarkName;
use bookmarks::BookmarkUpdateReason;
use cacheblob::InProcessLease;
use cloned::cloned;
use context::CoreContext;
use cross_repo_sync::update_mapping_with_version;
use cross_repo_sync::validation::verify_working_copy;
use cross_repo_sync::CommitSyncContext;
use cross_repo_sync::CommitSyncDataProvider;
use cross_repo_sync::CommitSyncOutcome;
use cross_repo_sync::ErrorKind;
use cross_repo_sync_test_utils::rebase_root_on_master;
use fixtures::Linear;
use fixtures::ManyFilesDirs;
use fixtures::TestRepoFixture;
use futures::future::join_all;
use futures::FutureExt;
use futures::TryStreamExt;
use live_commit_sync_config::TestLiveCommitSyncConfig;
use live_commit_sync_config::TestLiveCommitSyncConfigSource;
use manifest::ManifestOps;
use maplit::btreemap;
use maplit::hashmap;
use mercurial_derived_data::DeriveHgChangeset;
use mercurial_types::HgChangesetId;
use metaconfig_types::CommitSyncConfig;
use metaconfig_types::CommitSyncConfigVersion;
use metaconfig_types::CommonCommitSyncConfig;
use metaconfig_types::DefaultSmallToLargeCommitSyncPathAction;
use metaconfig_types::SmallRepoCommitSyncConfig;
use metaconfig_types::SmallRepoPermanentConfig;
use mononoke_types::BlobstoreValue;
use mononoke_types::BonsaiChangesetMut;
use mononoke_types::ChangesetId;
use mononoke_types::DateTime;
use mononoke_types::FileChange;
use mononoke_types::FileContents;
use mononoke_types::FileType;
use mononoke_types::MPath;
use mononoke_types::RepositoryId;
use pushrebase::PushrebaseError;
use reachabilityindex::LeastCommonAncestorsHint;
use skiplist::SkiplistIndex;
use sorted_vector_map::sorted_vector_map;
use sorted_vector_map::SortedVectorMap;
use sql_construct::SqlConstruct;
use synced_commit_mapping::SqlSyncedCommitMapping;
use synced_commit_mapping::SyncedCommitMapping;
use synced_commit_mapping::SyncedCommitMappingEntry;
use test_repo_factory::TestRepoFactory;
use tests_utils::bookmark;
use tests_utils::resolve_cs_id;
use tests_utils::CreateCommitContext;
use tunables::with_tunables_async;
use tunables::MononokeTunables;

use cross_repo_sync::types::Target;
use cross_repo_sync::CandidateSelectionHint;
use cross_repo_sync::CommitSyncRepos;
use cross_repo_sync::CommitSyncer;
use cross_repo_sync::PluralCommitSyncOutcome;
use sql::rusqlite::Connection as SqliteConnection;

fn mpath(p: &str) -> MPath {
    MPath::new(p).unwrap()
}

async fn move_bookmark(ctx: &CoreContext, repo: &BlobRepo, bookmark: &str, cs_id: ChangesetId) {
    let bookmark = BookmarkName::new(bookmark).unwrap();
    let mut txn = repo.update_bookmark_transaction(ctx.clone());
    txn.force_set(&bookmark, cs_id, BookmarkUpdateReason::TestMove)
        .unwrap();
    txn.commit().await.unwrap();
}

async fn get_bookmark(ctx: &CoreContext, repo: &BlobRepo, bookmark: &str) -> ChangesetId {
    let bookmark = BookmarkName::new(bookmark).unwrap();
    repo.get_bonsai_bookmark(ctx.clone(), &bookmark)
        .await
        .unwrap()
        .unwrap()
}

async fn create_initial_commit(ctx: CoreContext, repo: &BlobRepo) -> ChangesetId {
    create_initial_commit_with_contents(
        ctx,
        repo,
        btreemap! { "master_file" => Some(b"123" as &[u8]) },
    )
    .await
}

async fn create_initial_commit_with_contents<'a>(
    ctx: CoreContext,
    repo: &'a BlobRepo,
    file_changes: BTreeMap<&'static str, Option<impl Into<Bytes>>>,
) -> ChangesetId {
    let bookmark = BookmarkName::new("master").unwrap();

    let file_changes: Vec<(_, _)> = join_all(file_changes.into_iter().map(|(path, contents)| {
        cloned!(ctx);
        async move {
            let path = mpath(path);
            let file_change = match contents {
                Some(contents) => {
                    let contents = FileContents::new_bytes(contents.into());
                    let content_id = contents
                        .into_blob()
                        .store(&ctx, repo.blobstore())
                        .await
                        .unwrap();
                    FileChange::tracked(content_id, FileType::Regular, 3, None)
                }
                None => FileChange::Deletion,
            };
            (path, file_change)
        }
    }))
    .await;
    let file_changes: SortedVectorMap<_, _> = file_changes.into_iter().collect();

    let bcs = BonsaiChangesetMut {
        parents: vec![],
        author: "Test User <test@fb.com>".to_string(),
        author_date: DateTime::from_timestamp(1504040000, 0).unwrap(),
        committer: None,
        committer_date: None,
        message: "Initial commit to get going".to_string(),
        extra: Default::default(),
        file_changes,
        is_snapshot: false,
    }
    .freeze()
    .unwrap();

    let bcs_id = bcs.get_changeset_id();
    save_bonsai_changesets(vec![bcs], ctx.clone(), repo)
        .await
        .unwrap();

    let mut txn = repo.update_bookmark_transaction(ctx.clone());
    txn.force_set(&bookmark, bcs_id, BookmarkUpdateReason::TestMove)
        .unwrap();
    txn.commit().await.unwrap();
    bcs_id
}

async fn create_empty_commit(ctx: CoreContext, repo: &BlobRepo) -> ChangesetId {
    let bookmark = BookmarkName::new("master").unwrap();
    let p1 = repo
        .get_bonsai_bookmark(ctx.clone(), &bookmark)
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
        extra: Default::default(),
        file_changes: sorted_vector_map! {},
        is_snapshot: false,
    }
    .freeze()
    .unwrap();

    let bcs_id = bcs.get_changeset_id();
    save_bonsai_changesets(vec![bcs], ctx.clone(), repo)
        .await
        .unwrap();

    let mut txn = repo.update_bookmark_transaction(ctx.clone());
    txn.force_set(&bookmark, bcs_id, BookmarkUpdateReason::TestMove)
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
        .load(&ctx, config.get_source_repo().blobstore())
        .await
        .unwrap();

    config
        .unsafe_sync_commit_pushrebase(
            &ctx,
            source_bcs,
            bookmark_name,
            Target(Arc::new(SkiplistIndex::new())),
            CommitSyncContext::Tests,
        )
        .await
}

async fn get_bcs_id<M>(
    ctx: &CoreContext,
    config: &CommitSyncer<M>,
    source_hg_cs: HgChangesetId,
) -> ChangesetId
where
    M: SyncedCommitMapping + Clone + 'static,
{
    config
        .get_source_repo()
        .bonsai_hg_mapping()
        .get_bonsai_from_hg(ctx, source_hg_cs)
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
            .get(&ctx, source_repoid, source_bcs_id, destination_repoid,)
            .await
            .unwrap()
            .into_iter()
            .next()
            .map(|(cs, _maybe_version, _maybe_source_repo)| cs),
        expected_bcs_id
    );

    if let Some(expected_bcs_id) = expected_bcs_id {
        assert_eq!(
            mapping
                .get(&ctx, destination_repoid, expected_bcs_id, source_repoid)
                .await
                .unwrap()
                .into_iter()
                .next()
                .map(|(cs, _maybe_version, _maybe_source_repo)| cs),
            Some(source_bcs_id)
        );
    }
}

pub fn version_name_with_small_repo() -> CommitSyncConfigVersion {
    CommitSyncConfigVersion("TEST_VERSION_NAME".to_string())
}

fn create_commit_sync_config(
    small_repo_id: RepositoryId,
    large_repo_id: RepositoryId,
    prefix: &str,
) -> Result<CommitSyncConfig, Error> {
    let small_repo_config = SmallRepoCommitSyncConfig {
        default_action: DefaultSmallToLargeCommitSyncPathAction::PrependPrefix(MPath::new(prefix)?),
        map: hashmap! {},
    };

    Ok(CommitSyncConfig {
        large_repo_id,
        common_pushrebase_bookmarks: vec![],
        small_repos: hashmap! {
            small_repo_id => small_repo_config,
        },
        version_name: version_name_with_small_repo(),
    })
}

fn create_small_to_large_commit_syncer(
    ctx: &CoreContext,
    small_repo: BlobRepo,
    large_repo: BlobRepo,
    prefix: &str,
    mapping: SqlSyncedCommitMapping,
) -> Result<CommitSyncer<SqlSyncedCommitMapping>, Error> {
    let small_repo_id = small_repo.get_repoid();
    let large_repo_id = large_repo.get_repoid();

    let common_config = CommonCommitSyncConfig {
        common_pushrebase_bookmarks: vec![],
        small_repos: hashmap! {
            small_repo.get_repoid() => SmallRepoPermanentConfig {
                bookmark_prefix: AsciiString::new(),
            }
        },
        large_repo_id: large_repo.get_repoid(),
    };
    let repos = CommitSyncRepos::new(small_repo, large_repo, &common_config)?;
    let commit_sync_config = create_commit_sync_config(small_repo_id, large_repo_id, prefix)?;

    let (sync_config, source) = TestLiveCommitSyncConfig::new_with_source();
    source.add_config(commit_sync_config);
    source.add_common_config(common_config);

    let live_commit_sync_config = Arc::new(sync_config);
    let lease = Arc::new(InProcessLease::new());
    Ok(CommitSyncer::new(
        ctx,
        mapping,
        repos,
        live_commit_sync_config,
        lease,
    ))
}

fn create_large_to_small_commit_syncer_and_config_source(
    ctx: &CoreContext,
    small_repo: BlobRepo,
    large_repo: BlobRepo,
    prefix: &str,
    mapping: SqlSyncedCommitMapping,
) -> Result<
    (
        CommitSyncer<SqlSyncedCommitMapping>,
        TestLiveCommitSyncConfigSource,
    ),
    Error,
> {
    let small_repo_id = small_repo.get_repoid();
    let large_repo_id = large_repo.get_repoid();

    let commit_sync_config = create_commit_sync_config(small_repo_id, large_repo_id, prefix)?;
    let common_config = CommonCommitSyncConfig {
        common_pushrebase_bookmarks: vec![],
        small_repos: hashmap! {
            small_repo.get_repoid() => SmallRepoPermanentConfig {
                bookmark_prefix: AsciiString::new(),
            }
        },
        large_repo_id: large_repo.get_repoid(),
    };
    let repos = CommitSyncRepos::new(large_repo, small_repo, &common_config)?;

    let (sync_config, source) = TestLiveCommitSyncConfig::new_with_source();
    source.add_config(commit_sync_config);
    source.add_common_config(common_config);

    let live_commit_sync_config = Arc::new(sync_config);
    let lease = Arc::new(InProcessLease::new());
    Ok((
        CommitSyncer::new(ctx, mapping, repos, live_commit_sync_config, lease),
        source,
    ))
}

fn create_large_to_small_commit_syncer(
    ctx: &CoreContext,
    small_repo: BlobRepo,
    large_repo: BlobRepo,
    prefix: &str,
    mapping: SqlSyncedCommitMapping,
) -> Result<CommitSyncer<SqlSyncedCommitMapping>, Error> {
    let (syncer, _) = create_large_to_small_commit_syncer_and_config_source(
        ctx, small_repo, large_repo, prefix, mapping,
    )?;
    Ok(syncer)
}

#[fbinit::test]
async fn test_sync_parentage(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let (small_repo, megarepo, mapping) = prepare_repos_and_mapping(fb)?;
    Linear::initrepo(fb, &small_repo).await;
    let config =
        create_small_to_large_commit_syncer(&ctx, small_repo, megarepo.clone(), "linear", mapping)?;
    create_initial_commit(ctx.clone(), &megarepo).await;

    // Take 2d7d4ba9ce0a6ffd222de7785b249ead9c51c536 from linear, and rewrite it as a child of master
    // As this is the first commit from linear, it'll rewrite cleanly
    let linear_base_bcs_id = get_bcs_id(
        &ctx,
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
        &ctx,
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
    let mut proper_changes: BTreeMap<MPath, FileChange> = BTreeMap::new();
    for (path, maybe_content) in changes.into_iter() {
        let mpath = MPath::new(path).unwrap();
        match maybe_content {
            None => {
                proper_changes.insert(mpath, FileChange::Deletion);
            }
            Some(content) => {
                let file_contents = FileContents::new_bytes(content.as_bytes());
                let content_id = file_contents
                    .into_blob()
                    .store(ctx, repo.blobstore())
                    .await
                    .unwrap();
                let file_change =
                    FileChange::tracked(content_id, FileType::Regular, content.len() as u64, None);

                proper_changes.insert(mpath, file_change);
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
        extra: Default::default(),
        file_changes: proper_changes.into(),
        is_snapshot: false,
    }
    .freeze()
    .unwrap();

    let bcs_id = bcs.get_changeset_id();
    save_bonsai_changesets(vec![bcs], ctx.clone(), repo)
        .await
        .unwrap();

    bcs_id
}

async fn update_master_file(ctx: CoreContext, repo: &BlobRepo) -> ChangesetId {
    let bookmark = BookmarkName::new("master").unwrap();
    let p1 = repo
        .get_bonsai_bookmark(ctx.clone(), &bookmark)
        .await
        .unwrap()
        .unwrap();

    let content = FileContents::new_bytes(Bytes::from(b"456" as &[u8]));
    let content_id = content
        .into_blob()
        .store(&ctx, repo.blobstore())
        .await
        .unwrap();
    let file_change = FileChange::tracked(content_id, FileType::Regular, 3, None);

    let bcs = BonsaiChangesetMut {
        parents: vec![p1],
        author: "Test User <test@fb.com>".to_string(),
        author_date: DateTime::from_timestamp(1504040001, 0).unwrap(),
        committer: None,
        committer_date: None,
        message: "Change master_file".to_string(),
        extra: Default::default(),
        file_changes: sorted_vector_map! {mpath("master_file") => file_change},
        is_snapshot: false,
    }
    .freeze()
    .unwrap();

    let bcs_id = bcs.get_changeset_id();
    save_bonsai_changesets(vec![bcs], ctx.clone(), repo)
        .await
        .unwrap();

    let mut txn = repo.update_bookmark_transaction(ctx.clone());
    txn.force_set(&bookmark, bcs_id, BookmarkUpdateReason::TestMove)
        .unwrap();
    txn.commit().await.unwrap();
    bcs_id
}

#[fbinit::test]
async fn test_sync_causes_conflict(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let megarepo: BlobRepo = TestRepoFactory::new(fb)?
        .with_id(RepositoryId::new(1))
        .build()?;

    let mapping = SqlSyncedCommitMapping::with_sqlite_in_memory()?;
    let linear = Linear::getrepo(fb).await;
    let linear_config = create_small_to_large_commit_syncer(
        &ctx,
        linear.clone(),
        megarepo.clone(),
        "linear",
        mapping.clone(),
    )?;

    let master_file_config = create_small_to_large_commit_syncer(
        &ctx,
        linear,
        megarepo.clone(),
        "master_file",
        mapping,
    )?;

    create_initial_commit(ctx.clone(), &megarepo).await;

    // Take 2d7d4ba9ce0a6ffd222de7785b249ead9c51c536 from linear, and rewrite it as a child of master
    let linear_base_bcs_id = get_bcs_id(
        &ctx,
        &linear_config,
        HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536")?,
    )
    .await;
    rebase_root_on_master(ctx.clone(), &linear_config, linear_base_bcs_id).await?;

    // Change master_file
    update_master_file(ctx.clone(), &megarepo).await;

    // Finally, sync another commit over master_file - this should fail
    let linear_second_bcs_id = get_bcs_id(
        &ctx,
        &master_file_config,
        HgChangesetId::from_str("3e0e761030db6e479a7fb58b12881883f9f8c63f")?,
    )
    .await;
    let megarepo_fail_bcs_id =
        sync_to_master(ctx.clone(), &master_file_config, linear_second_bcs_id).await;
    // Confirm the syncing failed
    assert!(megarepo_fail_bcs_id.is_err(), "{:?}", megarepo_fail_bcs_id);

    check_mapping(ctx.clone(), &master_file_config, linear_second_bcs_id, None).await;
    Ok(())
}

fn prepare_repos_and_mapping(
    fb: FacebookInit,
) -> Result<(BlobRepo, BlobRepo, SqlSyncedCommitMapping), Error> {
    let metadata_con = SqliteConnection::open_in_memory()?;
    metadata_con.execute_batch(SqlSyncedCommitMapping::CREATION_QUERY)?;
    let hg_mutation_con = SqliteConnection::open_in_memory()?;
    let mut factory = TestRepoFactory::with_sqlite_connection(fb, metadata_con, hg_mutation_con)?;
    let megarepo = factory.with_id(RepositoryId::new(1)).build()?;
    let small_repo = factory.with_id(RepositoryId::new(0)).build()?;
    let mapping =
        SqlSyncedCommitMapping::from_sql_connections(factory.metadata_db().clone().into());
    Ok((small_repo, megarepo, mapping))
}

#[fbinit::test]
async fn test_sync_empty_commit(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let (small_repo, megarepo, mapping) = prepare_repos_and_mapping(fb)?;
    Linear::initrepo(fb, &small_repo).await;
    let linear = small_repo;

    let stl_config = create_small_to_large_commit_syncer(
        &ctx,
        linear.clone(),
        megarepo.clone(),
        "linear",
        mapping.clone(),
    )?;
    let lts_config = create_large_to_small_commit_syncer(
        &ctx,
        linear.clone(),
        megarepo.clone(),
        "linear",
        mapping.clone(),
    )?;

    create_initial_commit(ctx.clone(), &megarepo).await;

    // Take 2d7d4ba9ce0a6ffd222de7785b249ead9c51c536 from linear, and rewrite it as a child of master
    // As this is the first commit from linear, it'll rewrite cleanly
    let linear_base_bcs_id = get_bcs_id(
        &ctx,
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
        .await
        .unwrap()
        .unwrap();

    let content = FileContents::new_bytes(Bytes::from(b"99\n" as &[u8]));
    let content_id = content
        .into_blob()
        .store(&ctx, repo.blobstore())
        .await
        .unwrap();
    let file_change = FileChange::tracked(
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
        extra: Default::default(),
        file_changes: sorted_vector_map! {mpath("linear/new_file") => file_change},
        is_snapshot: false,
    }
    .freeze()
    .unwrap();

    let bcs_id = bcs.get_changeset_id();
    save_bonsai_changesets(vec![bcs], ctx.clone(), repo)
        .await
        .unwrap();

    let mut txn = repo.update_bookmark_transaction(ctx.clone());
    txn.force_set(&bookmark, bcs_id, BookmarkUpdateReason::TestMove)
        .unwrap();
    txn.commit().await.unwrap();
    bcs_id
}

#[fbinit::test]
async fn test_sync_copyinfo(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let (small_repo, megarepo, mapping) = prepare_repos_and_mapping(fb).unwrap();
    Linear::initrepo(fb, &small_repo).await;
    let linear = small_repo;

    let stl_config = create_small_to_large_commit_syncer(
        &ctx,
        linear.clone(),
        megarepo.clone(),
        "linear",
        mapping.clone(),
    )?;
    let lts_config = create_large_to_small_commit_syncer(
        &ctx,
        linear.clone(),
        megarepo.clone(),
        "linear",
        mapping,
    )?;

    create_initial_commit(ctx.clone(), &megarepo).await;

    // Take 2d7d4ba9ce0a6ffd222de7785b249ead9c51c536 from linear, and rewrite it as a child of master
    // As this is the first commit from linear, it'll rewrite cleanly
    let linear_base_bcs_id = get_bcs_id(
        &ctx,
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
        .load(&ctx, linear.blobstore())
        .await?;

    let file_changes: Vec<_> = linear_bcs.file_changes().collect();
    assert!(file_changes.len() == 1, "Wrong file change count");
    let (path, copy_info) = file_changes.first().unwrap();
    assert_eq!(**path, mpath("new_file"));
    let (copy_source, copy_bcs) = match copy_info {
        FileChange::Change(tc) => tc.copy_from().unwrap(),
        _ => panic!(),
    };
    assert_eq!(*copy_source, mpath("1"));
    assert_eq!(*copy_bcs, linear_master_bcs_id);

    Ok(())
}

#[fbinit::test]
async fn test_sync_implicit_deletes(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let (small_repo, megarepo, mapping) = prepare_repos_and_mapping(fb).unwrap();
    ManyFilesDirs::initrepo(fb, &small_repo).await;
    let repo = small_repo.clone();

    let mut commit_syncer = create_small_to_large_commit_syncer(
        &ctx,
        repo.clone(),
        megarepo.clone(),
        "linear",
        mapping.clone(),
    )?;

    let small_repo_config = SmallRepoCommitSyncConfig {
        default_action: DefaultSmallToLargeCommitSyncPathAction::Preserve,
        map: hashmap! {
            MPath::new("dir1/subdir1/subsubdir1")? => MPath::new("prefix1")?,
            MPath::new("dir1")? => MPath::new("prefix2")?,
        },
    };

    let commit_sync_config = CommitSyncConfig {
        large_repo_id: megarepo.get_repoid(),
        common_pushrebase_bookmarks: vec![],
        small_repos: hashmap! {
            small_repo.get_repoid() => small_repo_config,
        },
        version_name: version_name_with_small_repo(),
    };

    let common_config = CommonCommitSyncConfig {
        common_pushrebase_bookmarks: vec![],
        small_repos: hashmap! {
            small_repo.get_repoid() => SmallRepoPermanentConfig {
                bookmark_prefix: AsciiString::new(),
            }
        },
        large_repo_id: megarepo.get_repoid(),
    };
    let (sync_config, source) = TestLiveCommitSyncConfig::new_with_source();

    source.add_config(commit_sync_config.clone());
    source.add_common_config(common_config);

    let live_commit_sync_config = Arc::new(sync_config);

    let commit_sync_repos = CommitSyncRepos::SmallToLarge {
        small_repo: repo.clone(),
        large_repo: megarepo.clone(),
    };
    let version = version_name_with_small_repo();
    let commit_sync_data_provider = CommitSyncDataProvider::Live(live_commit_sync_config);
    commit_syncer.commit_sync_data_provider = commit_sync_data_provider;
    commit_syncer.repos = commit_sync_repos;

    let megarepo_initial_bcs_id = create_initial_commit(ctx.clone(), &megarepo).await;

    // Insert a fake mapping entry, so that syncs succeed
    let repo_initial_bcs_id = get_bcs_id(
        &ctx,
        &commit_syncer,
        HgChangesetId::from_str("2f866e7e549760934e31bf0420a873f65100ad63").unwrap(),
    )
    .await;
    let entry = SyncedCommitMappingEntry::new(
        megarepo.get_repoid(),
        megarepo_initial_bcs_id,
        repo.get_repoid(),
        repo_initial_bcs_id,
        version,
        commit_syncer.get_source_repo_type(),
    );
    mapping.add(&ctx, entry).await?;

    // d261bc7900818dea7c86935b3fb17a33b2e3a6b4 from "ManyFilesDirs" should sync cleanly
    // on top of master. Among others, it introduces the following files:
    // - "dir1/subdir1/subsubdir1/file_1"
    // - "dir1/subdir1/subsubdir2/file_1"
    // - "dir1/subdir1/subsubdir2/file_2"
    let repo_base_bcs_id = get_bcs_id(
        &ctx,
        &commit_syncer,
        HgChangesetId::from_str("d261bc7900818dea7c86935b3fb17a33b2e3a6b4").unwrap(),
    )
    .await;

    sync_to_master(ctx.clone(), &commit_syncer, repo_base_bcs_id)
        .await?
        .expect("Unexpectedly rewritten into nothingness");

    // 051946ed218061e925fb120dac02634f9ad40ae2 from "ManyFilesDirs" replaces the
    // entire "dir1" directory with a file, which implicitly deletes
    // "dir1/subdir1/subsubdir1" and "dir1/subdir1/subsubdir2".
    let repo_implicit_delete_bcs_id = get_bcs_id(
        &ctx,
        &commit_syncer,
        HgChangesetId::from_str("051946ed218061e925fb120dac02634f9ad40ae2").unwrap(),
    )
    .await;
    let megarepo_implicit_delete_bcs_id =
        sync_to_master(ctx.clone(), &commit_syncer, repo_implicit_delete_bcs_id)
            .await?
            .expect("Unexpectedly rewritten into nothingness");

    let megarepo_implicit_delete_bcs = megarepo_implicit_delete_bcs_id
        .load(&ctx, megarepo.blobstore())
        .await?;
    let file_changes: BTreeMap<MPath, _> = megarepo_implicit_delete_bcs
        .file_changes()
        .map(|(a, b)| (a.clone(), b.clone()))
        .collect();

    // "dir1" was rewrtitten as "prefix2" and explicitly replaced with file, so the file
    // change should be `Some`
    assert!(file_changes[&mpath("prefix2")].is_changed());
    // "dir1/subdir1/subsubdir1/file_1" was rewritten as "prefix1/file_1", and became
    // an implicit delete
    assert!(file_changes[&mpath("prefix1/file_1")].is_removed());
    // there are no other entries in `file_changes` as other implicit deletes where
    // removed by the minimization process
    assert_eq!(file_changes.len(), 2);

    Ok(())
}

async fn update_linear_1_file(ctx: CoreContext, repo: &BlobRepo) -> ChangesetId {
    let bookmark = BookmarkName::new("master").unwrap();
    let p1 = repo
        .get_bonsai_bookmark(ctx.clone(), &bookmark)
        .await
        .unwrap()
        .unwrap();

    let content = FileContents::new_bytes(Bytes::from(b"999" as &[u8]));
    let content_id = content
        .into_blob()
        .store(&ctx, repo.blobstore())
        .await
        .unwrap();
    let file_change = FileChange::tracked(content_id, FileType::Regular, 3, None);

    let bcs = BonsaiChangesetMut {
        parents: vec![p1],
        author: "Test User <test@fb.com>".to_string(),
        author_date: DateTime::from_timestamp(1504040002, 0).unwrap(),
        committer: None,
        committer_date: None,
        message: "Change linear/1".to_string(),
        extra: Default::default(),
        file_changes: sorted_vector_map! {mpath("linear/1") => file_change},
        is_snapshot: false,
    }
    .freeze()
    .unwrap();

    let bcs_id = bcs.get_changeset_id();
    save_bonsai_changesets(vec![bcs], ctx.clone(), repo)
        .await
        .unwrap();

    let mut txn = repo.update_bookmark_transaction(ctx.clone());
    txn.force_set(&bookmark, bcs_id, BookmarkUpdateReason::TestMove)
        .unwrap();
    txn.commit().await.unwrap();

    bcs_id
}

#[fbinit::test]
async fn test_sync_parent_search(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let (small_repo, megarepo, mapping) = prepare_repos_and_mapping(fb)?;
    Linear::initrepo(fb, &small_repo).await;
    let linear = small_repo;

    let config = create_small_to_large_commit_syncer(
        &ctx,
        linear.clone(),
        megarepo.clone(),
        "linear",
        mapping.clone(),
    )?;
    let reverse_config = create_large_to_small_commit_syncer(
        &ctx,
        linear.clone(),
        megarepo.clone(),
        "linear",
        mapping,
    )?;

    create_initial_commit(ctx.clone(), &megarepo).await;

    // Take 2d7d4ba9ce0a6ffd222de7785b249ead9c51c536 from linear, and rewrite it as a child of master
    let linear_base_bcs_id = get_bcs_id(
        &ctx,
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

async fn check_rewritten_multiple<M>(
    ctx: &CoreContext,
    syncer: &CommitSyncer<M>,
    cs_id: ChangesetId,
    expected_rewrite_count: usize,
) -> Result<(), Error>
where
    M: SyncedCommitMapping + Clone + 'static,
{
    let plural_commit_sync_outcome = syncer
        .get_plural_commit_sync_outcome(ctx, cs_id)
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
    let (small_repo, megarepo, mapping) = prepare_repos_and_mapping(fb).unwrap();
    Linear::initrepo(fb, &small_repo).await;
    let small_to_large_syncer = create_small_to_large_commit_syncer(
        &ctx,
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
    let version = version_name_with_small_repo();
    mapping
        .add(
            &ctx,
            SyncedCommitMappingEntry::new(
                megarepo.get_repoid(),
                megarepo_master_cs_id,
                small_repo.get_repoid(),
                small_repo_master_cs_id,
                version.clone(),
                small_to_large_syncer.get_source_repo_type(),
            ),
        )
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
        .load(&ctx, small_repo.blobstore())
        .await?;
    small_to_large_syncer
        .unsafe_sync_commit_pushrebase(
            &ctx,
            small_cs.clone(),
            BookmarkName::new("master").unwrap(),
            Target(megarepo_lca_hint.clone()),
            CommitSyncContext::Tests,
        )
        .await
        .expect("sync should have succeeded");

    small_to_large_syncer
        .unsafe_sync_commit_pushrebase(
            &ctx,
            small_cs.clone(),
            BookmarkName::new("other_branch").unwrap(),
            Target(megarepo_lca_hint.clone()),
            CommitSyncContext::Tests,
        )
        .await
        .expect("sync should have succeeded");

    // 3. Sanity-check that the small repo master is indeed rewritten
    // into two different commits in the large repo
    check_rewritten_multiple(&ctx, &small_to_large_syncer, small_repo_master_cs_id, 2).await?;

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

#[fbinit::test]
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
        .unsafe_sync_commit(
            &ctx,
            to_sync,
            CandidateSelectionHint::Only,
            CommitSyncContext::Tests,
        )
        .await
        .expect_err("sync should have failed");
    assert!(format!("{:?}", e).contains("Too many rewritten candidates for"));

    // Can sync with a bookmark-based hint
    let book = Target(BookmarkName::new("master").unwrap());
    let lca_hint: Target<Arc<dyn LeastCommonAncestorsHint>> =
        Target(Arc::new(SkiplistIndex::new()));
    small_to_large_syncer
        .unsafe_sync_commit(
            &ctx,
            to_sync,
            CandidateSelectionHint::OnlyOrAncestorOfBookmark(
                book,
                Target(megarepo.clone()),
                lca_hint,
            ),
            CommitSyncContext::Tests,
        )
        .await
        .expect("sync should have succeeded");

    Ok(())
}

#[fbinit::test]
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
    let to_sync = to_sync_id.load(&ctx, small_repo.blobstore()).await?;

    let lca_hint: Target<Arc<dyn LeastCommonAncestorsHint>> =
        Target(Arc::new(SkiplistIndex::new()));
    small_to_large_syncer
        .unsafe_sync_commit_pushrebase(
            &ctx,
            to_sync,
            BookmarkName::new("master").unwrap(),
            lca_hint,
            CommitSyncContext::Tests,
        )
        .await
        .expect("sync should have succeeded");

    Ok(())
}

#[fbinit::test]
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
    let to_sync = to_sync_id.load(&ctx, small_repo.blobstore()).await?;

    let lca_hint: Target<Arc<dyn LeastCommonAncestorsHint>> =
        Target(Arc::new(SkiplistIndex::new()));
    small_to_large_syncer
        .unsafe_sync_commit_pushrebase(
            &ctx,
            to_sync,
            BookmarkName::new("master").unwrap(),
            lca_hint,
            CommitSyncContext::Tests,
        )
        .await
        .expect("sync should have succeeded");

    Ok(())
}

#[fbinit::test]
async fn test_sync_with_mapping_change(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let (old_version, new_version, large_to_small_syncer) =
        prepare_commit_syncer_with_mapping_change(fb).await?;
    let megarepo = large_to_small_syncer.get_source_repo();
    let small_repo = large_to_small_syncer.get_target_repo();

    let new_mapping_large_cs_id = resolve_cs_id(&ctx, &megarepo, "new_mapping").await?;
    // Create a new commit on top of commit with new mapping.
    let new_mapping_cs_id =
        CreateCommitContext::new(&ctx, &megarepo, vec![new_mapping_large_cs_id])
            .add_file("tools/newtool", "1")
            .delete_file("tools/1.txt")
            .add_file("tools/somefile", "somefile1")
            .add_file("prefix/dir/file", "3")
            .add_file("prefix/dir/newfile", "3")
            .commit()
            .await?;

    let synced = large_to_small_syncer
        .sync_commit(
            &ctx,
            new_mapping_cs_id,
            CandidateSelectionHint::Only,
            CommitSyncContext::Tests,
        )
        .await?;
    assert!(synced.is_some());
    let new_mapping_small_cs_id = synced.unwrap();

    verify_working_copy(
        ctx.clone(),
        large_to_small_syncer.clone(),
        new_mapping_cs_id,
    )
    .await?;
    assert_working_copy(
        &ctx,
        small_repo,
        new_mapping_small_cs_id,
        vec!["tools/somefile", "tools/newtool", "dir/file", "dir/newfile"],
    )
    .await?;

    let outcome = large_to_small_syncer
        .get_commit_sync_outcome(&ctx, new_mapping_cs_id)
        .await?;

    match outcome {
        Some(CommitSyncOutcome::RewrittenAs(_, version)) => {
            assert_eq!(version, new_version);
        }
        _ => {
            return Err(anyhow!("unexpected outcome: {:?}", outcome));
        }
    }

    // Create a new commit on top of commit with old mapping.

    let old_mapping_large_cs_id = resolve_cs_id(&ctx, &megarepo, "old_mapping").await?;
    let old_mapping_cs_id =
        CreateCommitContext::new(&ctx, &megarepo, vec![old_mapping_large_cs_id])
            .add_file("tools/3.txt", "2")
            .add_file("prefix/file", "2")
            .commit()
            .await?;
    let synced = large_to_small_syncer
        .sync_commit(
            &ctx,
            old_mapping_cs_id,
            CandidateSelectionHint::Only,
            CommitSyncContext::Tests,
        )
        .await?;
    assert!(synced.is_some());
    let old_mapping_small_cs_id = synced.unwrap();

    verify_working_copy(
        ctx.clone(),
        large_to_small_syncer.clone(),
        old_mapping_cs_id,
    )
    .await?;
    assert_working_copy(
        &ctx,
        small_repo,
        old_mapping_small_cs_id,
        vec!["dir/file", "file", "tools/1.txt"],
    )
    .await?;

    let outcome = large_to_small_syncer
        .get_commit_sync_outcome(&ctx, old_mapping_cs_id)
        .await?;

    match outcome {
        Some(CommitSyncOutcome::RewrittenAs(_, version)) => {
            assert_eq!(version, old_version);
        }
        _ => {
            return Err(anyhow!("unexpected outcome: {:?}", outcome));
        }
    }
    Ok(())
}

#[fbinit::test]
async fn test_sync_equivalent_wc_with_mapping_change(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let (old_version, new_version, large_to_small_syncer) =
        prepare_commit_syncer_with_mapping_change(fb).await?;
    let megarepo = large_to_small_syncer.get_source_repo();
    let small_repo = large_to_small_syncer.get_target_repo();

    println!("create commits with new mapping");
    let new_mapping_large_cs_id = resolve_cs_id(&ctx, &megarepo, "new_mapping").await?;
    // Create a stack of commits on top of commit with new mapping.
    // First commit should not rewrite into a small repo, but second should

    let does_not_rewrite_large_cs_id =
        CreateCommitContext::new(&ctx, &megarepo, vec![new_mapping_large_cs_id])
            .add_file("somerandomfile", "1")
            .commit()
            .await?;

    let synced = large_to_small_syncer
        .sync_commit(
            &ctx,
            does_not_rewrite_large_cs_id,
            CandidateSelectionHint::Only,
            CommitSyncContext::Tests,
        )
        .await?;
    let parent_synced = large_to_small_syncer
        .sync_commit(
            &ctx,
            new_mapping_large_cs_id,
            CandidateSelectionHint::Only,
            CommitSyncContext::Tests,
        )
        .await?;
    // does_not_rewrite_large_cs_id commit was rewritten out, so sync_commit
    // should return the same changeset id as the parent
    assert_eq!(synced, parent_synced);

    let new_mapping_cs_id =
        CreateCommitContext::new(&ctx, &megarepo, vec![does_not_rewrite_large_cs_id])
            .add_file("tools/newtool", "1")
            .delete_file("tools/1.txt")
            .add_file("tools/somefile", "somefile1")
            .add_file("prefix/dir/file", "3")
            .add_file("prefix/dir/newfile", "3")
            .commit()
            .await?;

    let synced = large_to_small_syncer
        .sync_commit(
            &ctx,
            new_mapping_cs_id,
            CandidateSelectionHint::Only,
            CommitSyncContext::Tests,
        )
        .await?;
    assert!(synced.is_some());
    let new_mapping_small_cs_id = synced.unwrap();

    verify_working_copy(
        ctx.clone(),
        large_to_small_syncer.clone(),
        new_mapping_cs_id,
    )
    .await?;
    assert_working_copy(
        &ctx,
        small_repo,
        new_mapping_small_cs_id,
        vec!["tools/somefile", "tools/newtool", "dir/file", "dir/newfile"],
    )
    .await?;

    let outcome = large_to_small_syncer
        .get_commit_sync_outcome(&ctx, new_mapping_cs_id)
        .await?;

    match outcome {
        Some(CommitSyncOutcome::RewrittenAs(_, version)) => {
            assert_eq!(version, new_version);
        }
        _ => {
            return Err(anyhow!("unexpected outcome: {:?}", outcome));
        }
    }

    // Create a new commit on top of commit with old mapping.
    println!("create commits with old mapping");

    let old_mapping_large_cs_id = resolve_cs_id(&ctx, &megarepo, "old_mapping").await?;
    let does_not_rewrite_large_cs_id =
        CreateCommitContext::new(&ctx, &megarepo, vec![old_mapping_large_cs_id])
            .add_file("somerandomfile", "1")
            .commit()
            .await?;

    let old_mapping_cs_id =
        CreateCommitContext::new(&ctx, &megarepo, vec![does_not_rewrite_large_cs_id])
            .add_file("tools/3.txt", "2")
            .add_file("prefix/file", "2")
            .commit()
            .await?;
    let synced = large_to_small_syncer
        .sync_commit(
            &ctx,
            old_mapping_cs_id,
            CandidateSelectionHint::Only,
            CommitSyncContext::Tests,
        )
        .await?;
    assert!(synced.is_some());
    let old_mapping_small_cs_id = synced.unwrap();

    verify_working_copy(
        ctx.clone(),
        large_to_small_syncer.clone(),
        old_mapping_cs_id,
    )
    .await?;
    assert_working_copy(
        &ctx,
        small_repo,
        old_mapping_small_cs_id,
        vec!["dir/file", "file", "tools/1.txt"],
    )
    .await?;

    let outcome = large_to_small_syncer
        .get_commit_sync_outcome(&ctx, old_mapping_cs_id)
        .await?;

    match outcome {
        Some(CommitSyncOutcome::RewrittenAs(_, version)) => {
            assert_eq!(version, old_version);
        }
        _ => {
            return Err(anyhow!("unexpected outcome: {:?}", outcome));
        }
    }
    Ok(())
}

#[fbinit::test]
async fn test_disabled_sync(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let (_, _, large_to_small_syncer) = prepare_commit_syncer_with_mapping_change(fb).await?;
    let megarepo = large_to_small_syncer.get_source_repo();

    let new_mapping_large_cs_id = resolve_cs_id(&ctx, &megarepo, "new_mapping").await?;
    // Create a new commit on top of commit with new mapping.
    let new_mapping_cs_id =
        CreateCommitContext::new(&ctx, &megarepo, vec![new_mapping_large_cs_id])
            .add_file("tools/newtool", "1")
            .commit()
            .await?;

    let tunables = MononokeTunables::default();
    tunables.update_bools(&hashmap! {"xrepo_sync_disable_all_syncs".to_string() => true});

    // Disable sync - make sure it fails
    let res = with_tunables_async(
        tunables,
        async {
            large_to_small_syncer
                .sync_commit(
                    &ctx,
                    new_mapping_cs_id,
                    CandidateSelectionHint::Only,
                    CommitSyncContext::Tests,
                )
                .await
        }
        .boxed(),
    )
    .await;

    match res {
        Ok(_) => Err(anyhow!("unexpected success")),
        Err(err) => {
            check_x_repo_sync_disabled(&err);
            Ok(())
        }
    }
}

#[fbinit::test]
async fn test_disabled_sync_pushrebase(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let (small_repo, megarepo, mapping) = prepare_repos_and_mapping(fb).unwrap();
    Linear::initrepo(fb, &small_repo).await;
    let small_to_large_syncer = create_small_to_large_commit_syncer(
        &ctx,
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
    let version = version_name_with_small_repo();
    mapping
        .add(
            &ctx,
            SyncedCommitMappingEntry::new(
                megarepo.get_repoid(),
                megarepo_master_cs_id,
                small_repo.get_repoid(),
                small_repo_master_cs_id,
                version.clone(),
                small_to_large_syncer.get_source_repo_type(),
            ),
        )
        .await?;

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
        .load(&ctx, small_repo.blobstore())
        .await?;

    let tunables = MononokeTunables::default();
    tunables.update_bools(&hashmap! {"xrepo_sync_disable_all_syncs".to_string() => true});

    // Disable sync - make sure it fails
    let res = with_tunables_async(
        tunables,
        async {
            small_to_large_syncer
                .unsafe_sync_commit_pushrebase(
                    &ctx,
                    small_cs.clone(),
                    BookmarkName::new("master").unwrap(),
                    Target(megarepo_lca_hint.clone()),
                    CommitSyncContext::Tests,
                )
                .await
        }
        .boxed(),
    )
    .await;

    match res {
        Ok(_) => Err(anyhow!("unexpected success")),
        Err(err) => match err.downcast_ref::<ErrorKind>() {
            Some(error_kind) => match error_kind {
                ErrorKind::PushrebaseFailure(error) => match error {
                    PushrebaseError::Error(err) => {
                        check_x_repo_sync_disabled(err);
                        Ok(())
                    }
                    _ => Err(anyhow!("unexpected pushrebase error: {}", error)),
                },
                _ => Err(anyhow!("unexpected ErrorKind: {}", error_kind)),
            },
            None => Err(anyhow!("unexpected error - not ErrorKind")),
        },
    }
}

fn check_x_repo_sync_disabled(err: &Error) {
    assert_matches!(
        err.downcast_ref::<ErrorKind>(),
        Some(ErrorKind::XRepoSyncDisabled)
    );
}

async fn prepare_commit_syncer_with_mapping_change(
    fb: FacebookInit,
) -> Result<
    (
        CommitSyncConfigVersion,
        CommitSyncConfigVersion,
        CommitSyncer<SqlSyncedCommitMapping>,
    ),
    Error,
> {
    let ctx = CoreContext::test_mock(fb);
    let (small_repo, megarepo, mapping) = prepare_repos_and_mapping(fb)?;
    let (large_to_small_syncer, config_source) =
        create_large_to_small_commit_syncer_and_config_source(
            &ctx,
            small_repo.clone(),
            megarepo.clone(),
            "prefix",
            mapping,
        )?;

    let root_cs_id = CreateCommitContext::new_root(&ctx, &megarepo)
        .add_file("tools/somefile", "somefile")
        .add_file("prefix/tools/1.txt", "1")
        .add_file("prefix/dir/file", "2")
        .commit()
        .await?;

    bookmark(&ctx, &megarepo, "old_mapping")
        .set_to(root_cs_id)
        .await?;

    let maybe_small_root_cs_id = large_to_small_syncer
        .unsafe_always_rewrite_sync_commit(
            &ctx,
            root_cs_id,
            None,
            &version_name_with_small_repo(),
            CommitSyncContext::Tests,
        )
        .await?;
    assert!(maybe_small_root_cs_id.is_some());
    let small_root_cs_id = maybe_small_root_cs_id.unwrap();

    verify_working_copy(ctx.clone(), large_to_small_syncer.clone(), root_cs_id).await?;
    assert_working_copy(
        &ctx,
        &small_repo,
        small_root_cs_id,
        vec!["tools/1.txt", "dir/file"],
    )
    .await?;

    // Change the mapping - "tools" now doesn't change it's location after remapping!

    let small_repo_id = small_repo.get_repoid();
    let large_repo_id = megarepo.get_repoid();
    let small_repo_config = SmallRepoCommitSyncConfig {
        default_action: DefaultSmallToLargeCommitSyncPathAction::PrependPrefix(MPath::new(
            "prefix",
        )?),
        map: hashmap! {
            MPath::new("tools")? => MPath::new("tools")?,
        },
    };

    let old_version = CommitSyncConfigVersion("TEST_VERSION_NAME".to_string());
    let new_version = CommitSyncConfigVersion("TEST_VERSION_NAME2".to_string());
    let commit_sync_config = CommitSyncConfig {
        large_repo_id,
        common_pushrebase_bookmarks: vec![],
        small_repos: hashmap! {
            small_repo_id => small_repo_config,
        },
        version_name: new_version.clone(),
    };
    config_source.add_common_config(CommonCommitSyncConfig {
        common_pushrebase_bookmarks: vec![],
        small_repos: hashmap! {
            small_repo.get_repoid() => SmallRepoPermanentConfig {
                bookmark_prefix: AsciiString::new(),
            }
        },
        large_repo_id,
    });
    config_source.add_config(commit_sync_config);

    // Create manual commit to change mapping
    let new_mapping_large_cs_id = CreateCommitContext::new(&ctx, &megarepo, vec![root_cs_id])
        .delete_file("prefix/tools/1.txt")
        .add_file("tools/1.txt", "1")
        .commit()
        .await?;

    bookmark(&ctx, &megarepo, "new_mapping")
        .set_to(new_mapping_large_cs_id)
        .await?;

    let new_mapping_small_cs_id =
        CreateCommitContext::new(&ctx, &small_repo, vec![small_root_cs_id])
            .add_file("tools/somefile", "somefile")
            .commit()
            .await?;

    update_mapping_with_version(
        &ctx,
        hashmap! {new_mapping_large_cs_id => new_mapping_small_cs_id},
        &large_to_small_syncer,
        &new_version,
    )
    .await?;

    verify_working_copy(
        ctx.clone(),
        large_to_small_syncer.clone(),
        new_mapping_large_cs_id,
    )
    .await?;
    assert_working_copy(
        &ctx,
        &small_repo,
        new_mapping_small_cs_id,
        vec!["tools/1.txt", "tools/somefile", "dir/file"],
    )
    .await?;

    Ok((old_version, new_version, large_to_small_syncer))
}

/// Build a test CommitSyncDataProvider for merge
/// testing purposes.
fn get_merge_sync_data_provider(
    large_repo_id: RepositoryId,
    small_repo_id: RepositoryId,
) -> Result<CommitSyncDataProvider, Error> {
    let v1 = CommitSyncConfigVersion("v1".to_string());
    let v2 = CommitSyncConfigVersion("v2".to_string());

    let small_repo_config = SmallRepoCommitSyncConfig {
        default_action: DefaultSmallToLargeCommitSyncPathAction::Preserve,
        map: hashmap! {},
    };
    let commit_sync_config_v1 = CommitSyncConfig {
        large_repo_id,
        common_pushrebase_bookmarks: vec![BookmarkName::new("master")?],
        small_repos: hashmap! {
            small_repo_id => small_repo_config.clone(),
        },
        version_name: v1,
    };
    let commit_sync_config_v2 = CommitSyncConfig {
        large_repo_id,
        common_pushrebase_bookmarks: vec![BookmarkName::new("master")?],
        small_repos: hashmap! {
            small_repo_id => small_repo_config,
        },
        version_name: v2,
    };

    let common_config = CommonCommitSyncConfig {
        common_pushrebase_bookmarks: vec![BookmarkName::new("master")?],
        small_repos: hashmap! {
            small_repo_id => SmallRepoPermanentConfig {
                bookmark_prefix: AsciiString::new(),
            }
        },
        large_repo_id,
    };

    let (sync_config, source) = TestLiveCommitSyncConfig::new_with_source();
    source.add_config(commit_sync_config_v1);
    source.add_config(commit_sync_config_v2);
    source.add_common_config(common_config);

    let live_commit_sync_config = Arc::new(sync_config);

    Ok(CommitSyncDataProvider::Live(live_commit_sync_config))
}

/// This function sets up scene for syncing merges
/// Main goal is to return mergeable commits in the large repo
/// with a convenient grouping of which versions were used to
/// sync these commits to the small repo.
/// More concretely, this function returns:
/// - a context object
/// - a large-to-small syncer
/// - a map of version-to-changesets-list, where all the changesets
///   in the list are synced with that mapping
async fn merge_test_setup(
    fb: FacebookInit,
) -> Result<
    (
        CoreContext,
        CommitSyncer<SqlSyncedCommitMapping>,
        HashMap<Option<CommitSyncConfigVersion>, Vec<ChangesetId>>,
    ),
    Error,
> {
    let ctx = CoreContext::test_mock(fb);
    // Set up various structures
    let mut factory = TestRepoFactory::new(fb)?;
    let large_repo: BlobRepo = factory.with_id(RepositoryId::new(0)).build()?;
    let small_repo: BlobRepo = factory.with_id(RepositoryId::new(1)).build()?;
    let mapping = SqlSyncedCommitMapping::with_sqlite_in_memory()?;
    let v1 = CommitSyncConfigVersion("v1".to_string());
    let v2 = CommitSyncConfigVersion("v2".to_string());

    let lts_syncer = {
        let mut lts_syncer = create_large_to_small_commit_syncer(
            &ctx,
            small_repo.clone(),
            large_repo.clone(),
            // This is ignored
            "_",
            mapping.clone(),
        )?;
        lts_syncer.repos = CommitSyncRepos::LargeToSmall {
            small_repo: small_repo.clone(),
            large_repo: large_repo.clone(),
        };
        lts_syncer.commit_sync_data_provider =
            get_merge_sync_data_provider(large_repo.get_repoid(), small_repo.get_repoid())?;
        lts_syncer
    };

    let c1 = create_initial_commit_with_contents(
        ctx.clone(),
        &large_repo,
        btreemap! { "f1" => Some(b"1" as &[u8]) },
    )
    .await;
    let c2 = create_initial_commit_with_contents(
        ctx.clone(),
        &large_repo,
        btreemap! { "f2" => Some(b"2" as &[u8]) },
    )
    .await;
    let c3 = create_initial_commit_with_contents(
        ctx.clone(),
        &large_repo,
        btreemap! { "f3" => Some(b"3" as &[u8]) },
    )
    .await;
    let c4 = create_initial_commit_with_contents(
        ctx.clone(),
        &large_repo,
        btreemap! { "f4" => Some(b"4" as &[u8]) },
    )
    .await;

    lts_syncer
        .unsafe_always_rewrite_sync_commit(
            &ctx,
            c1,
            None, // parents override
            &v1,
            CommitSyncContext::Tests,
        )
        .await?;
    lts_syncer
        .unsafe_always_rewrite_sync_commit(
            &ctx,
            c2,
            None, // parents override
            &v1,
            CommitSyncContext::Tests,
        )
        .await?;
    lts_syncer
        .unsafe_always_rewrite_sync_commit(
            &ctx,
            c3,
            None, // parents override
            &v2,
            CommitSyncContext::Tests,
        )
        .await?;
    lts_syncer
        .unsafe_always_rewrite_sync_commit(
            &ctx,
            c4,
            None, // parents override
            &v2,
            CommitSyncContext::Tests,
        )
        .await?;

    let heads_with_versions = hashmap! {
        Some(v1) => vec![c1, c2],
        Some(v2) => vec![c3, c4],
    };

    Ok((ctx, lts_syncer, heads_with_versions))
}

async fn create_merge(
    ctx: &CoreContext,
    repo: &BlobRepo,
    parents: Vec<ChangesetId>,
) -> ChangesetId {
    let bcs = BonsaiChangesetMut {
        parents,
        author: "Test User <test@fb.com>".to_string(),
        author_date: DateTime::from_timestamp(1504040001, 0).unwrap(),
        committer: None,
        committer_date: None,
        message: "Never gonna give you up".to_string(),
        extra: Default::default(),
        file_changes: Default::default(),
        is_snapshot: false,
    }
    .freeze()
    .unwrap();

    let bcs_id = bcs.get_changeset_id();
    save_bonsai_changesets(vec![bcs], ctx.clone(), repo)
        .await
        .unwrap();

    bcs_id
}

#[fbinit::test]
async fn test_sync_merge_gets_version_from_parents_1(fb: FacebookInit) -> Result<(), Error> {
    let v1 = CommitSyncConfigVersion("v1".to_string());
    let (ctx, lts_syncer, heads_with_versions) = merge_test_setup(fb).await?;
    let heads = heads_with_versions[&Some(v1.clone())].clone();
    let merge_bcs_id = create_merge(&ctx, lts_syncer.get_source_repo(), heads).await;
    println!(
        "merge sync outcome: {:?}",
        lts_syncer
            .sync_commit(
                &ctx,
                merge_bcs_id,
                CandidateSelectionHint::Only,
                CommitSyncContext::Tests
            )
            .await?
    );
    let outcome = lts_syncer
        .get_commit_sync_outcome(&ctx, merge_bcs_id)
        .await?
        .expect("merge syncing outcome is missing");
    if let CommitSyncOutcome::RewrittenAs(_, merge_version) = outcome {
        assert_eq!(v1, merge_version);
    } else {
        panic!(
            "unexpected outcome after syncing a merge commit: {:?}",
            outcome
        );
    }
    Ok(())
}

#[fbinit::test]
async fn test_sync_merge_gets_version_from_parents_2(fb: FacebookInit) -> Result<(), Error> {
    let v2 = CommitSyncConfigVersion("v2".to_string());
    let (ctx, lts_syncer, heads_with_versions) = merge_test_setup(fb).await?;
    let heads = heads_with_versions[&Some(v2.clone())].clone();
    let merge_bcs_id = create_merge(&ctx, lts_syncer.get_source_repo(), heads).await;
    lts_syncer
        .sync_commit(
            &ctx,
            merge_bcs_id,
            CandidateSelectionHint::Only,
            CommitSyncContext::Tests,
        )
        .await?
        .unwrap();
    let outcome = lts_syncer
        .get_commit_sync_outcome(&ctx, merge_bcs_id)
        .await?
        .expect("merge syncing outcome is missing");
    if let CommitSyncOutcome::RewrittenAs(_, merge_version) = outcome {
        assert_eq!(v2, merge_version);
    } else {
        panic!(
            "unexpected outcome after syncing a merge commit: {:?}",
            outcome
        );
    }
    Ok(())
}

#[fbinit::test]
async fn test_sync_merge_fails_when_parents_have_different_versions(
    fb: FacebookInit,
) -> Result<(), Error> {
    let v1 = CommitSyncConfigVersion("v1".to_string());
    let v2 = CommitSyncConfigVersion("v2".to_string());
    let (ctx, lts_syncer, heads_with_versions) = merge_test_setup(fb).await?;
    let heads_0 = heads_with_versions[&Some(v1)].clone();
    let heads_1 = heads_with_versions[&Some(v2)].clone();
    let merge_heads = [heads_0[0], heads_1[0]].to_vec();
    let merge_bcs_id = create_merge(&ctx, lts_syncer.get_source_repo(), merge_heads).await;
    let e = lts_syncer
        .sync_commit(
            &ctx,
            merge_bcs_id,
            CandidateSelectionHint::Only,
            CommitSyncContext::Tests,
        )
        .await
        .expect_err("syncing a merge with differently-remapped parents must fail");
    assert!(format!("{}", e).contains("failed getting a mover to use for merge rewriting"));
    Ok(())
}

async fn assert_working_copy(
    ctx: &CoreContext,
    repo: &BlobRepo,
    cs_id: ChangesetId,
    expected_files: Vec<&str>,
) -> Result<(), Error> {
    let hg_cs_id = repo.derive_hg_changeset(ctx, cs_id).await?;

    let hg_cs = hg_cs_id.load(ctx, repo.blobstore()).await?;
    let mf_id = hg_cs.manifestid();
    let mut actual_paths = mf_id
        .list_leaf_entries(ctx.clone(), repo.get_blobstore())
        .map_ok(|(path, _)| path)
        .try_collect::<Vec<_>>()
        .await?;
    actual_paths.sort();

    let expected_paths: Result<Vec<_>, Error> =
        expected_files.into_iter().map(MPath::new).collect();
    let mut expected_paths = expected_paths?;
    expected_paths.sort();

    assert_eq!(actual_paths, expected_paths);
    Ok(())
}

async fn test_no_accidental_preserved_roots(
    ctx: CoreContext,
    commit_sync_repos: CommitSyncRepos,
    mapping: SqlSyncedCommitMapping,
) -> Result<(), Error> {
    let version = version_name_with_small_repo();
    let commit_syncer = {
        use CommitSyncRepos::*;
        let mut commit_syncer = match &commit_sync_repos {
            LargeToSmall {
                small_repo,
                large_repo,
            } => create_large_to_small_commit_syncer(
                &ctx,
                small_repo.clone(),
                large_repo.clone(),
                "ignored",
                mapping.clone(),
            )?,
            SmallToLarge {
                small_repo,
                large_repo,
            } => create_small_to_large_commit_syncer(
                &ctx,
                small_repo.clone(),
                large_repo.clone(),
                "ignored",
                mapping.clone(),
            )?,
        };

        let small_repo_config = SmallRepoCommitSyncConfig {
            default_action: DefaultSmallToLargeCommitSyncPathAction::Preserve,
            map: hashmap! {},
        };
        let commit_sync_config = CommitSyncConfig {
            large_repo_id: commit_syncer.get_large_repo().get_repoid(),
            common_pushrebase_bookmarks: vec![BookmarkName::new("master")?],
            small_repos: hashmap! {
                commit_syncer.get_small_repo().get_repoid() => small_repo_config,
            },
            version_name: version.clone(),
        };

        let common_config = CommonCommitSyncConfig {
            common_pushrebase_bookmarks: vec![BookmarkName::new("master")?],
            small_repos: hashmap! {
                commit_syncer.get_small_repo().get_repoid() => SmallRepoPermanentConfig {
                    bookmark_prefix: AsciiString::new(),
                }
            },
            large_repo_id: commit_syncer.get_large_repo().get_repoid(),
        };

        let (sync_config, source) = TestLiveCommitSyncConfig::new_with_source();
        source.add_config(commit_sync_config);
        source.add_common_config(common_config);

        let live_commit_sync_config = Arc::new(sync_config);

        let commit_sync_data_provider = CommitSyncDataProvider::Live(live_commit_sync_config);
        commit_syncer.commit_sync_data_provider = commit_sync_data_provider;
        commit_syncer.repos = commit_sync_repos.clone();

        commit_syncer
    };

    let root_commit = create_initial_commit(ctx.clone(), commit_sync_repos.get_source_repo()).await;
    commit_syncer
        .unsafe_sync_commit_with_expected_version(
            &ctx,
            root_commit,
            CandidateSelectionHint::Only,
            CommitSyncConfigVersion("TEST_VERSION_NAME".to_string()),
            CommitSyncContext::Tests,
        )
        .await?;
    let outcome = commit_syncer
        .get_commit_sync_outcome(&ctx, root_commit)
        .await?;
    assert!(matches!(outcome, Some(CommitSyncOutcome::RewrittenAs(_, v)) if v == version));

    Ok(())
}

#[fbinit::test]
async fn test_no_accidental_preserved_roots_large_to_small(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let (small_repo, large_repo, mapping) = prepare_repos_and_mapping(fb).unwrap();
    let commit_sync_repos = CommitSyncRepos::LargeToSmall {
        small_repo: small_repo.clone(),
        large_repo: large_repo.clone(),
    };
    test_no_accidental_preserved_roots(ctx, commit_sync_repos, mapping).await
}

#[fbinit::test]
async fn test_no_accidental_preserved_roots_small_to_large(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let (small_repo, large_repo, mapping) = prepare_repos_and_mapping(fb).unwrap();
    let commit_sync_repos = CommitSyncRepos::SmallToLarge {
        small_repo: small_repo.clone(),
        large_repo: large_repo.clone(),
    };
    test_no_accidental_preserved_roots(ctx, commit_sync_repos, mapping).await
}

#[fbinit::test]
async fn test_not_sync_candidate_if_mapping_does_not_have_small_repo(
    fb: FacebookInit,
) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let mut factory = TestRepoFactory::new(fb)?;
    let mapping =
        SqlSyncedCommitMapping::from_sql_connections(factory.metadata_db().clone().into());

    let large_repo_id = RepositoryId::new(0);
    let large_repo: BlobRepo = factory.with_id(large_repo_id).build()?;
    let first_small_repo_id = RepositoryId::new(1);
    let first_smallrepo: BlobRepo = factory.with_id(first_small_repo_id).build()?;
    let second_small_repo_id = RepositoryId::new(2);
    let second_smallrepo: BlobRepo = factory.with_id(second_small_repo_id).build()?;

    let (sync_config, source) = TestLiveCommitSyncConfig::new_with_source();

    // First create common config that have two small repos in it
    source.add_common_config(CommonCommitSyncConfig {
        common_pushrebase_bookmarks: vec![BookmarkName::new("master")?],
        small_repos: hashmap! {
            first_small_repo_id => SmallRepoPermanentConfig {
                bookmark_prefix: AsciiString::new(),
            },
            second_small_repo_id => SmallRepoPermanentConfig {
                bookmark_prefix: AsciiString::new(),
            },
        },
        large_repo_id,
    });

    // Then create config version that has only a first config repo
    let noop_version_first_small_repo = CommitSyncConfigVersion("noop_first".to_string());
    let noop_first_version_config = CommitSyncConfig {
        large_repo_id,
        common_pushrebase_bookmarks: vec![BookmarkName::new("master")?],
        small_repos: hashmap! {
            first_small_repo_id => SmallRepoCommitSyncConfig {
                default_action: DefaultSmallToLargeCommitSyncPathAction::Preserve,
                map: hashmap! {},
            },
        },
        version_name: noop_version_first_small_repo.clone(),
    };
    source.add_config(noop_first_version_config);

    // Now create commit in large repo and sync it to the first small repo with the config
    // created above.
    let commit_sync_data_provider = CommitSyncDataProvider::Live(Arc::new(sync_config));
    let repos = CommitSyncRepos::LargeToSmall {
        small_repo: first_smallrepo.clone(),
        large_repo: large_repo.clone(),
    };
    let large_to_first_small_commit_syncer = CommitSyncer::new_with_provider(
        &ctx,
        mapping.clone(),
        repos.clone(),
        commit_sync_data_provider.clone(),
    );

    let first_bcs_id = CreateCommitContext::new_root(&ctx, &large_repo)
        .add_file("file", "content")
        .commit()
        .await?;
    large_to_first_small_commit_syncer
        .unsafe_always_rewrite_sync_commit(
            &ctx,
            first_bcs_id,
            None, // parents override
            &noop_version_first_small_repo,
            CommitSyncContext::Tests,
        )
        .await?;

    // Now try to sync it to the other small repo, it should return NotSyncCandidate
    let repos = CommitSyncRepos::LargeToSmall {
        small_repo: second_smallrepo.clone(),
        large_repo: large_repo.clone(),
    };
    let large_to_second_small_commit_syncer = CommitSyncer::new_with_provider(
        &ctx,
        mapping.clone(),
        repos.clone(),
        commit_sync_data_provider.clone(),
    );
    large_to_second_small_commit_syncer
        .sync_commit(
            &ctx,
            first_bcs_id,
            CandidateSelectionHint::Only,
            CommitSyncContext::Tests,
        )
        .await?;

    assert_eq!(
        large_to_second_small_commit_syncer
            .get_commit_sync_outcome(&ctx, first_bcs_id)
            .await?,
        Some(CommitSyncOutcome::NotSyncCandidate(
            noop_version_first_small_repo
        ))
    );
    Ok(())
}
