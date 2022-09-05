/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::sync::RwLock;

use anyhow::Result;
use async_recursion::async_recursion;
use blobstore::Blobstore;
use blobstore::Loadable;
use changesets::ChangesetsRef;
use context::CoreContext;
use fbinit::FacebookInit;
use fixtures::TestRepoFixture;
use futures::stream;
use futures::TryStreamExt;
use mononoke_types::basename_suffix_skeleton_manifest::BssmDirectory;
use mononoke_types::basename_suffix_skeleton_manifest::BssmEntry;
use mononoke_types::BasenameSuffixSkeletonManifestId;
use mononoke_types::ChangesetIdPrefix;
use mononoke_types::ChangesetIdsResolvedFromPrefix;
use repo_derived_data::RepoDerivedDataRef;

use crate::RootBasenameSuffixSkeletonManifest;

#[async_recursion]
async fn validate(
    cache: &RwLock<HashMap<BasenameSuffixSkeletonManifestId, u64>>,
    ctx: &CoreContext,
    blobstore: &impl Blobstore,
    dir: BssmDirectory,
) -> Result<()> {
    let from_cache = cache.read().unwrap().get(&dir.id).copied();
    let calculated_rollup_count = if let Some(count) = from_cache {
        count
    } else {
        let mf = dir.load(ctx, blobstore).await?;
        let calculated_rollup_count = AtomicU64::new(1);
        let calculated_rollup_count = &calculated_rollup_count;
        mf.into_subentries(ctx, blobstore)
            .try_for_each_concurrent(None, |(path, entry)| async move {
                calculated_rollup_count.fetch_add(entry.rollup_count(), Ordering::Relaxed);
                match entry {
                    BssmEntry::File => assert_eq!(path.as_ref(), b"$"),
                    BssmEntry::Directory(dir) => validate(cache, ctx, blobstore, dir).await?,
                }
                Ok(())
            })
            .await?;
        let count = calculated_rollup_count.load(Ordering::Relaxed);
        cache.write().unwrap().insert(dir.id, count);
        count
    };
    assert_eq!(dir.rollup_count, calculated_rollup_count);
    Ok(())
}

async fn test_for_fixture<F: TestRepoFixture + Send>(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let ctx = &ctx;
    let repo = F::getrepo(fb).await;
    let derived_data = repo.repo_derived_data();
    let blobstore = repo.blobstore();
    let master = repo
        .bookmarks()
        .get(ctx.clone(), &"master".try_into()?)
        .await?
        .unwrap();
    derived_data
        .derive::<RootBasenameSuffixSkeletonManifest>(ctx, master)
        .await?;
    let all_commits = repo
        .changesets()
        .get_many_by_prefix(
            ctx.clone(),
            ChangesetIdPrefix::from_bytes("").unwrap(),
            1000,
        )
        .await?;
    let all_commits = match all_commits {
        ChangesetIdsResolvedFromPrefix::Multiple(all_commits) => all_commits,
        other => anyhow::bail!("Weird number of commits: {:?}", other),
    };
    let cache = RwLock::new(HashMap::new());
    let cache = &cache;
    stream::iter(all_commits.into_iter().map(anyhow::Ok))
        .try_for_each_concurrent(None, |cs_id| async move {
            let mf: RootBasenameSuffixSkeletonManifest = derived_data.derive(ctx, cs_id).await?;
            validate(cache, ctx, blobstore, mf.0).await
        })
        .await?;
    Ok(())
}

#[fbinit::test]
async fn basic_test(fb: FacebookInit) {
    futures::try_join!(
        test_for_fixture::<fixtures::Linear>(fb),
        test_for_fixture::<fixtures::BranchEven>(fb),
        test_for_fixture::<fixtures::BranchUneven>(fb),
        test_for_fixture::<fixtures::BranchWide>(fb),
        test_for_fixture::<fixtures::MergeEven>(fb),
        test_for_fixture::<fixtures::ManyFilesDirs>(fb),
        test_for_fixture::<fixtures::MergeUneven>(fb),
        test_for_fixture::<fixtures::UnsharedMergeEven>(fb),
        test_for_fixture::<fixtures::UnsharedMergeUneven>(fb),
        test_for_fixture::<fixtures::ManyDiamonds>(fb),
    )
    .unwrap();
}
