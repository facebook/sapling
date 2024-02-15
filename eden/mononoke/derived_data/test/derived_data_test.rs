/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;
use std::time::Duration;

use anyhow::anyhow;
use anyhow::Error;
use anyhow::Result;
use async_trait::async_trait;
use blobstore::Blobstore;
use blobstore::BlobstoreBytes;
use bonsai_hg_mapping::BonsaiHgMapping;
use bookmarks::BookmarkKey;
use bookmarks::Bookmarks;
use bookmarks::BookmarksRef;
use bytes::Bytes;
use cacheblob::LeaseOps;
use changeset_fetcher::ChangesetFetcher;
use changesets::Changesets;
use changesets::ChangesetsRef;
use context::CoreContext;
use derived_data_manager::BonsaiDerivable;
use fbinit::FacebookInit;
use filestore::FilestoreConfig;
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
use futures_stats::TimedTryFutureExt;
use generation_derivation::make_test_repo_factory;
use generation_derivation::DerivedGeneration;
use mononoke_types::NonRootMPath;
use mononoke_types::RepositoryId;
use repo_blobstore::RepoBlobstore;
use repo_blobstore::RepoBlobstoreRef;
use repo_derived_data::RepoDerivedData;
use repo_derived_data::RepoDerivedDataRef;
use repo_identity::RepoIdentity;
use repo_identity::RepoIdentityRef;
use tests_utils::CreateCommitContext;

#[facet::container]
#[derive(Clone)]
struct TestRepo {
    #[facet]
    bonsai_hg_mapping: dyn BonsaiHgMapping,
    #[facet]
    bookmarks: dyn Bookmarks,
    #[facet]
    changesets: dyn Changesets,
    #[facet]
    changeset_fetcher: dyn ChangesetFetcher,
    #[facet]
    filestore_config: FilestoreConfig,
    #[facet]
    repo_blobstore: RepoBlobstore,
    #[facet]
    repo_identity: RepoIdentity,
    #[facet]
    repo_derived_data: RepoDerivedData,
}

async fn derive_for_master(
    ctx: &CoreContext,
    repo: &(impl BookmarksRef + ChangesetsRef + RepoDerivedDataRef),
) -> Result<()> {
    let master = repo
        .bookmarks()
        .get(ctx.clone(), &BookmarkKey::new("master")?)
        .await?
        .expect("master should be set");
    let expected = repo
        .changesets()
        .get(ctx, master)
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

    let repo: TestRepo = factory.with_id(RepositoryId::new(1)).build().await?;
    BranchEven::initrepo(fb, &repo).await;
    derive_for_master(&ctx, &repo).await?;

    let repo: TestRepo = factory.with_id(RepositoryId::new(2)).build().await?;
    BranchUneven::initrepo(fb, &repo).await;
    derive_for_master(&ctx, &repo).await?;

    let repo: TestRepo = factory.with_id(RepositoryId::new(3)).build().await?;
    BranchWide::initrepo(fb, &repo).await;
    derive_for_master(&ctx, &repo).await?;

    let repo: TestRepo = factory.with_id(RepositoryId::new(4)).build().await?;
    Linear::initrepo(fb, &repo).await;
    derive_for_master(&ctx, &repo).await?;

    let repo: TestRepo = factory.with_id(RepositoryId::new(5)).build().await?;
    ManyFilesDirs::initrepo(fb, &repo).await;
    derive_for_master(&ctx, &repo).await?;

    let repo: TestRepo = factory.with_id(RepositoryId::new(6)).build().await?;
    MergeEven::initrepo(fb, &repo).await;
    derive_for_master(&ctx, &repo).await?;

    let repo: TestRepo = factory.with_id(RepositoryId::new(7)).build().await?;
    MergeUneven::initrepo(fb, &repo).await;
    derive_for_master(&ctx, &repo).await?;

    let repo: TestRepo = factory.with_id(RepositoryId::new(8)).build().await?;
    UnsharedMergeEven::initrepo(fb, &repo).await;
    derive_for_master(&ctx, &repo).await?;

    let repo: TestRepo = factory.with_id(RepositoryId::new(9)).build().await?;
    UnsharedMergeUneven::initrepo(fb, &repo).await;
    derive_for_master(&ctx, &repo).await?;

    let repo: TestRepo = factory.with_id(RepositoryId::new(10)).build().await?;
    ManyDiamonds::initrepo(fb, &repo).await;
    derive_for_master(&ctx, &repo).await?;

    Ok(())
}

#[fbinit::test]
/// Test that derivation is successful even when there are gaps (i.e. some
/// derived changesets do not have their parents derived).
async fn test_gapped_derivation(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let repo: TestRepo = make_test_repo_factory(fb).build().await?;
    Linear::initrepo(fb, &repo).await;

    let master = repo
        .bookmarks()
        .get(ctx.clone(), &BookmarkKey::new("master")?)
        .await?
        .expect("master should be set");
    let master_anc1 = repo
        .changesets()
        .get(&ctx, master)
        .await?
        .expect("changeset should exist")
        .parents
        .first()
        .expect("changeset should have a parent")
        .clone();
    let master_anc2 = repo
        .changesets()
        .get(&ctx, master_anc1)
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
            format!("test_generation.{}", master_anc1),
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
    let repo: TestRepo = make_test_repo_factory(fb)
        .with_derived_data_lease(|| Arc::new(FailingLease))
        .build()
        .await?;
    Linear::initrepo(fb, &repo).await;

    let master = repo
        .bookmarks()
        .get(ctx.clone(), &BookmarkKey::new("master")?)
        .await?
        .expect("master should be set");

    let lease = repo.repo_derived_data().lease();
    let lease_key = format!(
        "repo{}.{}.{}",
        repo.repo_identity().id(),
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
        .get(&ctx, master)
        .await?
        .expect("changeset should exist")
        .gen;
    assert_eq!(expected, result.generation);

    Ok(())
}

#[fbinit::test]
async fn test_parallel_derivation(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let repo: TestRepo = make_test_repo_factory(fb).build().await?;

    // Create a commit with lots of parents, and make each derivation take
    // 2 seconds.  Check that derivations happen in parallel by ensuring
    // it completes in a shorter time.
    let mut parents = vec![];
    for i in 0..8 {
        let p = CreateCommitContext::new_root(&ctx, &repo)
            .add_file(NonRootMPath::new(format!("file_{}", i))?, format!("{}", i))
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
