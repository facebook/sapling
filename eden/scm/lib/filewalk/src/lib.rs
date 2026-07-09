/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Crate for walking a manifest and fetching file contents in parallel.
//!
//! The main entry point is [`walk_and_fetch`], which returns batches of file results.

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

/// Walk the manifest matching the given matcher and fetch file contents in parallel.
///
/// The returned iterator owns the pipeline. Dropping it cancels pending work.
pub fn walk_and_fetch(
    manifest: TreeManifest,
    matcher: DynMatcher,
    file_store: &Arc<dyn FileStore>,
) -> FileItems {
    walk_and_fetch_with_options(manifest, matcher, file_store, WalkOptions::default())
}

/// Walk the manifest and fetch file contents with caller-provided tuning options.
///
/// `options` controls the fetch batch size and maximum fetch worker count.
/// The returned iterator owns the pipeline. Dropping it cancels pending work.
pub fn walk_and_fetch_with_options(
    manifest: TreeManifest,
    matcher: DynMatcher,
    file_store: &Arc<dyn FileStore>,
    options: WalkOptions,
) -> FileItems {
    let options = options.normalized();
    let file_store = file_store.clone();
    Work::run(
        WorkOptions::new()
            .max_workers(options.concurrent_fetches)
            .inline_items(options.batch_size)
            .result_queue_size(options.concurrent_fetches * RESULT_QUEUE_SIZE_PER_WORKER),
        manifest.iter(matcher),
        WorkShape::batch(move |batch, scope| fetch_files(batch?, scope, &file_store)),
    )
}

fn fetch_files(
    work: Vec<FetchWork>,
    scope: &mut WorkScope<'_, FetchWork, FileResult, anyhow::Error>,
    file_store: &Arc<dyn FileStore>,
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

    let content_items = file_store.get_content_iter(FetchContext::sapling_default(), keys)?;

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
}
