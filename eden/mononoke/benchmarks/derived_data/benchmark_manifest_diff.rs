/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Repro/benchmark for the `derived_data_use_content_manifests` diff cost.
//!
//! It builds a repo with one very large flat directory plus a set of medium
//! subdirectories, then runs the manifest operations that back `commit_compare`
//! and `metadata_diff` over BOTH fsnodes and content_manifests, through the same
//! generic entry points the SCS server and diff_service use
//! (`ManifestOps::filtered_diff`, `ManifestOrderedOps::filtered_diff_ordered`
//! and `ManifestOps::find_entry`).
//!
//! The blobstore is wrapped in a counting layer so we report, for each run:
//!   - number of result entries,
//!   - number of blobstore `get`s,
//!   - total bytes deserialized,
//!   - wall-clock time.
//!
//! The number that matters is `get`s. An fsnode directory is a single flat blob
//! whatever its size, so `list` and `lookup` are in-memory once the directory is
//! loaded. A content_manifest directory is a `ShardedMapV2` byte-trie with
//! `WEIGHT_LIMIT = 625`, so a directory of N entries is spread over ~N/625
//! separate blobs: `list` walks all of them and `lookup` costs a trie descent.
//! Content manifests read far fewer BYTES but far more BLOBS, and the blobs are
//! serialized by trie depth -- which is what turns into latency in production.
//!
//! `scm/mononoke:enable_sharding_aware_manifest_diff` (the fix for the
//! `derived_data_use_content_manifests` SEV) makes a *changed* directory prune
//! identical sub-shards by id without loading them. Every scenario below is run
//! with the knob both on and off, so the output shows which cases the fix
//! actually covers. The ones it does not cover are:
//!
//!   * `replacement-in-big` -- a `manifest_replacements` entry (what
//!     `commit_compare --compare-with-subtree-copy-sources` builds from a
//!     subtree copy) disables the fast path for the directory that holds it and
//!     every ancestor, falling back to `diff_manifest_node_by_listing`: a full
//!     `list` of both sides plus a `lookup` per entry, awaited in a serial loop.
//!     `replacement-in-small` is the control -- it shows the blast radius is
//!     exactly the directory holding the replacement.
//!
//!   * `added-subtree` / `removed-subtree` -- the fast path only applies to
//!     `Diff::Changed`; added and removed trees are always enumerated by
//!     listing. `added-subtree-limited` shows that stopping after `limit`
//!     entries does not help, because a directory is fully enumerated inside a
//!     single traversal step before anything is yielded.
//!
//!   * `metadata-lookup-*` -- `metadata_diff` resolves each path independently
//!     via `find_entry`, which does a `Manifest::lookup` per path component. On
//!     fsnodes that is an in-memory binary search; on content manifests it is a
//!     sharded-map trie descent. This path never sees the diff fast path at all.
//!
//! Run with optional positional args:
//!   <total_files> <modify_count> <wide_dirs> <wide_files> <limit>
//!   buck2 run //eden/mononoke/benchmarks/derived_data:benchmark_manifest_diff
//!   buck2 run //eden/mononoke/benchmarks/derived_data:benchmark_manifest_diff -- 200000 5
//!
//! At the default 100k `total_files` the `replacement-in-big` scenario does
//! ~200k sharded-map lookups awaited one at a time, so it takes minutes on the
//! content_manifest side -- that IS the finding, but drop `total_files` to
//! ~10000 for a quick run.

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::time::Duration;

use anyhow::Result;
use anyhow::anyhow;
use async_trait::async_trait;
use blobstore::BlobstoreBytes;
use blobstore::BlobstoreGetData;
use blobstore::BlobstoreIsPresent;
use blobstore::KeyedBlobstore;
use blobstore::StoreLoadable;
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
use futures::stream;
use futures::stream::BoxStream;
use futures_stats::TimedFutureExt;
use justknobs::test_helpers::JustKnobsInMemory;
use justknobs::test_helpers::KnobVal;
use justknobs::test_helpers::with_just_knobs;
use manifest::Entry;
use manifest::Manifest;
use manifest::ManifestOps;
use manifest::ManifestOrderedOps;
use manifest::OrderedManifest;
use manifest::TrieMapOps;
use mononoke_types::ChangesetId;
use mononoke_types::ContentManifestId;
use mononoke_types::FsnodeId;
use mononoke_types::path::MPath;
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

