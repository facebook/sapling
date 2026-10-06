/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

use anyhow::Result;
use async_trait::async_trait;
use basename_suffix_skeleton_manifest_v3::RootBssmV3DirectoryId;
use blame::RootBlameV3;
use blobstore::Loadable;
use bonsai_hg_mapping::BonsaiHgMapping;
use bookmarks::Bookmarks;
use changeset_info::ChangesetInfo;
use commit_graph::CommitGraph;
use commit_graph::CommitGraphWriter;
use context::CoreContext;
use deleted_manifest::RootDeletedManifestV2Id;
use derived_data_manager::BonsaiDerivable;
use derived_data_manager::DerivedDataManager;
use fastlog::RootFastlogV2;
use fbinit::FacebookInit;
use filestore::FilestoreConfig;
use futures::FutureExt;
use history_manifest::RootHistoryManifestDirectoryId;
use justknobs::test_helpers::JustKnobsInMemory;
use justknobs::test_helpers::KnobVal;
use justknobs::test_helpers::with_just_knobs_async;
use maplit::hashmap;
use mononoke_macros::mononoke;
use mononoke_types::ChangesetId;
use repo_blobstore::RepoBlobstore;
use repo_blobstore::RepoBlobstoreRef;
use repo_derived_data::RepoDerivedData;
use repo_derived_data::RepoDerivedDataRef;
use repo_identity::RepoIdentity;
use sql::rusqlite::Connection as SqliteConnection;
use sql::sqlite::SqliteCallbacks;
use sql::sqlite::SqliteQueryType;
use test_repo_factory::TestRepoFactory;
use tests_utils::CreateCommitContext;

#[facet::container]
struct TestRepo(
    dyn BonsaiHgMapping,
    dyn Bookmarks,
    CommitGraph,
    dyn CommitGraphWriter,
    RepoDerivedData,
    RepoBlobstore,
    FilestoreConfig,
    RepoIdentity,
);

#[derive(Debug)]
struct SqlWriteCounter(Arc<AtomicUsize>);

#[async_trait]
impl SqliteCallbacks for SqlWriteCounter {
    async fn query_start(&self, query_type: SqliteQueryType) -> Result<()> {
        if matches!(query_type, SqliteQueryType::Write) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
        Ok(())
    }
}

async fn check_mapping_batch<T: BonsaiDerivable + Eq>(
    ctx: &CoreContext,
    manager: &DerivedDataManager,
    csids: &[ChangesetId],
    writes: &AtomicUsize,
) -> Result<(HashMap<ChangesetId, T>, usize)> {
    let derivation_ctx = manager.derivation_context(None);
    let before = writes.load(Ordering::SeqCst);
    T::store_mapping_batch(ctx, &derivation_ctx, Vec::new()).await?;
    assert_eq!(writes.load(Ordering::SeqCst), before, "empty batch");

    manager
        .derive_exactly_batch::<T>(ctx, csids.to_vec(), None)
        .await?;
    let derivation_writes = writes.load(Ordering::SeqCst) - before;
    let mappings = manager
        .fetch_derived_batch::<T>(ctx, csids.to_vec(), None)
        .await?;
    assert_eq!(mappings.len(), csids.len(), "all mappings must be readable");

    let retry = mappings
        .iter()
        .cycle()
        .take(1001)
        .map(|(csid, mapping)| (*csid, mapping.clone()))
        .collect();
    let before = writes.load(Ordering::SeqCst);
    T::store_mapping_batch(ctx, &derivation_ctx, retry).await?;
    assert_eq!(
        writes.load(Ordering::SeqCst) - before,
        2,
        "{} should split 1,001 mappings into two SQL writes",
        T::NAME,
    );
    assert_eq!(
        manager
            .fetch_derived_batch::<T>(ctx, csids.to_vec(), None)
            .await?,
        mappings,
        "retrying the batch must preserve existing mappings",
    );
    Ok((mappings, derivation_writes))
}

