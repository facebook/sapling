/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Crate for walking manifests and fetching file contents in parallel.
//!
//! [`walk_and_fetch`] streams fetched file contents. [`prefetch`] walks the same
//! input but only populates caches and returns [`FileStats`].

use std::collections::HashMap;
use std::sync::Arc;

use blob::Blob;
use configmodel::Config;
use configmodel::ConfigExt;
use manifest::FileMetadata;
use manifest::FileType;
use manifest::FsNodeMetadata;
use manifest::Manifest;
use manifest_tree::TreeManifest;
use pathmatcher::DynMatcher;
use pathmatcher::IntersectMatcher;
use slex::Items;
use slex::Work;
use slex::WorkOptions;
use slex::WorkScope;
use slex::WorkShape;
use storemodel::FileStore;
use types::FetchContext;
use types::HgId;
use types::Key;
use types::RepoPathBuf;

const FETCH_BATCH_SIZE: usize = 4000;
const CONCURRENT_FETCHES: usize = 32;
const MAX_CONCURRENT_FETCHES: usize = 128;
const RESULT_QUEUE_SIZE_PER_WORKER: usize = 8;

/// Tuning options for manifest walking and file fetching.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WalkOptions {
    /// Number of manifest file entries to group into one fetch work item.
    pub batch_size: usize,
    /// Maximum number of concurrent file fetch workers.
    pub concurrent_fetches: usize,
}

impl WalkOptions {
    /// Load generic filewalk tuning from config.
    ///
    /// Commands can set `filewalk.batch-size` and `filewalk.concurrent-fetches`
    /// without needing a command-specific bridge.
    pub fn from_config(config: &dyn Config) -> anyhow::Result<Self> {
        let mut options = Self::default();
        if let Some(batch_size) = config.get_opt::<usize>("filewalk", "batch-size")? {
            options = options.with_batch_size(batch_size);
        }
        if let Some(concurrent_fetches) =
            config.get_opt::<usize>("filewalk", "concurrent-fetches")?
        {
            options = options.with_concurrent_fetches(concurrent_fetches);
        }
        Ok(options)
    }

    /// Set the number of manifest file entries per fetch work item.
    pub fn with_batch_size(mut self, batch_size: usize) -> Self {
        self.batch_size = batch_size;
        self
    }

    /// Set the maximum number of concurrent file fetch workers.
    pub fn with_concurrent_fetches(mut self, concurrent_fetches: usize) -> Self {
        self.concurrent_fetches = concurrent_fetches;
        self
    }

    fn normalized(self) -> Self {
        Self {
            batch_size: self.batch_size.max(1),
            concurrent_fetches: self.concurrent_fetches.clamp(1, MAX_CONCURRENT_FETCHES),
        }
    }
}

impl Default for WalkOptions {
    fn default() -> Self {
        Self {
            batch_size: FETCH_BATCH_SIZE,
            concurrent_fetches: CONCURRENT_FETCHES,
        }
    }
}

/// A file result containing the path, content blob, and file type.
pub struct FileResult {
    pub path: RepoPathBuf,
    pub hgid: HgId,
    pub data: Blob,
    pub file_type: FileType,
}

impl FileResult {
    pub fn is_symlink(&self) -> bool {
        matches!(self.file_type, FileType::Symlink)
    }
}

type FetchWork = (RepoPathBuf, FsNodeMetadata);
pub type FileItems = Items<FileResult, anyhow::Error>;

/// Starting point for a file walk.
pub enum WalkInput {
    /// Walk files reachable from a single manifest.
    Manifest(TreeManifest),
    /// Walk files changed between two manifests.
    ///
    /// `manifest` is the target side and `base_manifest` is the base side. Only file nodes from
    /// `manifest` are yielded to downstream fetchers.
    Diff {
        manifest: TreeManifest,
        base_manifest: TreeManifest,
    },
    /// Walk files modified across several manifest pairs in one traversal.
    ///
    /// Additions and removals are skipped. Both file nodes are yielded for each modification.
    ModifiedDiffPairs(Vec<ModifiedDiffPair>),
}

