/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::HashMap;
use std::collections::HashSet;
use std::collections::VecDeque;
use std::mem;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::time::Duration;
use std::time::Instant;

use manifest::Manifest;
use parking_lot::RwLock;
use pathmatcher::DepthMatcher;
use pathmatcher::IntersectMatcher;
use pathmatcher::TreeMatcher;
use pathmatcher::make_glob_recursive;
use pathmatcher::plain_to_glob;
use repo::ReadTreeManifest;
use storemodel::FileStore;
use types::FetchContext;
use types::HgId;
use types::Key;
use types::RepoPathBuf;
use types::fetch_mode::FetchMode;

/// Launch an asynchronous prefetch manager to kick of file/dir prefetches when kicked via the
/// returned channel. The prefetches are based on active fs walks, according to the walk detector
/// (prefetching content makes the serial walks go faster). The manager and any active prefetches
/// are canceled when all copies of the returned channel are dropped.
pub(crate) fn prefetch_manager(
    tree_resolver: Arc<dyn ReadTreeManifest + Send + Sync>,
    file_store: Arc<dyn FileStore>,
    current_commit_id: Arc<RwLock<Option<String>>>,
    detector: Arc<walkdetector::Detector>,
) -> flume::Sender<()> {
    // We don't need to queue up lots of kicks. A single one will suffice.
    let (send, recv) = flume::bounded(1);

    const MAX_CONCURRENT_PREFETCHES: usize = 5;

    std::thread::spawn(move || {
        // Map in-progress walks to corresponding in-progress prefetch. We allow multiple
        // in-prefetches for the same root path at different depths. This happens as the walk
        // detector witnesses deeper accesses and expands the depth boundary.
        let mut prefetches = HashMap::<(RepoPathBuf, bool), Vec<(usize, PrefetchHandle)>>::new();

        // Remember which active walks we have already finished prefetching (so we don't kick of a
        // new prefetch).
        let mut handled_prefetches = HashSet::<((RepoPathBuf, bool), usize)>::new();

        // Store the commit id we were prefetching for. When this changes (e.g. when eden checks out
        // a new commit), we drop our existing state to start fresh on the new commit.
        let mut prev_commit_id = None;

        // Wait for kicks, or otherwise check every second. The intermittent check is important
        // to notice that walks have stopped (because the kicks only happen on file/dir access,
        // which could altogether stop).
        while let Ok(_) | Err(flume::RecvTimeoutError::Timeout) =
            recv.recv_timeout(Duration::from_secs(1))
        {
            let current_commit_id = match current_commit_id.read().as_ref() {
                Some(commit_hex) => match HgId::from_hex(commit_hex.as_bytes()) {
                    Ok(hgid) => hgid,
                    Err(err) => {
                        tracing::warn!(?err, commit_hex, "invalid commit hash");
                        continue;
                    }
                },
                None => {
                    tracing::warn!("no commit when managing prefetches");
                    continue;
                }
            };

            if prev_commit_id
                .as_ref()
                .is_some_and(|id| *id != current_commit_id)
            {
                // Our "current" commit has changed. Clear existing prefetches out - they are likely
                // doing pointless prefetching based on the old commit.
                prefetches.clear();
                handled_prefetches.clear();
            }

            prev_commit_id = Some(current_commit_id);

            // Currently active walks according to the walk detector. Note that it will only report
            // a single walk for any given root (e.g. as depth deepnds, [(root="foo", depth=1)] will
            // become [(root="foo", depth=2)], _not_ including the depth=1 walk anymore).
            let active_walks: HashMap<(RepoPathBuf, bool), usize> = detector
                .file_walks()
                .into_iter()
                .map(|(path, depth)| ((path, true), depth))
                .chain(
                    detector
                        .dir_walks()
                        .into_iter()
                        .map(|(path, depth)| ((path, false), depth)),
                )
                .collect();

            // Clear entries out of `handles_prefetches` once they disappear from active walks. If
            // the walk resumes later, we should prefetch again, not assume it is "handled".
            handled_prefetches.retain(|walk: &((RepoPathBuf, bool), usize)| {
                active_walks
                    .get(&walk.0)
                    .is_some_and(|depth| *depth == walk.1)
            });

            // Cancel prefetches that are done or don't correspond to an active walk anymore.
            prefetches.retain(|walk, handles| {
                if !active_walks.contains_key(walk) {
                    // If there is no active walk for this root, clear out all prefetches (there
                    // could be multiple at different depths). This cancels the prefetches when the
                    // walk has stopped.
                    return false;
                }

                // Clear out prefetches at depths that have completed.
                handles.retain(|(_, handle)| !handle.is_canceled());

                !handles.is_empty()
            });

            for new_walk in active_walks {
                // We have already previously handled this walk.
                if handled_prefetches.contains(&new_walk) {
                    continue;
                }

                let mut depth_offset = None;

                // Check if we are already prefetching this root at a shallow depth.
                let existing_but_shallower = prefetches.get(&new_walk.0).and_then(|handles| {
                    handles.last().and_then(|(depth, _)| {
                        if depth < &new_walk.1 {
                            Some(*depth)
                        } else {
                            None
                        }
                    })
                });

                if let Some(shallower_depth) = existing_but_shallower {
                    // Keep the existing prefetch around and start the deeper prefetch so that the
                    // two don't overlap. For example, if we are currently prefetching (root="foo",
                    // depth=1) and we now see (root="foo", depth=2), we keep the first prefetch and
                    // create a new prefetch (root="foo", min_depth=1, max_depth=2).
                    depth_offset = Some(shallower_depth + 1);
                    tracing::debug!(
                        ?depth_offset,
                        ?new_walk,
                        "starting walk with additional depth offset due to existing walk"
                    );
                } else if prefetches.len() >= MAX_CONCURRENT_PREFETCHES {
                    tracing::warn!(?new_walk, "not kicking off new walk - prefetches full");
                    continue;
                }

                let mf = match tree_resolver.get(&current_commit_id) {
                    Ok(mf) => mf,
                    Err(err) => {
                        tracing::error!(?err, %current_commit_id, "error fetching root manifest");
                        continue;
                    }
                };

                handled_prefetches.insert(new_walk.clone());

                let prefetches_for_this_root = prefetches.entry(new_walk.0.clone()).or_default();

                // Kick off the actual prefetch and store its handle. The prefetch will be canceled
                // when we drop its handle.
                prefetches_for_this_root.push((
                    new_walk.1,
                    prefetch(mf, file_store.clone(), new_walk, depth_offset),
                ));

                // Make sure the prefetches stay ordered by depth.
                prefetches_for_this_root.sort_by_key(|(depth, _handle)| *depth);
            }
        }
    });

    send
}