/// Matches the fan-out `metadata_diff` callers use when resolving a batch of
/// paths, so the per-path scenario is not artificially serialized.
const LOOKUP_CONCURRENCY: usize = 100;

const FAST_PATH_KNOB: &str = "scm/mononoke:enable_sharding_aware_manifest_diff";

/// The big flat directory that makes the sharding visible.
const BIG_DIR: &str = "large_directory";
/// A subdirectory of [`BIG_DIR`], used as the site of a manifest replacement.
const BIG_DIR_SUBDIR: &str = "large_directory/subdir";
/// A small directory used as the payload of a manifest replacement.
const REPLACEMENT_SOURCE: &str = "copy_source";

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

type Store = Arc<CountingBlobstore>;

/// Shared handles onto the counting blobstore's tallies.
struct Counters {
    gets: Arc<AtomicU64>,
    bytes: Arc<AtomicU64>,
}

impl Counters {
    fn reset(&self) {
        self.gets.store(0, Ordering::Relaxed);
        self.bytes.store(0, Ordering::Relaxed);
    }

    fn gets(&self) -> u64 {
        self.gets.load(Ordering::Relaxed)
    }

    fn bytes(&self) -> u64 {
        self.bytes.load(Ordering::Relaxed)
    }
}

struct Measurement {
    entries: u64,
    gets: u64,
    bytes: u64,
    time: Duration,
}

/// Drain `stream`, counting entries, while measuring blob gets and bytes.
/// Counters are reset first, so setup done by the caller is not charged to the
/// scenario.
async fn measure(
    counters: &Counters,
    limit: Option<usize>,
    stream: BoxStream<'static, Result<()>>,
) -> Result<Measurement> {
    counters.reset();

    let (stats, entries) = async move {
        let mut stream = match limit {
            Some(limit) => stream.take(limit).boxed(),
            None => stream,
        };
        let mut entries = 0u64;
        while let Some(item) = stream.next().await {
            item?;
            entries += 1;
        }
        anyhow::Ok(entries)
    }
    .timed()
    .await;

    Ok(Measurement {
        entries: entries?,
        gets: counters.gets(),
        bytes: counters.bytes(),
        time: stats.completion_time,
    })
}

/// What the scenario compares, in terms of the fixture's commits.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// A handful of files changed inside the big directory. The case the
    /// sharding-aware fast path was written for.
    ChangedSmall,
    /// Same diff, plus a manifest replacement inside the big directory, which
    /// takes that directory off the fast path.
    ReplacementInBig,
    /// Same diff, but the replacement sits in a medium directory instead --
    /// the big directory keeps the fast path.
    ReplacementInSmall,
    /// The big directory appears wholesale.
    AddedSubtree,
    /// The big directory disappears wholesale.
    RemovedSubtree,
    /// One file changed in each of many medium directories.
    WideTree,
    /// `metadata_diff`-shaped: resolve each changed path independently on both
    /// sides via `find_entry`.
    MetadataLookupPerPath,
    /// The same resolution, batched into a single `find_entries` walk.
    MetadataLookupBatched,
}

impl Kind {
    /// Path-lookup scenarios never reach the diff code, so the fast-path knob
    /// is irrelevant to them and running both settings would just duplicate
    /// rows.
    fn is_diff(&self) -> bool {
        !matches!(
            self,
            Kind::MetadataLookupPerPath | Kind::MetadataLookupBatched
        )
    }
}

struct Scenario {
    name: &'static str,
    kind: Kind,
    ordered: bool,
    limit: Option<usize>,
}