/// One manifest pair walked by [`WalkInput::ModifiedDiffPairs`].
pub struct ModifiedDiffPair {
    /// Target side of the diff.
    pub manifest: TreeManifest,
    /// Base side of the diff.
    pub base_manifest: TreeManifest,
    /// Only paths accepted by both this matcher and the walk matcher are visited.
    pub matcher: DynMatcher,
}

/// Counts of file content fetches performed by a prefetch walk.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FileStats {
    /// File contents satisfied locally according to the drained `FetchContext`.
    pub local_files: u64,
    /// File contents fetched remotely according to the drained `FetchContext`.
    pub remote_files: u64,
}

/// Walk the manifest matching the given matcher and fetch file contents in parallel.
///
/// The returned iterator owns the pipeline. Dropping it cancels pending work.
pub fn walk_and_fetch(
    input: WalkInput,
    matcher: DynMatcher,
    file_store: &Arc<dyn FileStore>,
    options: WalkOptions,
) -> FileItems {
    let options = options.normalized();
    let file_store = file_store.clone();
    let file_nodes = manifest_items(input, matcher);
    Work::run(
        work_options(options),
        file_nodes,
        WorkShape::batch(move |batch, scope| {
            fetch_batch(batch?, scope, &file_store, FetchContext::sapling_default())
        }),
    )
}

/// Fetch an explicit stream of file nodes in parallel.
///
/// This is useful when callers already know the file nodes and need one fetch pipeline spanning
/// files from multiple manifests.
pub fn fetch_file_nodes(
    file_nodes: Items<(RepoPathBuf, FileMetadata), anyhow::Error>,
    file_store: &Arc<dyn FileStore>,
    fetch_context: FetchContext,
    options: WalkOptions,
) -> FileItems {
    let options = options.normalized();
    let file_store = file_store.clone();
    let file_nodes = file_nodes.map_batch(|batch| {
        Ok(batch?
            .into_iter()
            .map(|(path, metadata)| (path, FsNodeMetadata::File(metadata)))
            .collect::<Vec<_>>())
    });
    Work::run(
        work_options(options),
        file_nodes,
        WorkShape::batch(move |batch, scope| {
            fetch_batch(batch?, scope, &file_store, fetch_context.clone())
        }),
    )
}

/// Walk matching files and prefetch their contents into the backing cache.
///
/// Unlike [`walk_and_fetch`], this is cache-only: it uses `FetchContext::sapling_prefetch()`,
/// discards file contents, and waits for the pipeline to drain before returning [`FileStats`].
/// The returned counters are read from that fetch context after draining, so they describe the
/// number of file contents satisfied locally vs fetched remotely by this prefetch operation.
pub fn prefetch(
    input: WalkInput,
    matcher: DynMatcher,
    file_store: &Arc<dyn FileStore>,
    options: WalkOptions,
) -> anyhow::Result<FileStats> {
    let fctx = FetchContext::sapling_prefetch();
    let stats_fctx = fctx.clone();
    let options = options.normalized();
    let file_store = file_store.clone();
    let file_nodes = manifest_items(input, matcher);
    Work::run(
        work_options(options),
        file_nodes,
        WorkShape::batch(move |batch, scope| cache_files(batch?, scope, &file_store, fctx.clone())),
    )
    .drain()?;
    Ok(FileStats::from_fetch_context(&stats_fctx))
}

fn work_options(options: WalkOptions) -> WorkOptions {
    WorkOptions::new()
        .max_workers(options.concurrent_fetches)
        .inline_items(options.batch_size)
        .result_queue_size(options.concurrent_fetches * RESULT_QUEUE_SIZE_PER_WORKER)
}

fn manifest_items(input: WalkInput, matcher: DynMatcher) -> Items<FetchWork, anyhow::Error> {
    match input {
        WalkInput::Manifest(manifest) => manifest.iter(matcher),
        WalkInput::Diff {
            manifest,
            base_manifest,
        } => diff_manifest_items(manifest, base_manifest, matcher),
        WalkInput::ModifiedDiffPairs(manifest_pairs) => {
            modified_diff_manifest_pairs_items(manifest_pairs, matcher)
        }
    }
}

