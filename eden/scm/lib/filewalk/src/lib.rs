/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Crate for walking a manifest and fetching file contents in parallel.
//!
//! The main entry point is [`walk_and_fetch`], which returns a channel of file
//! results and a [`FirstError`] handle that can be used to check for errors or
//! cancel the operation.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::thread;

use blob::Blob;
use manifest::FileMetadata;
use manifest::FileType;
use manifest::FsNodeMetadata;
use manifest_tree::TreeManifest;
use pathmatcher::Matcher;
use storemodel::FileStore;
use types::FetchContext;
use types::HgId;
use types::Key;
use types::RepoPathBuf;

const FETCH_BATCH_SIZE: usize = 1000;
const CONCURRENT_FETCHES: usize = 10;
const RESULT_BATCH_SIZE: usize = 128;
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

/// FirstError helps propagate the first error seen in parallel operations.
///
/// It provides a "has_error" method to aid in cancellation. The caller can
/// also use `send_error` to cancel the operation by inserting its own error.
pub struct FirstError {
    tx: flume::Sender<anyhow::Error>,
    rx: flume::Receiver<anyhow::Error>,
    has_error: Arc<AtomicBool>,
}

impl Clone for FirstError {
    fn clone(&self) -> Self {
        FirstError {
            tx: self.tx.clone(),
            rx: self.rx.clone(),
            has_error: self.has_error.clone(),
        }
    }
}

impl FirstError {
    /// Create a new FirstError instance.
    pub fn new() -> Self {
        let (tx, rx) = flume::bounded(1);
        FirstError {
            tx,
            rx,
            has_error: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Store error (if first). This also signals cancellation.
    pub fn send_error(&self, err: anyhow::Error) {
        self.has_error.store(true, Ordering::Relaxed);
        let _ = self.tx.try_send(err);
    }

    /// Return whether an error has been stored. Useful for cancellation.
    pub fn has_error(&self) -> bool {
        self.has_error.load(Ordering::Relaxed)
    }

    /// Wait for all copies this FirstError to be dropped, and then yield the first error, if any.
    pub fn wait(self) -> anyhow::Result<()> {
        drop(self.tx);
        match self.rx.try_recv() {
            Ok(err) => Err(err),
            Err(_) => Ok(()),
        }
    }
}

impl Default for FirstError {
    fn default() -> Self {
        Self::new()
    }
}

/// Walk the manifest matching the given matcher and fetch file contents in parallel.
///
/// Returns a channel that receives batches of `FileResult` instances
/// (not wrapped in Result/Error)
/// and a `FirstError` handle that can be used to:
/// - Check for errors with `has_error()`
/// - Cancel the operation by calling `send_error()`
/// - Wait for completion and get the first error (if any) with `wait()`
///
/// The walk stops early if an error occurs (either from the manifest iteration or file fetching).
pub fn walk_and_fetch<M: 'static + Matcher + Sync + Send>(
    manifest: TreeManifest,
    matcher: M,
    file_store: &Arc<dyn FileStore>,
) -> (flume::Receiver<Vec<FileResult>>, FirstError) {
    let first_error = FirstError::new();
    let (result_tx, result_rx) = flume::bounded(RESULT_QUEUE_SIZE);

    let (fetch_content_tx, fetch_content_rx) =
        flume::bounded::<Vec<(RepoPathBuf, FileMetadata)>>(CONCURRENT_FETCHES);

    let mut handles: Vec<thread::JoinHandle<()>> = Vec::new();

    // Spawn fetch threads
    for _ in 0..CONCURRENT_FETCHES {
        let fetch_content_rx = fetch_content_rx.clone();
        let file_store = file_store.clone();
        let first_error = first_error.clone();
        let result_tx = result_tx.clone();

        handles.push(thread::spawn(move || {
            let run = || -> anyhow::Result<()> {
                while let Ok(batch) = fetch_content_rx.recv() {
                    if first_error.has_error() {
                        return Ok(());
                    }

                    let keys: Vec<Key> = batch
                        .iter()
                        .map(|(path, meta)| Key::new(path.clone(), meta.hgid))
                        .collect();

                    let iter =
                        file_store.get_content_iter(FetchContext::sapling_default(), keys)?;

                    let mut file_info = HashMap::with_capacity(batch.len());
                    for (path, meta) in batch {
                        file_info.insert(Key::new(path, meta.hgid), meta.file_type);
                    }

                    let mut result_batch = Vec::with_capacity(RESULT_BATCH_SIZE);

                    for result in iter {
                        if first_error.has_error() {
                            return Ok(());
                        }

                        let (key, data) = result?;

                        let file_type = file_info
                            .get(&key)
                            .ok_or_else(|| anyhow::anyhow!("missing file info for {}", key.hgid))?;

                        result_batch.push(FileResult {
                            path: key.path,
                            hgid: key.hgid,
                            data,
                            file_type: *file_type,
                        });

                        if result_batch.len() >= RESULT_BATCH_SIZE {
                            if result_tx.send(std::mem::take(&mut result_batch)).is_err() {
                                // Receiver dropped, stop processing
                                return Ok(());
                            }
                        }
                    }

                    if !result_batch.is_empty() && result_tx.send(result_batch).is_err() {
                        // Receiver dropped, stop processing
                        return Ok(());
                    }
                }
                Ok(())
            };

            if let Err(e) = run() {
                first_error.send_error(e);
            }
        }));
    }

    drop(fetch_content_rx);
    drop(result_tx);

    // Spawn manifest iteration thread
    let manifest_first_error = first_error.clone();
    handles.push(thread::spawn(move || {
        let run = || -> anyhow::Result<()> {
            let mut current_batch = Vec::new();

            for result in manifest.iter(matcher) {
                if manifest_first_error.has_error() {
                    break;
                }

                let (path, metadata) = result?;
                if let FsNodeMetadata::File(file_meta) = metadata {
                    current_batch.push((path, file_meta));

                    if current_batch.len() >= FETCH_BATCH_SIZE {
                        if fetch_content_tx
                            .send(std::mem::take(&mut current_batch))
                            .is_err()
                        {
                            return Ok(());
                        }
                    }
                }
            }

            if !current_batch.is_empty() {
                let _ = fetch_content_tx.send(current_batch);
            }

            Ok(())
        };

        if let Err(e) = run() {
            manifest_first_error.send_error(e);
        }
    }));

    // Spawn a thread to join all worker threads
    // This ensures the FirstError is kept alive until all work is done
    let join_first_error = first_error.clone();
    thread::spawn(move || {
        for handle in handles {
            if let Err(e) = handle.join() {
                std::panic::resume_unwind(e);
            }
        }
        // Keep first_error alive until all threads have finished
        drop(join_first_error);
    });

    (result_rx, first_error)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_first_error_no_error() {
        let first_error = FirstError::new();
        assert!(!first_error.has_error());
        first_error.wait().unwrap();
    }

    #[test]
    fn test_first_error_with_error() {
        let first_error = FirstError::new();
        let cloned = first_error.clone();

        cloned.send_error(anyhow::anyhow!("test error"));
        assert!(first_error.has_error());

        drop(cloned);
        let err = first_error.wait().unwrap_err();
        assert_eq!(err.to_string(), "test error");
    }

    #[test]
    fn test_first_error_only_first() {
        let first_error = FirstError::new();
        first_error.send_error(anyhow::anyhow!("first error"));
        first_error.send_error(anyhow::anyhow!("second error"));
        assert!(first_error.has_error());

        let err = first_error.wait().unwrap_err();
        assert_eq!(err.to_string(), "first error");
    }
}