/// Start an async prefetch of file content by walking `manifest` using `matcher`,
/// fetching file content of resultant files via `file_store`. Returns a handle
/// that will cancel the prefetch on drop.
pub(crate) fn prefetch(
    manifest: impl Manifest + Send + Sync + 'static,
    file_store: Arc<dyn FileStore>,
    // ((walk_root, fetch file contents), depth)
    walk: ((RepoPathBuf, bool), usize),
    depth_offset: Option<usize>,
) -> PrefetchHandle {
    // The cancelation works by making our thread below return early when the handle has been
    // dropped. When it returns, it drops its manifest/file iterators, which will cause those
    // operations to cancel themselves the next time they fail sending on their result channels.
    let handle = PrefetchHandle::default();

    let my_handle = handle.clone();
    // Sapling APIs are not async, so to achieve asynchronicity we create a new thread.
    std::thread::spawn(move || {
        let _span = tracing::info_span!("prefetch", ?walk, ?depth_offset).entered();

        let mut file_count = 0;
        let start_time = Instant::now();

        let dir_matcher = match TreeMatcher::from_rules(
            std::iter::once(make_glob_recursive(&plain_to_glob(walk.0.0.as_str()))),
            true,
        ) {
            Ok(matcher) => matcher,
            Err(err) => {
                tracing::error!(?err, "error constructing TreeMatcher");
                return;
            }
        };

        let start_depth = walk.0.0.components().count();
        let depth_matcher = DepthMatcher::new(
            // If we have a starting depth offset, increase the matcher's min_depth. This is so we
            // skip prefetching work that is already being handled by another active prefetch for
            // the same root at a shallow depth.
            Some(start_depth + depth_offset.unwrap_or_default()),
            Some(start_depth + walk.1),
        );

        let matcher = IntersectMatcher::new(vec![Arc::new(dir_matcher), Arc::new(depth_matcher)]);

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

            let span = tracing::debug_span!("prefetching file content", batch_size = batch.len());
            let _span = span.enter();

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

            // Skip the file content fetching if we are only prefetching directories.
            if !walk.0.1 {
                continue;
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

    use anyhow::anyhow;
    use manifest::FileMetadata;
    use manifest_tree::TreeManifest;
    use manifest_tree::testutil::TestStore;
    use rand_chacha::ChaChaRng;
    use rand_chacha::rand_core::SeedableRng;

    use super::*;

    #[test]
    fn test_prefetch_files() -> anyhow::Result<()> {
        let store = Arc::new(TestStore::new());

        let file_store = store.clone() as Arc<dyn FileStore>;

        let foo_path: RepoPathBuf = "dir/foo".to_string().try_into()?;
        let bar_path: RepoPathBuf = "dir/dir2/bar".to_string().try_into()?;

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

        // Prefetch for "dir/" at depth=0 (i.e. "dir/*").
        let handle = prefetch(
            mf.clone(),
            file_store.clone(),
            (("dir".to_string().try_into()?, true), 0),
            None,
        );

        // Wait for prefetch to complete.
        while !handle.is_canceled() {
            std::thread::sleep(Duration::from_millis(1));
        }

        // We only fetched "foo" (due to the depth limit).
        assert_eq!(
            store.fetches(),
            vec![Key::new(RepoPathBuf::new(), foo_hgid)]
        );

        // Prefetch for "dir/" at min_depth=1, max_depth=1 (i.e. "dir/dir2/*").
        let handle = prefetch(
            mf,
            file_store,
            (("dir".to_string().try_into()?, true), 1),
            Some(1),
        );

        // Wait for prefetch to complete.
        while !handle.is_canceled() {
            std::thread::sleep(Duration::from_millis(1));
        }

        // We only additionally fetched "dir/dir2/bar" (i.e. did not redundantly fetch "dir/foo".
        assert_eq!(
            store.fetches(),
            vec![
                Key::new(RepoPathBuf::new(), foo_hgid),
                Key::new(RepoPathBuf::new(), bar_hgid)
            ]
        );

        Ok(())
    }

    struct StubTreeResolver(HashMap<HgId, TreeManifest>);

    impl ReadTreeManifest for StubTreeResolver {
        fn get(&self, commit_id: &HgId) -> anyhow::Result<TreeManifest> {
            self.0
                .get(commit_id)
                .ok_or(anyhow!("no manifest for {commit_id}"))
                .cloned()
        }

        fn get_root_id(&self, _commit_id: &HgId) -> anyhow::Result<HgId> {
            unimplemented!()
        }
    }

    #[test]
    fn test_prefetch_manager() -> anyhow::Result<()> {
        let detector = Arc::new(walkdetector::Detector::new());
        detector.set_min_dir_walk_threshold(2);

        let store = Arc::new(TestStore::new());

        let file_store = store.clone() as Arc<dyn FileStore>;

        let foo_path: RepoPathBuf = "dir/foo".to_string().try_into()?;
        let bar_path: RepoPathBuf = "dir/bar".to_string().try_into()?;

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

        let mut rng = ChaChaRng::from_seed([0u8; 32]);
        let stub_commit_id = HgId::random(&mut rng);

        let tree_resolver = StubTreeResolver([(stub_commit_id, mf)].into());

        let kick_manager = prefetch_manager(
            Arc::new(tree_resolver),
            file_store,
            Arc::new(RwLock::new(Some(stub_commit_id.to_hex()))),
            detector.clone(),
        );

        // Trigger a walk.
        detector.file_read(&foo_path);
        detector.file_read(&bar_path);

        kick_manager.send(())?;

        // Wait for prefetch to finish.
        for _ in 0..10 {
            if store.key_fetch_count() != 2 {
                std::thread::sleep(Duration::from_millis(1));
            }
        }

        let mut fetches = store.fetches();
        fetches.sort();
        assert_eq!(
            fetches,
            vec![
                Key::new(RepoPathBuf::new(), foo_hgid),
                Key::new(RepoPathBuf::new(), bar_hgid)
            ]
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