fn modified_diff_manifest_pairs_items(
    manifest_pairs: Vec<ModifiedDiffPair>,
    matcher: DynMatcher,
) -> Items<FetchWork, anyhow::Error> {
    let pairs = manifest_pairs
        .iter()
        .map(|pair| {
            let pair_matcher: DynMatcher = Arc::new(IntersectMatcher::new(vec![
                matcher.clone(),
                pair.matcher.clone(),
            ]));
            (&pair.manifest, &pair.base_manifest, pair_matcher)
        })
        .collect();

    manifest_tree::diff_manifests(pairs).map_batch(|batch| {
        Ok(batch?
            .into_iter()
            .flat_map(|(_pair, diff_entry)| {
                let path = diff_entry.path;
                match (diff_entry.diff_type.left(), diff_entry.diff_type.right()) {
                    (Some(left), Some(right)) => [Some((path.clone(), left)), Some((path, right))],
                    _ => [None, None],
                }
                .into_iter()
                .flatten()
            })
            .map(|(path, metadata)| (path, FsNodeMetadata::File(metadata)))
            .collect::<Vec<_>>())
    })
}

fn diff_manifest_items(
    manifest: TreeManifest,
    base_manifest: TreeManifest,
    matcher: DynMatcher,
) -> Items<FetchWork, anyhow::Error> {
    match manifest.diff(&base_manifest, matcher) {
        Ok(items) => items.map_batch(|batch| {
            Ok(batch?
                .into_iter()
                .filter_map(|diff_entry| {
                    diff_entry
                        .diff_type
                        .left()
                        .map(|file_meta| (diff_entry.path, FsNodeMetadata::File(file_meta)))
                })
                .collect::<Vec<_>>())
        }),
        Err(err) => Items::error(err),
    }
}

fn fetch_batch(
    work: Vec<FetchWork>,
    scope: &mut WorkScope<'_, FetchWork, FileResult, anyhow::Error>,
    file_store: &Arc<dyn FileStore>,
    fctx: FetchContext,
) -> anyhow::Result<()> {
    let mut file_info = HashMap::with_capacity(work.len());
    let mut keys = Vec::new();
    for (path, metadata) in work {
        if let FsNodeMetadata::File(meta) = metadata {
            let key = Key::new(path, meta.hgid);
            file_info.insert(key.clone(), meta.file_type);
            keys.push(key);
        }
    }

    if keys.is_empty() {
        return Ok(());
    }

    let content_items = file_store.get_content_iter(fctx.clone(), keys)?;

    for batch in content_items.into_batches() {
        if scope.is_canceled() {
            return Ok(());
        }

        let batch = match batch {
            Ok(batch) => batch,
            Err(err) => {
                if !scope.send_error(err) {
                    return Ok(());
                }
                continue;
            }
        };

        for (key, data) in batch {
            let file_type = file_info
                .get(&key)
                .copied()
                .ok_or_else(|| anyhow::anyhow!("missing file info for {}", key.hgid))?;

            if !scope.send_result([FileResult {
                path: key.path,
                hgid: key.hgid,
                data,
                file_type,
            }]) {
                return Ok(());
            }
        }
    }

    Ok(())
}

fn cache_files(
    work: Vec<FetchWork>,
    scope: &mut WorkScope<'_, FetchWork, (), anyhow::Error>,
    file_store: &Arc<dyn FileStore>,
    fctx: FetchContext,
) -> anyhow::Result<()> {
    let keys = work
        .into_iter()
        .filter_map(|(path, metadata)| match metadata {
            FsNodeMetadata::File(meta) => Some(Key::new(path, meta.hgid)),
            _ => None,
        })
        .collect::<Vec<_>>();

    if keys.is_empty() || scope.is_canceled() {
        return Ok(());
    }

    let content_items = match file_store.get_content_iter(fctx, keys) {
        Ok(items) => items,
        Err(err) => {
            scope.send_error(err);
            return Ok(());
        }
    };

    for batch in content_items.into_batches() {
        if scope.is_canceled() {
            return Ok(());
        }
        if let Err(err) = batch {
            if !scope.send_error(err) {
                return Ok(());
            }
        }
    }

    Ok(())
}