async fn check_xdb_mapping_batches(fb: FacebookInit, batching_enabled: bool) -> Result<()> {
    with_just_knobs_async(
        JustKnobsInMemory::new(hashmap! {
            "scm/mononoke:derived_data_use_store_mapping_batch".to_owned() => KnobVal::Bool(batching_enabled),
        }),
        async {
            let ctx = CoreContext::test_mock(fb);
            let writes = Arc::new(AtomicUsize::new(0));
            let repo: TestRepo = TestRepoFactory::with_sqlite_connection_callbacks(
                fb,
                SqliteConnection::open_in_memory()?,
                SqliteConnection::open_in_memory()?,
                Some(Box::new(SqlWriteCounter(writes.clone()))),
            )?
            .build()
            .await?;

            let mut csids = Vec::new();
            for i in 0..20 {
                let parents = csids.last().copied().into_iter().collect::<Vec<_>>();
                let csid = CreateCommitContext::new(&ctx, &repo, parents)
                    .add_file("file", format!("revision {i}\n"))
                    .commit()
                    .await?;
                csids.push(csid);
            }

            let manager = repo.repo_derived_data().manager();
            let (history, history_writes) = check_mapping_batch::<RootHistoryManifestDirectoryId>(
                &ctx, manager, &csids, &writes,
            )
            .await?;
            let (blame, blame_writes) =
                check_mapping_batch::<RootBlameV3>(&ctx, manager, &csids, &writes).await?;
            let (fastlog, fastlog_writes) =
                check_mapping_batch::<RootFastlogV2>(&ctx, manager, &csids, &writes).await?;
            let expected_writes = if batching_enabled { 1 } else { csids.len() };
            for (name, actual_writes) in [
                (RootHistoryManifestDirectoryId::NAME, history_writes),
                (RootBlameV3::NAME, blame_writes),
                (RootFastlogV2::NAME, fastlog_writes),
            ] {
                assert_eq!(actual_writes, expected_writes, "{name} derivation batch");
            }
            for csid in csids {
                assert_eq!(blame[&csid].changeset_id(), csid);
                assert_eq!(blame[&csid].root_manifest(), history[&csid]);
                assert_eq!(fastlog[&csid].changeset_id(), csid);
                assert_eq!(fastlog[&csid].root_manifest(), history[&csid]);
            }
            Ok(())
        }
        .boxed(),
    )
    .await
}

#[mononoke::fbinit_test]
async fn test_xdb_mapping_batches(fb: FacebookInit) -> Result<()> {
    check_xdb_mapping_batches(fb, true).await
}

#[mononoke::fbinit_test]
async fn test_xdb_mapping_batches_disabled(fb: FacebookInit) -> Result<()> {
    check_xdb_mapping_batches(fb, false).await
}

#[mononoke::fbinit_test]
async fn test_batch_checks_every_external_merge_parent(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    for parent_count in [2, 3] {
        for missing_parent in 0..parent_count {
            let repo: TestRepo = test_repo_factory::build_empty(fb).await?;
            let mut parents = Vec::new();
            for index in 0..parent_count {
                parents.push(
                    CreateCommitContext::new_root(&ctx, &repo)
                        .add_file(format!("parent{index}").as_str(), "content")
                        .commit()
                        .await?,
                );
            }
            let merge = CreateCommitContext::new(&ctx, &repo, parents.clone())
                .commit()
                .await?;
            let manager = repo.repo_derived_data().manager();
            let available = parents
                .iter()
                .enumerate()
                .filter_map(|(index, csid)| (index != missing_parent).then_some(*csid))
                .collect();
            manager
                .derive_exactly_batch::<ChangesetInfo>(&ctx, available, None)
                .await?;

            // ChangesetInfo does not read parents itself, so this tests the manager.
            let error = manager
                .derive_exactly_batch::<ChangesetInfo>(&ctx, vec![merge], None)
                .await
                .expect_err("every external merge parent must already be derived");
            assert!(format!("{error:#}").contains(&parents[missing_parent].to_string()));
            assert!(
                manager
                    .fetch_derived::<ChangesetInfo>(&ctx, merge, None)
                    .await?
                    .is_none()
            );

            manager
                .derive_exactly_batch::<ChangesetInfo>(
                    &ctx,
                    vec![parents[missing_parent], merge],
                    None,
                )
                .await?;
        }
    }
    Ok(())
}

