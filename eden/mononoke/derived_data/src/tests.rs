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
use blame::RootBlameV3;
use bonsai_hg_mapping::BonsaiHgMapping;
use bookmarks::Bookmarks;
use changeset_info::ChangesetInfo;
use commit_graph::CommitGraph;
use commit_graph::CommitGraphWriter;
use context::CoreContext;
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