impl FileStats {
    fn from_fetch_context(fctx: &FetchContext) -> Self {
        Self {
            local_files: fctx.local_fetch_count(),
            remote_files: fctx.remote_fetch_count(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use manifest_tree::testutil::TestStore;
    use manifest_tree::testutil::make_tree_manifest;
    use pathmatcher::AlwaysMatcher;
    use pathmatcher::ExactMatcher;
    use storemodel::InsertOpts;
    use storemodel::KeyStore;
    use types::testutil::hgid;
    use types::testutil::repo_path;
    use types::testutil::repo_path_buf;

    use super::*;

    #[test]
    fn test_walk_options_normalized() {
        let options = WalkOptions::default()
            .with_batch_size(0)
            .with_concurrent_fetches(0)
            .normalized();
        assert_eq!(options.batch_size, 1);
        assert_eq!(options.concurrent_fetches, 1);

        let options = WalkOptions::default()
            .with_concurrent_fetches(usize::MAX)
            .normalized();
        assert_eq!(options.concurrent_fetches, MAX_CONCURRENT_FETCHES);
    }

    #[test]
    fn test_walk_options_from_config_defaults() -> anyhow::Result<()> {
        let config = BTreeMap::<&str, &str>::new();
        assert_eq!(WalkOptions::from_config(&config)?, WalkOptions::default());
        Ok(())
    }

    #[test]
    fn test_walk_options_from_config_overrides() -> anyhow::Result<()> {
        let mut config = BTreeMap::<&str, &str>::new();
        config.insert("filewalk.batch-size", "123");
        config.insert("filewalk.concurrent-fetches", "456");

        assert_eq!(
            WalkOptions::from_config(&config)?,
            WalkOptions {
                batch_size: 123,
                concurrent_fetches: 456
            }
        );
        assert_eq!(
            WalkOptions::from_config(&config)?
                .normalized()
                .concurrent_fetches,
            MAX_CONCURRENT_FETCHES
        );
        Ok(())
    }

    #[test]
    fn test_file_stats_from_fetch_context() {
        let fctx = FetchContext::sapling_prefetch();
        fctx.inc_local(3);
        fctx.inc_remote(5);

        assert_eq!(
            FileStats::from_fetch_context(&fctx),
            FileStats {
                local_files: 3,
                remote_files: 5,
            }
        );
    }

    /// Two manifest pairs: `first` and `second` are modified in their pairs, `added` and
    /// `removed` are one-sided, and the second pair repeats the same `first` modification.
    fn modified_diff_pairs_store() -> anyhow::Result<(
        Arc<TestStore>,
        Vec<(TreeManifest, TreeManifest)>,
        BTreeMap<&'static str, Vec<Key>>,
    )> {
        let store = Arc::new(TestStore::new());
        let first_id = hgid("11");
        let first_base_id = hgid("22");
        let second_id = hgid("33");
        let second_base_id = hgid("44");
        let added_id = hgid("55");
        let removed_id = hgid("66");
        for (path, id, data) in [
            (repo_path("first"), first_id, b"first\n".as_slice()),
            (
                repo_path("first"),
                first_base_id,
                b"first base\n".as_slice(),
            ),
            (repo_path("second"), second_id, b"second\n".as_slice()),
            (
                repo_path("second"),
                second_base_id,
                b"second base\n".as_slice(),
            ),
            (repo_path("added"), added_id, b"added\n".as_slice()),
            (repo_path("removed"), removed_id, b"removed\n".as_slice()),
        ] {
            store.insert_data(
                InsertOpts {
                    forced_id: Some(Box::new(id)),
                    ..Default::default()
                },
                path,
                Blob::from(data.to_vec()),
            )?;
        }

        let first = make_tree_manifest(store.clone(), &[("first", "11"), ("added", "55")]);
        let first_base = make_tree_manifest(store.clone(), &[("first", "22"), ("removed", "66")]);
        let second = make_tree_manifest(store.clone(), &[("first", "11"), ("second", "33")]);
        let second_base = make_tree_manifest(store.clone(), &[("first", "22"), ("second", "44")]);
        let keys = BTreeMap::from([
            (
                "first",
                vec![
                    Key::new(repo_path_buf("first"), first_id),
                    Key::new(repo_path_buf("first"), first_base_id),
                ],
            ),
            (
                "second",
                vec![
                    Key::new(repo_path_buf("second"), second_id),
                    Key::new(repo_path_buf("second"), second_base_id),
                ],
            ),
        ]);
        Ok((
            store,
            vec![(first, first_base), (second, second_base)],
            keys,
        ))
    }

    /// Distinct keys handed to the store. The walk does not deduplicate keys repeated
    /// across pairs; the store does that within a request.
    fn distinct_fetches(store: &TestStore) -> Vec<Key> {
        let mut fetched = store.fetches().into_iter().flatten().collect::<Vec<_>>();
        fetched.sort();
        fetched.dedup();
        fetched
    }

    #[test]
    fn test_prefetch_modified_diff_pairs() -> anyhow::Result<()> {
        let (store, pairs, keys) = modified_diff_pairs_store()?;
        let always: DynMatcher = Arc::new(AlwaysMatcher::new());
        let pairs = pairs
            .into_iter()
            .map(|(manifest, base_manifest)| ModifiedDiffPair {
                manifest,
                base_manifest,
                matcher: always.clone(),
            })
            .collect();

        prefetch(
            WalkInput::ModifiedDiffPairs(pairs),
            always,
            &(store.clone() as Arc<dyn FileStore>),
            WalkOptions::default(),
        )?;
        let mut expected = keys.into_values().flatten().collect::<Vec<_>>();
        expected.sort();
        assert_eq!(distinct_fetches(&store), expected);
        Ok(())
    }

    #[test]
    fn test_prefetch_modified_diff_pairs_per_pair_matcher() -> anyhow::Result<()> {
        let (store, mut pairs, keys) = modified_diff_pairs_store()?;
        let (second, second_base) = pairs.pop().unwrap();
        let (first, first_base) = pairs.pop().unwrap();
        let second_only: DynMatcher =
            Arc::new(ExactMatcher::new([repo_path("second")].iter(), true));
        let nothing: DynMatcher = Arc::new(ExactMatcher::new(
            std::iter::empty::<&types::RepoPath>(),
            true,
        ));

        prefetch(
            WalkInput::ModifiedDiffPairs(vec![
                ModifiedDiffPair {
                    manifest: first,
                    base_manifest: first_base,
                    matcher: nothing,
                },
                ModifiedDiffPair {
                    manifest: second,
                    base_manifest: second_base,
                    matcher: second_only,
                },
            ]),
            Arc::new(AlwaysMatcher::new()),
            &(store.clone() as Arc<dyn FileStore>),
            WalkOptions::default(),
        )?;
        let mut expected = keys["second"].clone();
        expected.sort();
        assert_eq!(distinct_fetches(&store), expected);
        Ok(())
    }

    #[test]
    fn test_fetch_file_nodes() -> anyhow::Result<()> {
        let store = Arc::new(TestStore::new());
        let id = hgid("11");
        store.insert_data(
            InsertOpts {
                forced_id: Some(Box::new(id)),
                ..Default::default()
            },
            repo_path("a.txt"),
            Blob::from(b"a\n".to_vec()),
        )?;
        let files = Items::ready(vec![(
            repo_path_buf("a.txt"),
            FileMetadata {
                hgid: id,
                file_type: FileType::Regular,
                ignore_unless_conflict: false,
            },
        )]);

        let results = fetch_file_nodes(
            files,
            &(store.clone() as Arc<dyn FileStore>),
            FetchContext::sapling_default(),
            WalkOptions::default(),
        )
        .into_iter()
        .collect::<anyhow::Result<Vec<_>>>()?;

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].path, repo_path_buf("a.txt"));
        assert_eq!(results[0].data.to_bytes().as_ref(), b"a\n");
        assert_eq!(store.key_fetch_count(), 1);
        Ok(())
    }
}
