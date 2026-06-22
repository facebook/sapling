/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Repro/benchmark for the `derived_data_use_content_manifests` SEV.
//!
//! It builds a base changeset with a single very large directory, then a child
//! changeset that modifies just a handful of files in that directory. It then
//! diffs the base against the child using BOTH fsnodes and content_manifests,
//! via the same generic `filtered_diff` / `filtered_diff_ordered` entry points
//! used by `commit_compare` in the SCS server (`ManifestOps::diff` and
//! `ManifestOrderedOps::diff_ordered`).
//!
//! The blobstore is wrapped in a counting layer so we report, for each diff:
//!   - number of result entries (identical across manifest types),
//!   - number of blobstore `get`s,
//!   - total bytes deserialized,
//!   - wall-clock time.
//!
//! The point it highlights: a tiny diff over a large directory costs O(1) blob
//! loads with fsnodes (the directory is a single flat blob) but O(sharded-map
//! nodes) blob loads with content_manifests, because the generic diff prunes
//! only at the directory-id level and re-enumerates each changed directory's
//! whole ShardedMapV2 via `list`/`lookup` -- on both sides -- rather than
//! skipping identical sub-shards by id.
//!
//! Run with two optional positional args: <total_files> <modify_count>
//!   buck2 run //eden/mononoke/benchmarks/derived_data:benchmark_manifest_diff
//!   buck2 run //eden/mononoke/benchmarks/derived_data:benchmark_manifest_diff -- 200000 5

use std::collections::BTreeSet;
use std::fmt;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;

use anyhow::Result;
use async_trait::async_trait;
use blobstore::BlobstoreBytes;
use blobstore::BlobstoreGetData;
use blobstore::BlobstoreIsPresent;
use blobstore::KeyedBlobstore;
use bonsai_hg_mapping::BonsaiHgMapping;
use bookmarks::Bookmarks;
use commit_graph::CommitGraph;
use commit_graph::CommitGraphWriter;
use content_manifest_derivation::RootContentManifestId;
use context::CoreContext;
use derivation_queue_thrift::DerivationPriority;
use fbinit::FacebookInit;
use filestore::FilestoreConfig;
use fsnodes::RootFsnodeId;
use futures::StreamExt;
use futures::TryStreamExt;
use futures::future;
use futures::stream::BoxStream;
use futures_stats::TimedFutureExt;
use manifest::ManifestOps;
use manifest::ManifestOrderedOps;
use mononoke_types::ChangesetId;
use mononoke_types::ContentManifestId;
use mononoke_types::FsnodeId;
use rand::Rng;
use rand::RngExt as _;
use rand::distr::Alphanumeric;
use rand::distr::Uniform;
use repo_blobstore::RepoBlobstore;
use repo_blobstore::RepoBlobstoreRef;
use repo_derived_data::RepoDerivedData;
use repo_derived_data::RepoDerivedDataRef;
use repo_identity::RepoIdentity;
use tests_utils::CreateCommitContext;

#[facet::container]
#[derive(Clone)]
struct Repo {
    #[facet]
    repo_identity: RepoIdentity,

    #[facet]
    repo_blobstore: RepoBlobstore,

    #[facet]
    repo_derived_data: RepoDerivedData,

    #[facet]
    bonsai_hg_mapping: dyn BonsaiHgMapping,

    #[facet]
    bookmarks: dyn Bookmarks,

    #[facet]
    commit_graph: CommitGraph,

    #[facet]
    commit_graph_writer: dyn CommitGraphWriter,

    #[facet]
    filestore_config: FilestoreConfig,
}

/// A `Blobstore` wrapper that counts `get` calls and bytes returned. Every
/// clone shares the same counters via `Arc`, so wrapping in `Arc<_>` (which is
/// what the diff entry points need: `Store: Clone + Send + Sync + 'static`)
/// keeps a single shared tally.
struct CountingBlobstore {
    inner: RepoBlobstore,
    gets: Arc<AtomicU64>,
    bytes: Arc<AtomicU64>,
}

impl fmt::Debug for CountingBlobstore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "CountingBlobstore({:?})", self.inner)
    }
}

impl fmt::Display for CountingBlobstore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "CountingBlobstore({})", self.inner)
    }
}

#[async_trait]
impl KeyedBlobstore for CountingBlobstore {
    async fn get<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: &'a str,
    ) -> Result<Option<BlobstoreGetData>> {
        let res = self.inner.get(ctx, key).await?;
        self.gets.fetch_add(1, Ordering::Relaxed);
        if let Some(data) = &res {
            self.bytes.fetch_add(data.len() as u64, Ordering::Relaxed);
        }
        Ok(res)
    }

    async fn put<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: String,
        value: BlobstoreBytes,
    ) -> Result<()> {
        self.inner.put(ctx, key, value).await
    }

    async fn is_present<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: &'a str,
    ) -> Result<BlobstoreIsPresent> {
        self.inner.is_present(ctx, key).await
    }

    async fn copy<'a>(
        &'a self,
        ctx: &'a CoreContext,
        old_key: &'a str,
        new_key: String,
    ) -> Result<()> {
        self.inner.copy(ctx, old_key, new_key).await
    }

    async fn unlink<'a>(&'a self, ctx: &'a CoreContext, key: &'a str) -> Result<()> {
        self.inner.unlink(ctx, key).await
    }
}

fn gen_filename(rng: &mut impl Rng, len: usize) -> String {
    std::iter::repeat_with(|| rng.sample(Alphanumeric))
        .take(len)
        .map(char::from)
        .collect()
}