#[mononoke::fbinit_test]
async fn test_batch_rejects_parent_after_child(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let repo: TestRepo = test_repo_factory::build_empty(fb).await?;
    let first = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("first", "content")
        .commit()
        .await?;
    let second = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("second", "content")
        .commit()
        .await?;
    let merge = CreateCommitContext::new(&ctx, &repo, vec![first, second])
        .commit()
        .await?;
    let manager = repo.repo_derived_data().manager();
    manager
        .derive_exactly_batch::<ChangesetInfo>(&ctx, vec![first, second], None)
        .await?;

    for late_parent in [first, second] {
        let error = manager
            .derive_exactly_batch::<ChangesetInfo>(&ctx, vec![merge, late_parent], None)
            .await
            .expect_err("a persisted parent must still precede its child within a batch");
        assert!(format!("{error:#}").contains("batch not in topological order"));
    }
    manager
        .derive_exactly_batch::<ChangesetInfo>(&ctx, vec![first, second, merge], None)
        .await?;
    Ok(())
}

async fn create_merge_dag(ctx: &CoreContext, repo: &TestRepo) -> Result<Vec<ChangesetId>> {
    // A diamond at D, consecutive merges D -> F -> I, an octopus merge at I,
    // a linear child J, and a second head K sharing ancestry with J.
    let graph: &[(&str, &[usize])] = &[
        ("A", &[]),
        ("B", &[0]),
        ("C", &[0]),
        ("D", &[1, 2]),
        ("E", &[0]),
        ("F", &[3, 4]),
        ("G", &[0]),
        ("H", &[0]),
        ("I", &[5, 6, 7]),
        ("J", &[8]),
        ("K", &[1, 2]),
    ];
    let mut commits = Vec::new();
    for &(name, parents) in graph {
        let mut commit = CreateCommitContext::new(
            ctx,
            repo,
            parents
                .iter()
                .map(|&index| commits[index])
                .collect::<Vec<_>>(),
        )
        .add_file(name, name);
        if matches!(name, "D" | "F" | "I") {
            commit = commit.delete_file("A");
        }
        if name == "J" {
            commit = commit.delete_file("B");
        }
        commits.push(commit.commit().await?);
    }
    Ok(commits)
}

#[mononoke::fbinit_test]
async fn test_merge_batches_share_mapping_flushes(fb: FacebookInit) -> Result<()> {
    with_just_knobs_async(
        JustKnobsInMemory::new(hashmap! {
            "scm/mononoke:derived_data_use_store_mapping_batch".to_owned() => KnobVal::Bool(true),
        }),
        async {
            let ctx = CoreContext::test_mock(fb);
            for batch_size in [1, 2, 4, 20] {
                let writes = Arc::new(AtomicUsize::new(0));
                let repo: TestRepo = TestRepoFactory::with_sqlite_connection_callbacks(
                    fb,
                    SqliteConnection::open_in_memory()?,
                    SqliteConnection::open_in_memory()?,
                    Some(Box::new(SqlWriteCounter(writes.clone()))),
                )?
                .build()
                .await?;
                let commits = create_merge_dag(&ctx, &repo).await?;
                let manager = repo.repo_derived_data().manager();
                // Keep one parent outside the batch and share ancestry across heads.
                manager
                    .derive_exactly_batch::<RootHistoryManifestDirectoryId>(
                        &ctx,
                        vec![commits[0]],
                        None,
                    )
                    .await?;
                let heads = vec![commits[9], commits[10], commits[9]];
                let before = writes.load(Ordering::SeqCst);
                let count = manager
                    .derive_heads::<RootHistoryManifestDirectoryId>(
                        ctx.clone(),
                        heads.clone(),
                        Some(batch_size),
                        None,
                    )
                    .await?;
                assert_eq!(count, 10, "derive each underived commit exactly once");
                // HistoryManifest has no dependent types and writes its mappings
                // once per derivation batch, including the final partial batch.
                assert_eq!(
                    writes.load(Ordering::SeqCst) - before,
                    10_usize.div_ceil(batch_size as usize),
                    "batch size {batch_size}",
                );
                assert_eq!(
                    manager
                        .fetch_derived_batch::<RootHistoryManifestDirectoryId>(&ctx, commits, None,)
                        .await?
                        .len(),
                    11,
                );
                let before = writes.load(Ordering::SeqCst);
                assert_eq!(
                    manager
                        .derive_heads::<RootHistoryManifestDirectoryId>(
                            ctx.clone(),
                            heads,
                            Some(batch_size),
                            None,
                        )
                        .await?,
                    0,
                );
                assert_eq!(writes.load(Ordering::SeqCst), before, "no empty batches");
            }
            Ok(())
        }
        .boxed(),
    )
    .await
}