fn scenarios(limit: usize) -> Vec<Scenario> {
    let mut scenarios = Vec::new();
    for (name, kind) in [
        ("changed-small", Kind::ChangedSmall),
        ("replacement-in-big", Kind::ReplacementInBig),
        ("replacement-in-small", Kind::ReplacementInSmall),
        ("added-subtree", Kind::AddedSubtree),
        ("removed-subtree", Kind::RemovedSubtree),
        ("wide-tree", Kind::WideTree),
    ] {
        for ordered in [false, true] {
            scenarios.push(Scenario {
                name,
                kind,
                ordered,
                limit: None,
            });
        }
    }
    // Truncating the consumer does not truncate the enumeration of an added
    // directory, so measure it explicitly.
    for ordered in [false, true] {
        scenarios.push(Scenario {
            name: "added-subtree-limited",
            kind: Kind::AddedSubtree,
            ordered,
            limit: Some(limit),
        });
    }
    scenarios.push(Scenario {
        name: "metadata-lookup-per-path",
        kind: Kind::MetadataLookupPerPath,
        ordered: false,
        limit: None,
    });
    scenarios.push(Scenario {
        name: "metadata-lookup-batched",
        kind: Kind::MetadataLookupBatched,
        ordered: false,
        limit: None,
    });
    scenarios
}

/// Root manifest ids of the fixture's commits, for one manifest backend.
struct Roots<Id> {
    /// No `large_directory`.
    no_big: Id,
    /// `large_directory` present; the base of every "changed" scenario.
    base: Id,
    /// `base` with a few files in `large_directory` modified.
    small_change: Id,
    /// `base` with a few files in `large_directory` modified AND one file in
    /// `wide/dir_0` modified, so that a replacement can be placed in either
    /// directory and the two compared.
    mixed_change: Id,
    /// `base` with one file modified in each `wide/dir_*`.
    wide_change: Id,
}

/// The descendant count a manifest replacement contributes to the ordered
/// traversal's queue weighting -- read from the manifest's rollup rather than
/// counted, exactly as `ChangesetContext::diff` does.
#[async_trait]
trait TreeWeight: Sized {
    async fn tree_weight(&self, ctx: &CoreContext, store: &Store) -> Result<usize>;
}

#[async_trait]
impl TreeWeight for FsnodeId {
    async fn tree_weight(&self, ctx: &CoreContext, store: &Store) -> Result<usize> {
        let fsnode = StoreLoadable::load(self, ctx, store).await?;
        let summary = fsnode.summary();
        Ok((summary.descendant_files_count + summary.child_dirs_count) as usize)
    }
}

#[async_trait]
impl TreeWeight for ContentManifestId {
    async fn tree_weight(&self, ctx: &CoreContext, store: &Store) -> Result<usize> {
        let manifest = StoreLoadable::load(self, ctx, store).await?;
        let counts = manifest.subentries.rollup_data().descendant_counts;
        Ok((counts.files_count + counts.dirs_count) as usize)
    }
}