/// Build a root changeset with `count` files all in one large directory.
/// Returns the changeset id and the sorted list of file paths created.
async fn make_base_commit(
    ctx: &CoreContext,
    repo: &Repo,
    count: usize,
) -> Result<(ChangesetId, Vec<String>)> {
    let mut rng = rand::rng();
    let len_distr = Uniform::new(5, 50).unwrap();
    let mut filenames = BTreeSet::new();
    while filenames.len() < count {
        let len = rng.sample(len_distr);
        filenames.insert(gen_filename(&mut rng, len));
    }

    let paths: Vec<String> = filenames
        .into_iter()
        .map(|name| format!("large_directory/{name}"))
        .collect();

    let mut create = CreateCommitContext::new_root(ctx, repo);
    for path in &paths {
        create = create.add_file(path.as_str(), format!("content of {path}"));
    }
    let csid = create.commit().await?;
    Ok((csid, paths))
}

/// Build a child of `parent` that modifies the first `modify_count` files.
async fn make_child_commit(
    ctx: &CoreContext,
    repo: &Repo,
    parent: ChangesetId,
    paths: &[String],
    modify_count: usize,
) -> Result<ChangesetId> {
    let modify_count = modify_count.min(paths.len());
    let mut create = CreateCommitContext::new(ctx, repo, vec![parent]);
    for path in &paths[..modify_count] {
        create = create.add_file(path.as_str(), format!("modified content of {path}"));
    }
    create.commit().await
}

async fn derive_fsnode(ctx: &CoreContext, repo: &Repo, csid: ChangesetId) -> Result<FsnodeId> {
    Ok(*repo
        .repo_derived_data()
        .derive::<RootFsnodeId>(ctx, csid, DerivationPriority::LOW)
        .await?
        .fsnode_id())
}

async fn derive_content_manifest(
    ctx: &CoreContext,
    repo: &Repo,
    csid: ChangesetId,
) -> Result<ContentManifestId> {
    Ok(repo
        .repo_derived_data()
        .derive::<RootContentManifestId>(ctx, csid, DerivationPriority::LOW)
        .await?
        .into_content_manifest_id())
}

/// Drain a diff stream, counting entries, while measuring blob gets and bytes.
/// `gets`/`bytes` are reset before draining so the numbers are per-diff.
async fn measure(
    label: &str,
    gets: &AtomicU64,
    bytes: &AtomicU64,
    mut stream: BoxStream<'static, Result<()>>,
) -> Result<()> {
    gets.store(0, Ordering::Relaxed);
    bytes.store(0, Ordering::Relaxed);

    let (stats, count) = async {
        let mut entries = 0u64;
        while let Some(item) = stream.next().await {
            item?;
            entries += 1;
        }
        anyhow::Ok(entries)
    }
    .timed()
    .await;
    let count = count?;

    println!(
        "{label:<30} entries={count:<7} blob_gets={:<8} bytes={:<11} time={:?}",
        gets.load(Ordering::Relaxed),
        bytes.load(Ordering::Relaxed),
        stats.completion_time,
    );
    Ok(())
}

#[fbinit::main]
async fn main(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);

    let mut args = std::env::args().skip(1);
    let total_files: usize = args.next().and_then(|s| s.parse().ok()).unwrap_or(100_000);
    let modify_count: usize = args.next().and_then(|s| s.parse().ok()).unwrap_or(10);

    println!("Building repo: {total_files} files in one directory, modifying {modify_count}");
    let repo: Repo = test_repo_factory::build_empty(ctx.fb).await?;

    let (base, paths) = make_base_commit(&ctx, &repo, total_files).await?;
    let child = make_child_commit(&ctx, &repo, base, &paths, modify_count).await?;
    println!("Base:  {base}");
    println!("Child: {child}");

    let (base_fsnode, child_fsnode) = future::try_join(
        derive_fsnode(&ctx, &repo, base),
        derive_fsnode(&ctx, &repo, child),
    )
    .await?;
    let (base_cm, child_cm) = future::try_join(
        derive_content_manifest(&ctx, &repo, base),
        derive_content_manifest(&ctx, &repo, child),
    )
    .await?;

    let gets = Arc::new(AtomicU64::new(0));
    let bytes = Arc::new(AtomicU64::new(0));
    let store = Arc::new(CountingBlobstore {
        inner: repo.repo_blobstore().clone(),
        gets: gets.clone(),
        bytes: bytes.clone(),
    });

    println!(
        "\nDiffing base vs child (a {modify_count}-file change in a {total_files}-file dir):\n"
    );

    // fsnode, unordered (ManifestOps::diff -> filtered_diff)
    measure(
        "fsnode  diff (unordered)",
        &gets,
        &bytes,
        base_fsnode
            .diff(ctx.clone(), store.clone(), child_fsnode)
            .map_ok(|_| ())
            .boxed(),
    )
    .await?;

    // fsnode, ordered (ManifestOrderedOps::diff_ordered -> filtered_diff_ordered)
    measure(
        "fsnode  diff (ordered)",
        &gets,
        &bytes,
        base_fsnode
            .diff_ordered(ctx.clone(), store.clone(), child_fsnode, None)
            .map_ok(|_| ())
            .boxed(),
    )
    .await?;

    // content_manifest, unordered
    measure(
        "content diff (unordered)",
        &gets,
        &bytes,
        base_cm
            .diff(ctx.clone(), store.clone(), child_cm)
            .map_ok(|_| ())
            .boxed(),
    )
    .await?;

    // content_manifest, ordered
    measure(
        "content diff (ordered)",
        &gets,
        &bytes,
        base_cm
            .diff_ordered(ctx.clone(), store.clone(), child_cm, None)
            .map_ok(|_| ())
            .boxed(),
    )
    .await?;

    Ok(())
}
