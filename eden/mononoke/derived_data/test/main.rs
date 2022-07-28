/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

use anyhow::anyhow;
use anyhow::Error;
use anyhow::Result;
use async_trait::async_trait;
use blobrepo::BlobRepo;
use blobstore::Blobstore;
use blobstore::BlobstoreBytes;
use bookmarks::BookmarkName;
use bookmarks::BookmarksRef;
use bytes::Bytes;
use cacheblob::LeaseOps;
use changesets::ChangesetsRef;
use cloned::cloned;
use context::CoreContext;
use fbinit::FacebookInit;
use fixtures::BranchEven;
use fixtures::BranchUneven;
use fixtures::BranchWide;
use fixtures::Linear;
use fixtures::ManyDiamonds;
use fixtures::ManyFilesDirs;
use fixtures::MergeEven;
use fixtures::MergeUneven;
use fixtures::TestRepoFixture;
use fixtures::UnsharedMergeEven;
use fixtures::UnsharedMergeUneven;
use futures::future::BoxFuture;
use futures_stats::TimedFutureExt;
use futures_stats::TimedTryFutureExt;
use lock_ext::LockExt;
use maplit::hashmap;
use mononoke_types::ChangesetId;
use mononoke_types::MPath;
use mononoke_types::RepositoryId;
use repo_blobstore::RepoBlobstoreRef;
use repo_derived_data::RepoDerivedDataArc;
use repo_derived_data::RepoDerivedDataRef;
use tests_utils::CreateCommitContext;
use tunables::override_tunables;
use tunables::MononokeTunables;

use derived_data_manager::BonsaiDerivable;
use derived_data_manager::DerivationError;
use derived_data_test_derived_generation::make_test_repo_factory;
use derived_data_test_derived_generation::DerivedGeneration;

async fn derive_for_master(
    ctx: &CoreContext,
    repo: &(impl BookmarksRef + ChangesetsRef + RepoDerivedDataRef),
) -> Result<()> {
    let master = repo
        .bookmarks()
        .get(ctx.clone(), &BookmarkName::new("master")?)
        .await?
        .expect("master should be set");
    let expected = repo
        .changesets()
        .get(ctx.clone(), master)
        .await?
        .expect("changeset should exist")
        .gen;

    let derived = repo
        .repo_derived_data()
        .derive::<DerivedGeneration>(ctx, master)
        .await?;

    assert_eq!(expected, derived.generation);

    Ok(())
}

#[fbinit::test]
async fn test_derive_fixtures(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let mut factory = make_test_repo_factory(fb);

    let repo: BlobRepo = factory.with_id(RepositoryId::new(1)).build()?;
    BranchEven::initrepo(fb, &repo).await;
    derive_for_master(&ctx, &repo).await?;

    let repo: BlobRepo = factory.with_id(RepositoryId::new(2)).build()?;
    BranchUneven::initrepo(fb, &repo).await;
    derive_for_master(&ctx, &repo).await?;

    let repo: BlobRepo = factory.with_id(RepositoryId::new(3)).build()?;
    BranchWide::initrepo(fb, &repo).await;
    derive_for_master(&ctx, &repo).await?;

    let repo: BlobRepo = factory.with_id(RepositoryId::new(4)).build()?;
    Linear::initrepo(fb, &repo).await;
    derive_for_master(&ctx, &repo).await?;

    let repo: BlobRepo = factory.with_id(RepositoryId::new(5)).build()?;
    ManyFilesDirs::initrepo(fb, &repo).await;
    derive_for_master(&ctx, &repo).await?;

    let repo: BlobRepo = factory.with_id(RepositoryId::new(6)).build()?;
    MergeEven::initrepo(fb, &repo).await;
    derive_for_master(&ctx, &repo).await?;

    let repo: BlobRepo = factory.with_id(RepositoryId::new(7)).build()?;
    MergeUneven::initrepo(fb, &repo).await;
    derive_for_master(&ctx, &repo).await?;

    let repo: BlobRepo = factory.with_id(RepositoryId::new(8)).build()?;
    UnsharedMergeEven::initrepo(fb, &repo).await;
    derive_for_master(&ctx, &repo).await?;

    let repo: BlobRepo = factory.with_id(RepositoryId::new(9)).build()?;
    UnsharedMergeUneven::initrepo(fb, &repo).await;
    derive_for_master(&ctx, &repo).await?;

    let repo: BlobRepo = factory.with_id(RepositoryId::new(10)).build()?;
    ManyDiamonds::initrepo(fb, &repo).await;
    derive_for_master(&ctx, &repo).await?;

    Ok(())
}

