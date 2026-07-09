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
use manifest::FileType;
use manifest::FsNodeMetadata;
use manifest_tree::TreeManifest;
use pathmatcher::Matcher;
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
const RESULT_QUEUE_SIZE: usize = CONCURRENT_FETCHES * 8;

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
pub fn walk_and_fetch<M: 'static + Matcher + Sync + Send>(
    manifest: TreeManifest,
    matcher: M,
    file_store: &Arc<dyn FileStore>,
) -> FileItems {
    let file_store = file_store.clone();
    Work::run(
        WorkOptions::new()
            .max_workers(CONCURRENT_FETCHES)
            .inline_items(FETCH_BATCH_SIZE)
            .result_queue_size(RESULT_QUEUE_SIZE),
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