/// Run one scenario against one manifest backend.
///
/// The fast-path knob is pinned around the *construction* of the diff stream
/// only: both `filtered_diff` and `filtered_diff_ordered` read it eagerly and
/// then return a lazy stream, so this is the whole window in which it matters,
/// and it keeps the thread-local override off every other knob read.
async fn run_scenario<Id>(
    ctx: &CoreContext,
    store: &Store,
    roots: &Roots<Id>,
    lookup_paths: &[MPath],
    scenario: &Scenario,
    fast_path: bool,
    counters: &Counters,
) -> Result<Measurement>
where
    Id: ManifestOps<Store>
        + ManifestOrderedOps<Store>
        + TreeWeight
        + StoreLoadable<Store>
        + Clone
        + Send
        + Sync
        + Eq
        + Unpin
        + 'static,
    <Id as StoreLoadable<Store>>::Value:
        Manifest<Store, TreeId = Id> + OrderedManifest<Store> + Send + Sync,
    <<Id as StoreLoadable<Store>>::Value as Manifest<Store>>::Leaf:
        Clone + Send + Sync + Eq + Unpin + 'static,
    <<Id as StoreLoadable<Store>>::Value as Manifest<Store>>::TrieMapType: TrieMapOps<Store, Entry<Id, <<Id as StoreLoadable<Store>>::Value as Manifest<Store>>::Leaf>>
        + Eq,
    <<Id as StoreLoadable<Store>>::Value as OrderedManifest<Store>>::WeightedTrieMapType:
        TrieMapOps<
                Store,
                Entry<(usize, Id), <<Id as StoreLoadable<Store>>::Value as Manifest<Store>>::Leaf>,
            > + Eq
            + 'static,
{
    // Path-lookup scenarios bypass the diff code entirely.
    match scenario.kind {
        Kind::MetadataLookupPerPath => {
            // `metadata_diff` resolves each path on each side as an independent
            // request, sharing nothing between paths.
            let mut work = Vec::with_capacity(lookup_paths.len() * 2);
            for path in lookup_paths {
                work.push((roots.base.clone(), path.clone()));
                work.push((roots.mixed_change.clone(), path.clone()));
            }
            let stream = {
                let ctx = ctx.clone();
                let store = store.clone();
                stream::iter(work)
                    .map(move |(root, path)| root.find_entry(ctx.clone(), store.clone(), path))
                    .buffered(LOOKUP_CONCURRENCY)
                    .map_ok(|_| ())
                    .boxed()
            };
            return measure(counters, scenario.limit, stream).await;
        }
        Kind::MetadataLookupBatched => {
            let old = roots
                .base
                .find_entries(ctx.clone(), store.clone(), lookup_paths.to_vec())
                .map_ok(|_| ());
            let new = roots
                .mixed_change
                .find_entries(ctx.clone(), store.clone(), lookup_paths.to_vec())
                .map_ok(|_| ());
            let stream = old.chain(new).boxed();
            return measure(counters, scenario.limit, stream).await;
        }
        _ => {}
    }

    // (old side, new side, Option<(replacement site, path to take the payload from)>)
    let (old, new, replacement) = match scenario.kind {
        Kind::ChangedSmall => (&roots.base, &roots.small_change, None),
        Kind::ReplacementInBig => (
            &roots.base,
            &roots.mixed_change,
            Some((MPath::new(BIG_DIR_SUBDIR)?, MPath::new(REPLACEMENT_SOURCE)?)),
        ),
        Kind::ReplacementInSmall => (
            &roots.base,
            &roots.mixed_change,
            Some((
                MPath::new("wide/dir_0/file_0")?,
                MPath::new("wide/dir_1/file_0")?,
            )),
        ),
        Kind::AddedSubtree => (&roots.no_big, &roots.base, None),
        Kind::RemovedSubtree => (&roots.base, &roots.no_big, None),
        Kind::WideTree => (&roots.base, &roots.wide_change, None),
        Kind::MetadataLookupPerPath | Kind::MetadataLookupBatched => unreachable!(),
    };

    // Resolving the replacement payload costs a few gets of its own; do it
    // before `measure` resets the counters, so it isn't charged to the diff.
    let replacement = match replacement {
        None => None,
        Some((at, from)) => {
            let entry = roots
                .base
                .find_entry(ctx.clone(), store.clone(), from.clone())
                .await?
                .ok_or_else(|| anyhow!("replacement source {from} not found"))?;
            Some((at, entry))
        }
    };

    let knobs = JustKnobsInMemory::new(HashMap::from([(
        FAST_PATH_KNOB.to_string(),
        KnobVal::Bool(fast_path),
    )]));

    let stream: BoxStream<'static, Result<()>> = if scenario.ordered {
        let mut replacements = HashMap::new();
        if let Some((at, entry)) = replacement {
            let entry = match entry {
                Entry::Tree(id) => {
                    let weight = id.tree_weight(ctx, store).await?;
                    Entry::Tree((weight, id))
                }
                Entry::Leaf(leaf) => Entry::Leaf(leaf),
            };
            replacements.insert(at, entry);
        }
        with_just_knobs(knobs, || {
            old.filtered_diff_ordered(
                ctx.clone(),
                store.clone(),
                new.clone(),
                store.clone(),
                None,
                |_| Some(()),
                |_| true,
                replacements,
            )
        })
    } else {
        let mut replacements = HashMap::new();
        if let Some((at, entry)) = replacement {
            replacements.insert(at, entry);
        }
        with_just_knobs(knobs, || {
            old.filtered_diff(
                ctx.clone(),
                store.clone(),
                new.clone(),
                store.clone(),
                |_| Some(()),
                |_| true,
                replacements,
            )
        })
    };

    measure(counters, scenario.limit, stream).await
}

fn print_header() {
    println!(
        "\n{:<34} {:<10} {:<5} {:<9} {:<9} {:<12} {:<12} time",
        "scenario", "ordering", "fast", "backend", "entries", "blob_gets", "bytes"
    );
    println!("{}", "-".repeat(110));
}