#[fbinit::test]
/// Test that derivation is successful even when there are gaps (i.e. some
/// derived changesets do not have their parents derived).
async fn test_gapped_derivation(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let repo: BlobRepo = make_test_repo_factory(fb).build()?;
    Linear::initrepo(fb, &repo).await;

    let master = repo
        .bookmarks()
        .get(ctx.clone(), &BookmarkName::new("master")?)
        .await?
        .expect("master should be set");
    let master_anc1 = repo
        .changesets()
        .get(ctx.clone(), master)
        .await?
        .expect("changeset should exist")
        .parents
        .first()
        .expect("changeset should have a parent")
        .clone();
    let master_anc2 = repo
        .changesets()
        .get(ctx.clone(), master_anc1)
        .await?
        .expect("changeset should exist")
        .parents
        .first()
        .expect("changeset should have a parent")
        .clone();
    // Insert a derived entry for the first ancestor changeset.  We will
    // deliberately use a different value than is expected.
    repo.repo_blobstore()
        .put(
            &ctx,
            format!("repo0.test_generation.{}", master_anc1),
            BlobstoreBytes::from_bytes(Bytes::from_static(b"41")),
        )
        .await?;

    // Derivation was based on that different value.
    let derived = repo
        .repo_derived_data()
        .derive::<DerivedGeneration>(&ctx, master)
        .await?;

    assert_eq!(42, derived.generation);

    // The other ancestors were not derived.
    let derived_anc2 = repo
        .repo_derived_data()
        .fetch_derived::<DerivedGeneration>(&ctx, master_anc2)
        .await?;
    assert!(derived_anc2.is_none());

    Ok(())
}

#[fbinit::test]
async fn test_leases(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let repo: BlobRepo = make_test_repo_factory(fb).build()?;
    Linear::initrepo(fb, &repo).await;

    let master = repo
        .bookmarks()
        .get(ctx.clone(), &BookmarkName::new("master")?)
        .await?
        .expect("master should be set");

    let lease = repo.repo_derived_data().lease();
    let lease_key = format!(
        "repo{}.{}.{}",
        repo.get_repoid().id(),
        DerivedGeneration::NAME,
        master
    );

    // take lease
    assert!(lease.try_add_put_lease(&lease_key).await?);
    assert!(!lease.try_add_put_lease(&lease_key).await?);

    let output = Arc::new(Mutex::new(None));
    tokio::spawn({
        cloned!(ctx, repo, output);
        async move {
            let result = repo
                .repo_derived_data()
                .derive::<DerivedGeneration>(&ctx, master)
                .await;
            output.with(move |output| {
                let _ = output.insert(result);
            });
        }
    });

    // Let the derivation process get started, however derivation won't
    // happen yet.
    tokio::time::sleep(Duration::from_millis(300)).await;

    assert!(
        repo.repo_derived_data()
            .fetch_derived::<DerivedGeneration>(&ctx, master)
            .await?
            .is_none()
    );

    // Release the lease, allowing derivation to proceed.
    lease.release_lease(&lease_key).await;
    tokio::time::sleep(Duration::from_millis(3000)).await;

    let expected = repo
        .changesets()
        .get(ctx.clone(), master)
        .await?
        .expect("changeset should exist")
        .gen;
    let result = output
        .with(|output| output.take())
        .expect("scheduled derivation should have completed")?;
    assert_eq!(expected, result.generation,);

    // Take the lease again.
    assert!(lease.try_add_put_lease(&lease_key).await?);

    // This time it should succeed, as the lease won't be necessary.
    let result = repo
        .repo_derived_data()
        .derive::<DerivedGeneration>(&ctx, master)
        .await?;
    assert_eq!(expected, result.generation);
    lease.release_lease(&lease_key).await;
    Ok(())
}

#[derive(Debug)]
struct FailingLease;

impl std::fmt::Display for FailingLease {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "FailingLease")
    }
}

#[async_trait]
impl LeaseOps for FailingLease {
    async fn try_add_put_lease(&self, _key: &str) -> Result<bool> {
        Err(anyhow!("error"))
    }

