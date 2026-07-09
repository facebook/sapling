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
use manifest::FileType;
use manifest::FsNodeMetadata;
use manifest_tree::TreeManifest;
use pathmatcher::DynMatcher;
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

const FETCH_BATCH_SIZE: usize = 1000;
const CONCURRENT_FETCHES: usize = 10;
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
            fetch_files(batch?, scope, &file_store, FetchContext::sapling_default())
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
    }
}
fn fetch_files(
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

        let batch = batch?;

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
}
