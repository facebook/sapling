/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::VecDeque;
use std::mem;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::time::Instant;

use manifest::Manifest;
use pathmatcher::DynMatcher;
use storemodel::FileStore;
use types::FetchContext;
use types::Key;
use types::RepoPathBuf;
use types::fetch_mode::FetchMode;

/// Start an async prefetch of file content by walking `manifest` using `matcher`,
/// fetching file content of resultant files via `file_store`. Returns a handle
/// that will cancel the prefetch on drop.
pub(crate) fn prefetch(
    manifest: impl Manifest + Send + Sync + 'static,
    file_store: Arc<dyn FileStore>,
    matcher: DynMatcher,
) -> PrefetchHandle {
    // The cancelation works by making our thread below return early when the handle has been
    // dropped. When it returns, it drops its manifest/file iterators, which will cause those
    // operations to cancel themselves the next time they fail sending on their result channels.
    let handle = PrefetchHandle::default();

    let my_handle = handle.clone();
    // Sapling APIs are not async, so to achieve asynchronicity we create a new thread.
    std::thread::spawn(move || {
        let mut file_count = 0;
        let start_time = Instant::now();

        // This iterator is populated async by the manifest iterator.
        let files = manifest.files(matcher);

        const BATCH_SIZE: usize = 10_000;
        let mut batch = Vec::new();

        // Allow multiple concurrent file fetches to stack up.
        const MAX_CONCURRENT_FILE_FETCHES: usize = 5;
        let mut file_fetches: VecDeque<Box<dyn Iterator<Item = _>>> = VecDeque::new();

        let mut fetch_batch = |batch: &mut Vec<Key>| {
            if batch.is_empty() {
                return;
            }

            file_count += batch.len();

            // If file fetches are full, wait for first one to finish.
            if file_fetches.len() >= MAX_CONCURRENT_FILE_FETCHES {
                file_fetches.pop_front().unwrap().for_each(drop);
            }

            // An important implementation detail for us: the scmstore FileStore spawns a thread
            // when you fetch more than 1_000 keys (i.e. this method will operate asynchronously if
            // we fetch more than 1k files). If that assumption changes, we will need to change what
            // we do here.
            let fetch_res = file_store.get_content_iter(
                // Use IGNORE_RESULT optimization since we don't care about the data.
                FetchContext::new(FetchMode::AllowRemote | FetchMode::IGNORE_RESULT),
                mem::take(batch),
            );

            match fetch_res {
                Ok(iter) => {
                    file_fetches.push_back(iter);
                }
                Err(err) => {
                    tracing::error!(?err, "error prefetching file content");
                }
            }
        };

        for file in files {
            if my_handle.is_canceled() {
                tracing::info!(elapsed=?start_time.elapsed(), file_count, "prefetch canceled");
                return;
            }

            match file {
                Ok(file) => {
                    // Don't propagate path to save memory.
                    batch.push(Key::new(RepoPathBuf::new(), file.meta.hgid));

                    if batch.len() >= BATCH_SIZE {
                        fetch_batch(&mut batch);
                    }
                }
                Err(err) => {
                    tracing::error!(?err, "error fetching files from manifest");
                }
            }
        }

        fetch_batch(&mut batch);

        // Wait for any remaining file fetches to finish.
        for fetch in file_fetches {
            if my_handle.is_canceled() {
                tracing::info!(elapsed=?start_time.elapsed(), file_count, "prefetch canceled");
                return;
            }

            fetch.for_each(drop);
        }

        // Make sure this doesn't get dropped early.
        drop(my_handle);

        tracing::info!(elapsed=?start_time.elapsed(), file_count, "prefetch complete");
    });

    handle
}

#[derive(Clone, Default)]
pub(crate) struct PrefetchHandle {
    canceled: Arc<AtomicBool>,
}

impl PrefetchHandle {
    pub(crate) fn is_canceled(&self) -> bool {
        self.canceled.load(Ordering::Relaxed)
    }
}

impl Drop for PrefetchHandle {
    fn drop(&mut self) {
        self.canceled.store(true, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod test {
    use std::time::Duration;

    use manifest::FileMetadata;
    use manifest_tree::TreeManifest;
    use manifest_tree::testutil::TestStore;
    use pathmatcher::ExactMatcher;

    use super::*;

    #[test]
    fn test_prefetch_files() -> anyhow::Result<()> {
        let store = Arc::new(TestStore::new());

        let file_store = store.clone() as Arc<dyn FileStore>;

        let foo_path: RepoPathBuf = "foo".to_string().try_into()?;
        let bar_path: RepoPathBuf = "bar".to_string().try_into()?;

        // Insert a couple files into the file store.
        let foo_hgid = file_store.insert_data(Default::default(), &foo_path, b"foo content")?;
        let bar_hgid = file_store.insert_data(Default::default(), &bar_path, b"bar content")?;

        let mut mf = TreeManifest::ephemeral(store.clone());

        // Create corresponding manifest entries.
        mf.insert(
            foo_path.clone(),
            FileMetadata::new(foo_hgid, types::FileType::Regular),
        )?;

        mf.insert(
            bar_path.clone(),
            FileMetadata::new(bar_hgid, types::FileType::Regular),
        )?;

        // Only match foo_path.
        let matcher = ExactMatcher::new(std::iter::once(&foo_path), true);

        let handle = prefetch(mf, file_store, Arc::new(matcher));

        // Wait for prefetch to complete.
        while !handle.is_canceled() {
            std::thread::sleep(Duration::from_millis(1));
        }

        // We only fetched "foo" (due to the matcher).
        assert_eq!(
            store.fetches(),
            vec![Key::new(RepoPathBuf::new(), foo_hgid)]
        );

        Ok(())
    }

    #[test]
    fn test_prefetch_handle() {
        let handle = PrefetchHandle::default();
        assert!(!handle.is_canceled());

        drop(handle.clone());
        assert!(handle.is_canceled());
    }
}