    fn renew_lease_until(&self, _ctx: CoreContext, _key: &str, _done: BoxFuture<'static, ()>) {}

    async fn wait_for_other_leases(&self, _key: &str) {}

    async fn release_lease(&self, _key: &str) {}
}

#[fbinit::test]
async fn test_always_failing_lease(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let repo: BlobRepo = make_test_repo_factory(fb)
        .with_derived_data_lease(|| Arc::new(FailingLease))
        .build()?;
    Linear::initrepo(fb, &repo).await;

    let master = repo
        .bookmarks()
        .get(ctx.clone(), &BookmarkName::new("master")?)
        .await?
        .expect("master should be set");

    let lease = repo.repo_derived_data().lease();
    let lease_key = format!(
        "repo{}.{}.{}",
        repo.get_repoid().id(),
        DerivedGeneration::NAME,
        master,
    );

    // Taking the lease should fail
    assert!(lease.try_add_put_lease(&lease_key).await.is_err());

    // Derivation should succeed even though lease always fails
    let result = repo
        .repo_derived_data()
        .derive::<DerivedGeneration>(&ctx, master)
        .await?;
    let expected = repo
        .changesets()
        .get(ctx.clone(), master)
        .await?
        .expect("changeset should exist")
        .gen;
    assert_eq!(expected, result.generation);

    Ok(())
}

#[fbinit::test]
async fn test_parallel_derivation(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let repo: BlobRepo = make_test_repo_factory(fb).build()?;

    // Create a commit with lots of parents, and make each derivation take
    // 2 seconds.  Check that derivations happen in parallel by ensuring
    // it completes in a shorter time.
    let mut parents = vec![];
    for i in 0..8 {
        let p = CreateCommitContext::new_root(&ctx, &repo)
            .add_file(MPath::new(format!("file_{}", i))?, format!("{}", i))
            .add_extra("test-derive-delay", "2")
            .commit()
            .await?;
        parents.push(p);
    }

    let merge = CreateCommitContext::new(&ctx, &repo, parents)
        .add_extra("test-derive-delay", "2")
        .commit()
        .await?;

    let (stats, _res) = repo
        .repo_derived_data()
        .derive::<DerivedGeneration>(&ctx, merge)
        .try_timed()
        .await?;

    assert!(stats.completion_time > Duration::from_secs(2));
    assert!(stats.completion_time < Duration::from_secs(10));

    Ok(())
}

async fn ensure_tunables_disable_derivation(
    ctx: &CoreContext,
    repo: &impl RepoDerivedDataArc,
    csid: ChangesetId,
    tunables: MononokeTunables,
) -> Result<(), Error> {
    let spawned_derivation = tokio::spawn({
        let ctx = ctx.clone();
        let repo_derived_data = repo.repo_derived_data_arc();
        async move {
            repo_derived_data
                .derive::<DerivedGeneration>(&ctx, csid)
                .timed()
                .await
        }
    });
    tokio::time::sleep(Duration::from_millis(1000)).await;

    override_tunables(Some(Arc::new(tunables)));

    let (stats, res) = spawned_derivation.await?;

    assert!(matches!(res, Err(DerivationError::Disabled(..))));

    eprintln!("derivation cancelled after {:?}", stats.completion_time);
    assert!(stats.completion_time < Duration::from_secs(15));

    Ok(())
}

#[fbinit::test]
/// Test that very slow derivation can be cancelled mid-flight by setting
/// the appropriate values in tunables.
async fn test_cancelling_slow_derivation(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let repo: BlobRepo = make_test_repo_factory(fb).build()?;

    let create_tunables = || {
        let tunables = MononokeTunables::default();
        // Make delay smaller so that the test runs faster
        tunables.update_ints(&hashmap! {
            "derived_data_disabled_watcher_delay_secs".to_string() => 1,
        });
        tunables
    };

    let commit = CreateCommitContext::new_root(&ctx, &repo)
        .add_file(MPath::new("file")?, "content")
        .add_extra("test-derive-delay", "20")
        .commit()
        .await?;

    // Reset tunables.
    override_tunables(Some(Arc::new(create_tunables())));

    // Disable derived data for all types
    let tunables_to_disable_all = create_tunables();
    tunables_to_disable_all.update_by_repo_bools(&hashmap! {
        repo.name().to_string() => hashmap! {
            "all_derived_data_disabled".to_string() => true,
        },
    });
    ensure_tunables_disable_derivation(&ctx, &repo, commit, tunables_to_disable_all).await?;

    // Reset tunables.
    override_tunables(Some(Arc::new(create_tunables())));

    // Disable derived data for a single type
    let tunables_to_disable_by_type = create_tunables();
    tunables_to_disable_by_type.update_by_repo_vec_of_strings(&hashmap! {
        repo.name().to_string() => hashmap! {
            "derived_data_types_disabled".to_string() => vec![DerivedGeneration::NAME.to_string()],
        },
    });
    ensure_tunables_disable_derivation(&ctx, &repo, commit, tunables_to_disable_by_type).await?;

    Ok(())
}