async fn check_merge_batch_results<T: BonsaiDerivable + Eq>(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let serial_repo: TestRepo = test_repo_factory::build_empty(fb).await?;
    let commits = create_merge_dag(&ctx, &serial_repo).await?;
    let serial = serial_repo.repo_derived_data().manager();
    serial
        .derive_heads::<T>(ctx.clone(), vec![commits[9], commits[10]], Some(1), None)
        .await?;
    let expected = serial
        .fetch_derived_batch::<T>(&ctx, commits.clone(), None)
        .await?;
    assert_eq!(expected.len(), commits.len());

    for batch_size in [2, 4, 20] {
        let repo: TestRepo = test_repo_factory::build_empty(fb).await?;
        assert_eq!(create_merge_dag(&ctx, &repo).await?, commits);
        let manager = repo.repo_derived_data().manager();
        assert_eq!(
            manager
                .derive_heads::<T>(
                    ctx.clone(),
                    vec![commits[9], commits[10]],
                    Some(batch_size),
                    None,
                )
                .await?,
            commits.len() as u64,
        );
        assert_eq!(
            manager
                .fetch_derived_batch::<T>(&ctx, commits.clone(), None)
                .await?,
            expected,
            "{} results with batch size {batch_size}",
            T::NAME,
        );
    }
    Ok(())
}

#[mononoke::fbinit_test]
async fn test_merge_batch_history_results(fb: FacebookInit) -> Result<()> {
    check_merge_batch_results::<RootHistoryManifestDirectoryId>(fb).await
}

#[mononoke::fbinit_test]
async fn test_merge_batch_bssm_results(fb: FacebookInit) -> Result<()> {
    check_merge_batch_results::<RootBssmV3DirectoryId>(fb).await
}

#[mononoke::fbinit_test]
async fn test_merge_batch_deleted_manifest_results(fb: FacebookInit) -> Result<()> {
    check_merge_batch_results::<RootDeletedManifestV2Id>(fb).await
}

#[mononoke::fbinit_test]
async fn test_merge_batches_fill_derivation_gaps(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let repo: TestRepo = test_repo_factory::build_empty(fb).await?;
    let commits = create_merge_dag(&ctx, &repo).await?;
    let manager = repo.repo_derived_data().manager();
    let derivation_ctx = manager.derivation_context(None);
    // Persist D while its parents B and C are underived. Include D as a head
    // so it is a derived frontier, exposing B and C as gap parents of K.
    let derived = commits[3].load(&ctx, repo.repo_blobstore()).await?;
    ChangesetInfo::new(commits[3], derived)
        .store_mapping(&ctx, &derivation_ctx, commits[3])
        .await?;
    assert_eq!(
        manager
            .fetch_derived_batch::<ChangesetInfo>(&ctx, commits.clone(), None)
            .await?
            .len(),
        1,
    );
    assert_eq!(
        manager
            .derive_heads::<ChangesetInfo>(
                ctx.clone(),
                vec![commits[3], commits[10]],
                Some(4),
                None,
            )
            .await?,
        4,
    );
    assert_eq!(
        manager
            .fetch_derived_batch::<ChangesetInfo>(
                &ctx,
                [0, 1, 2, 3, 10].map(|index| commits[index]).to_vec(),
                None,
            )
            .await?
            .len(),
        5,
    );
    Ok(())
}

#[mononoke::fbinit_test]
async fn test_merge_batches_reject_zero_size(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let repo: TestRepo = test_repo_factory::build_empty(fb).await?;
    let error = repo
        .repo_derived_data()
        .manager()
        .derive_heads::<ChangesetInfo>(ctx, Vec::new(), Some(0), None)
        .await
        .expect_err("zero-sized batches cannot make progress");
    assert!(format!("{error:#}").contains("derivation batch size must be greater than zero"));
    Ok(())
}