fn print_pair(
    scenario: &Scenario,
    fast: Option<bool>,
    fsnode: &Measurement,
    content: &Measurement,
) {
    let ordering = if scenario.ordered {
        "ordered"
    } else {
        "unordered"
    };
    let name = match scenario.limit {
        Some(limit) => format!("{} (take {limit})", scenario.name),
        None => scenario.name.to_string(),
    };
    let fast = match fast {
        Some(true) => "on",
        Some(false) => "off",
        None => "n/a",
    };
    println!(
        "{:<34} {:<10} {:<5} {:<9} {:<9} {:<12} {:<12} {:?}",
        name, ordering, fast, "fsnode", fsnode.entries, fsnode.gets, fsnode.bytes, fsnode.time,
    );
    let ratio = if fsnode.gets == 0 {
        String::from("n/a")
    } else {
        format!("{:.1}x", content.gets as f64 / fsnode.gets as f64)
    };
    println!(
        "{:<34} {:<10} {:<5} {:<9} {:<9} {:<12} {:<12} {:?}   ({ratio} gets)",
        "", "", "", "content", content.entries, content.gets, content.bytes, content.time,
    );
}

fn gen_filename(rng: &mut impl Rng, len: usize) -> String {
    std::iter::repeat_with(|| rng.sample(Alphanumeric))
        .take(len)
        .map(char::from)
        .collect()
}

/// The commits every scenario is built from.
struct Fixture {
    no_big: ChangesetId,
    base: ChangesetId,
    small_change: ChangesetId,
    mixed_change: ChangesetId,
    wide_change: ChangesetId,
    /// The paths modified in [`BIG_DIR`], reused as the `metadata_diff` inputs.
    changed_paths: Vec<MPath>,
}

/// Build the fixture:
///
/// * `no_big`: `base_file`, `copy_source/{a,b,c}`, `wide/dir_i/file_j`
/// * `base`: adds `large_directory/<total_files random names>` and
///   `large_directory/subdir/{a,b,c}`
/// * `small_change`, `mixed_change`, `wide_change`: children of `base`
async fn build_fixture(
    ctx: &CoreContext,
    repo: &Repo,
    total_files: usize,
    modify_count: usize,
    wide_dirs: usize,
    wide_files: usize,
) -> Result<Fixture> {
    let mut rng = rand::rng();
    let len_distr = Uniform::new(5, 50).unwrap();
    let mut filenames = std::collections::BTreeSet::new();
    while filenames.len() < total_files {
        let len = rng.sample(len_distr);
        filenames.insert(gen_filename(&mut rng, len));
    }
    let big_paths: Vec<String> = filenames
        .into_iter()
        .map(|name| format!("{BIG_DIR}/{name}"))
        .collect();

    let mut create = CreateCommitContext::new_root(ctx, repo).add_file("base_file", "content");
    for name in ["a", "b", "c"] {
        create = create.add_file(
            format!("{REPLACEMENT_SOURCE}/{name}").as_str(),
            format!("content of {REPLACEMENT_SOURCE}/{name}"),
        );
    }
    for dir in 0..wide_dirs {
        for file in 0..wide_files {
            create = create.add_file(
                format!("wide/dir_{dir}/file_{file}").as_str(),
                format!("content of wide/dir_{dir}/file_{file}"),
            );
        }
    }
    let no_big = create.commit().await?;

    let mut create = CreateCommitContext::new(ctx, repo, vec![no_big]);
    for path in &big_paths {
        create = create.add_file(path.as_str(), format!("content of {path}"));
    }
    for name in ["a", "b", "c"] {
        create = create.add_file(
            format!("{BIG_DIR_SUBDIR}/{name}").as_str(),
            format!("content of {BIG_DIR_SUBDIR}/{name}"),
        );
    }
    let base = create.commit().await?;

    let modify_count = modify_count.min(big_paths.len());
    let modified = &big_paths[..modify_count];

    let mut create = CreateCommitContext::new(ctx, repo, vec![base]);
    for path in modified {
        create = create.add_file(path.as_str(), format!("modified content of {path}"));
    }
    let small_change = create.commit().await?;

    // Same as `small_change` plus one file in a medium directory, so that
    // `replacement-in-big` and `replacement-in-small` diff the same two commits
    // and their get counts are directly comparable.
    let mut create = CreateCommitContext::new(ctx, repo, vec![base]);
    for path in modified {
        create = create.add_file(path.as_str(), format!("modified content of {path}"));
    }
    let mixed_change = create
        .add_file("wide/dir_0/file_0", "modified content of wide/dir_0/file_0")
        .commit()
        .await?;

    let mut create = CreateCommitContext::new(ctx, repo, vec![base]);
    for dir in 0..wide_dirs {
        create = create.add_file(
            format!("wide/dir_{dir}/file_0").as_str(),
            format!("modified content of wide/dir_{dir}/file_0"),
        );
    }
    let wide_change = create.commit().await?;

    let changed_paths = modified
        .iter()
        .map(|path| MPath::new(path.as_str()))
        .collect::<Result<Vec<_>, _>>()?;

    Ok(Fixture {
        no_big,
        base,
        small_change,
        mixed_change,
        wide_change,
        changed_paths,
    })
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

#[fbinit::main]
async fn main(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);

    let mut args = std::env::args().skip(1);
    let mut next = |default: usize| -> usize {
        args.next()
            .and_then(|arg| arg.parse().ok())
            .unwrap_or(default)
    };
    let total_files = next(100_000);
    let modify_count = next(10);
    let wide_dirs = next(8);
    let wide_files = next(2_000);
    let limit = next(100);

    println!(
        "Building repo: {total_files} files in {BIG_DIR}, {wide_dirs} x {wide_files} files under \
         wide/, modifying {modify_count}"
    );
    let repo: Repo = test_repo_factory::build_empty(ctx.fb).await?;
    let fixture = build_fixture(
        &ctx,
        &repo,
        total_files,
        modify_count,
        wide_dirs,
        wide_files,
    )
    .await?;

    let commits = [
        fixture.no_big,
        fixture.base,
        fixture.small_change,
        fixture.mixed_change,
        fixture.wide_change,
    ];
    let (fsnodes, content_manifests) = future::try_join(
        future::try_join_all(commits.iter().map(|csid| derive_fsnode(&ctx, &repo, *csid))),
        future::try_join_all(
            commits
                .iter()
                .map(|csid| derive_content_manifest(&ctx, &repo, *csid)),
        ),
    )
    .await?;

    let [no_big, base, small_change, mixed_change, wide_change]: [FsnodeId; 5] = fsnodes
        .try_into()
        .map_err(|_| anyhow!("expected one fsnode per fixture commit"))?;
    let fsnode_roots = Roots {
        no_big,
        base,
        small_change,
        mixed_change,
        wide_change,
    };
    let [no_big, base, small_change, mixed_change, wide_change]: [ContentManifestId; 5] =
        content_manifests
            .try_into()
            .map_err(|_| anyhow!("expected one content manifest per fixture commit"))?;
    let content_roots = Roots {
        no_big,
        base,
        small_change,
        mixed_change,
        wide_change,
    };

    let gets = Arc::new(AtomicU64::new(0));
    let bytes = Arc::new(AtomicU64::new(0));
    let store: Store = Arc::new(CountingBlobstore {
        inner: repo.repo_blobstore().clone(),
        gets: gets.clone(),
        bytes: bytes.clone(),
    });
    let counters = Counters { gets, bytes };

    print_header();
    for scenario in scenarios(limit) {
        // `fast` is the `enable_sharding_aware_manifest_diff` setting; the
        // path-lookup scenarios don't read it.
        let settings: Vec<Option<bool>> = if scenario.kind.is_diff() {
            vec![Some(true), Some(false)]
        } else {
            vec![None]
        };
        for fast in settings {
            let fast_path = fast.unwrap_or(true);
            let fsnode = run_scenario(
                &ctx,
                &store,
                &fsnode_roots,
                &fixture.changed_paths,
                &scenario,
                fast_path,
                &counters,
            )
            .await?;
            let content = run_scenario(
                &ctx,
                &store,
                &content_roots,
                &fixture.changed_paths,
                &scenario,
                fast_path,
                &counters,
            )
            .await?;
            print_pair(&scenario, fast, &fsnode, &content);
        }
    }

    Ok(())
}
