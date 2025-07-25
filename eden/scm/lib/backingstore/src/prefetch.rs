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
use std::sync::atomic::AtomicU8;
use std::sync::atomic::Ordering;
use std::time::Duration;
use std::time::Instant;

use manifest::Manifest;
use manifest_tree::TreeManifest;
use parking_lot::RwLock;
use pathmatcher::DepthMatcher;
use pathmatcher::DirectoryMatch;
use pathmatcher::DynMatcher;
use pathmatcher::Matcher;
use pathmatcher::UnionMatcher;
use repo::ReadTreeManifest;
use storemodel::FileStore;
use types::FetchContext;
use types::HgId;
use types::Key;
use types::RepoPath;
use types::RepoPathBuf;
use types::fetch_cause::FetchCause;
use types::fetch_mode::FetchMode;
use walkdetector::WalkType;

macro_rules! info_if {
    ($cond:expr, $kind:ident, $($rest:tt)*) => {
        if $cond {
            tracing::$kind!(tracing::Level::INFO, $($rest)*)
        } else {
            tracing::$kind!(tracing::Level::DEBUG, $($rest)*)
        }
    };
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Config {
    pub(crate) max_initial_lag: u64,
    pub(crate) min_ratio: f64,
    pub(crate) min_interval: Duration,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            max_initial_lag: 1000,
            min_ratio: 0.1,
            min_interval: Duration::from_millis(10),
        }
    }
}

/// Launch an asynchronous prefetch manager to kick of file/dir prefetches when kicked via the
/// returned channel. The prefetches are based on active fs walks, according to the walk detector
/// (prefetching content makes the serial walks go faster). The manager and any active prefetches
/// are canceled when all copies of the returned channel are dropped.
pub(crate) fn prefetch_manager(
    config: Config,
    tree_resolver: Arc<dyn ReadTreeManifest>,
    file_store: Arc<dyn FileStore>,
    current_commit_id: Arc<RwLock<Option<String>>>,
    detector: walkdetector::Detector,
) -> flume::Sender<()> {
    // We don't need to queue up lots of kicks. A single one will suffice.
    let (send, recv) = flume::bounded(1);

    const MAX_CONCURRENT_PREFETCHES: usize = 5;

    std::thread::spawn(move || {
        // Map in-progress walks to corresponding in-progress prefetch. We allow multiple
        // in-progress prefetches for the same root path at different depths. This happens as the walk
        // detector witnesses deeper accesses and expands the depth boundary.
        let mut prefetches = HashMap::<(RepoPathBuf, bool), Vec<(usize, PrefetchHandle)>>::new();

        // Big batches of shallow depth directory prefetches. We don't address the prefetches
        // per-walk because, since they are batched, there is no way to cancel individual
        // prefetches, anyway.
        let mut batch_dir_prefetches = Vec::<PrefetchHandle>::new();

        // Remember which active walks we have already finished prefetching (so we don't kick of a
        // new prefetch).
        let mut handled_prefetches = HashSet::<((RepoPathBuf, bool), usize)>::new();

        // Store the commit id we were prefetching for. When this changes (e.g. when eden checks out
        // a new commit), we drop our existing state to start fresh on the new commit.
        let mut prev_commit_id = None;

        // Shared TreeManifest object to avoid duplicative tree fetching/deserialization.
        let mut current_manifest: Option<TreeManifest> = None;

        let mut last_iteration_time = None;

        // Wait for kicks, or otherwise check every second. The intermittent check is important
        // to notice that walks have stopped (because the kicks only happen on file/dir access,
        // which could altogether stop).
        while let Ok(_) | Err(flume::RecvTimeoutError::Timeout) =
            recv.recv_timeout(Duration::from_secs(1))
        {
            // Add a small sleep to make sure we aren't busy looping when there is non-stop walk activity.
            let now = Instant::now();
            if last_iteration_time
                .replace(now)
                .is_some_and(|t| now.duration_since(t) < config.min_interval)
            {
                std::thread::sleep(config.min_interval);
            }

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
                tracing::info!(
                    ?prev_commit_id,
                    ?current_commit_id,
                    "clearing out state for new commit id"
                );
                // Our "current" commit has changed. Clear existing prefetches out - they are likely
                // doing pointless prefetching based on the old commit.
                prefetches.clear();
                handled_prefetches.clear();
                batch_dir_prefetches.clear();
                current_manifest.take();
            }

            prev_commit_id = Some(current_commit_id);

            // Currently active walks according to the walk detector. Note that it will only report
            // a single walk for any given root (e.g. as depth deepens, [(root="foo", depth=1)] will
            // become [(root="foo", depth=2)], _not_ including the depth=1 walk anymore).
            // Each walk is represented as `((root_path, wants_file_content), depth)`.
            let active_walks: HashMap<(RepoPathBuf, bool), usize> = detector
                .all_walks()
                .into_iter()
                .map(|(path, depth, wt)| ((path, wt == WalkType::File), depth))
                .collect();

            if active_walks.is_empty() {
                // If we are done with walks (for now), then drop our cached manifest. It could be
                // holding a lot of trees in memory.
                current_manifest.take();
            }

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
                    tracing::debug!(?walk, "canceling prefetch with no corresponding walk");
                    // If there is no active walk for this root, clear out all prefetches (there
                    // could be multiple at different depths). This cancels the prefetches when the
                    // walk has stopped.
                    return false;
                }

                let any_paused = handles.iter().any(|(_, handle)| handle.is_paused());

                // Clear out prefetches at depths that have completed.
                handles.retain(|(depth, handle)| {
                    if any_paused {
                        // Prefetch paused - don't retain handle, and clear from `handled_prefetches`
                        // so it will be resumed as a new prefetch when possible.
                        handled_prefetches.remove(&(walk.clone(), *depth));
                        false
                    } else {
                        !handle.is_done()
                    }
                });

                if handles.is_empty() {
                    tracing::debug!(?walk, "removing complete prefetch");
                    false
                } else {
                    true
                }
            });

            batch_dir_prefetches.retain(|handle| !handle.is_done());

            // Reuse a cached TreeManifest if available. It is thread safe and cheaply cloneable
            // when used read-only.
            let mf: TreeManifest = match current_manifest {
                Some(ref mf) => mf.clone(),
                None => match tree_resolver.get(&current_commit_id) {
                    Ok(mf) => {
                        current_manifest = Some(mf.clone());
                        mf
                    }
                    Err(err) => {
                        tracing::error!(?err, %current_commit_id, "error fetching root manifest");
                        continue;
                    }
                },
            };

            const SHALLOW_PREFETCH_BATCH_SIZE: usize = 100;
            let mut batchable_walks = Vec::<(RepoPathBuf, usize)>::new();
            let mut batch_prefetch =
                |batch: &mut Vec<(RepoPathBuf, usize)>, handled: &mut HashSet<_>| {
                    if batch.is_empty() {
                        return;
                    }

                    if batch_dir_prefetches.len() >= MAX_CONCURRENT_PREFETCHES {
                        tracing::debug!(
                            batch_size = batch.len(),
                            "not kicking off new batch of shallow dir walks - prefetches full"
                        );
                    } else {
                        for (root, depth) in batch.iter() {
                            handled.insert(((root.clone(), false), *depth));
                        }
                        batch_dir_prefetches.push(prefetch(
                            config,
                            mf.clone(),
                            file_store.clone(),
                            detector.clone(),
                            PrefetchWork::DirectoriesOnly(std::mem::take(batch)),
                        ));
                    }
                };

            for new_walk in active_walks {
                // We have already previously handled this walk.
                if handled_prefetches.contains(&new_walk) {
                    continue;
                }

                // Shallow, directory-only walk - batch them together for better throughput.
                if !new_walk.0.1 && new_walk.1 <= 2 {
                    batchable_walks.push((new_walk.0.0, new_walk.1));
                    if batchable_walks.len() >= SHALLOW_PREFETCH_BATCH_SIZE {
                        batch_prefetch(&mut batchable_walks, &mut handled_prefetches);
                    }
                    continue;
                }

                let mut depth_offset = None;

                // Check if we are already prefetching this root at a shallower depth.
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
                    // create a new prefetch (root="foo", min_depth=2, max_depth=2).
                    depth_offset = Some(shallower_depth + 1);
                    tracing::debug!(
                        ?depth_offset,
                        ?new_walk,
                        "starting walk with additional depth offset due to existing walk"
                    );
                } else if prefetches.len() >= MAX_CONCURRENT_PREFETCHES {
                    tracing::debug!(?new_walk, "not kicking off new walk - prefetches full");
                    continue;
                }

                if should_pause_prefetch(&detector, &config, &new_walk.0.0) {
                    tracing::debug!(?new_walk, "not kicking off new walk - prefetching paused");
                    continue;
                }

                handled_prefetches.insert(new_walk.clone());

                let prefetches_for_this_root = prefetches.entry(new_walk.0.clone()).or_default();

                tracing::debug!(?new_walk, "kicking off prefetch");

                let work = if new_walk.0.1 {
                    PrefetchWork::FileContent(new_walk.0.0, new_walk.1, depth_offset)
                } else {
                    PrefetchWork::DirectoriesOnly(vec![(new_walk.0.0, new_walk.1)])
                };

                // Kick off the actual prefetch and store its handle. The prefetch will be canceled
                // when we drop its handle.
                prefetches_for_this_root.push((
                    new_walk.1,
                    prefetch(
                        config,
                        mf.clone(),
                        file_store.clone(),
                        detector.clone(),
                        work,
                    ),
                ));

                // Make sure the prefetches stay ordered by depth.
                prefetches_for_this_root.sort_by_key(|(depth, _handle)| *depth);
            }

            batch_prefetch(&mut batchable_walks, &mut handled_prefetches);
        }
    });

    send
}

// Arbitrary cutoffs for more verbose logging.
const INTERESTING_DEPTH: usize = 7;
const INTERESTING_NUMBER_OF_SMALL_WALKS: usize = 50;

enum PrefetchWork {
    DirectoriesOnly(Vec<(RepoPathBuf, usize)>),
    FileContent(RepoPathBuf, usize, Option<usize>),
}

/// Start an async prefetch of file content by walking `manifest` using `matcher`,
/// fetching file content of resultant files via `file_store`. Returns a handle
/// that will cancel the prefetch on drop.
fn prefetch(
    config: Config,
    manifest: impl Manifest + Send + Sync + 'static,
    file_store: Arc<dyn FileStore>,
    walk_detector: walkdetector::Detector,
    work: PrefetchWork,
) -> PrefetchHandle {
    // The cancellation works by making our thread below return early when the handle has been
    // dropped. When it returns, it drops its manifest/file iterators, which will cause those
    // operations to cancel themselves the next time they fail sending on their result channels.
    let handle = PrefetchHandle::default();

    let my_handle = handle.clone();
    // Sapling APIs are not async, so to achieve asynchronicity we create a new thread.
    std::thread::spawn(move || {
        let (file_content_root, matcher, span) = match work {
            PrefetchWork::DirectoriesOnly(walks) => {
                let Some((first_root, first_depth)) = walks.first() else {
                    return;
                };

                let span = info_if!(
                    walks.len() >= INTERESTING_NUMBER_OF_SMALL_WALKS || *first_depth >= INTERESTING_DEPTH,
                    span,
                    "dir prefetch",
                    num_dir_walks=walks.len(),
                    %first_root,
                    first_depth,
                )
                .entered();

                let mut matchers = Vec::new();
                for (walk_root, walk_depth) in walks {
                    matchers.push(Arc::new(WalkMatcher::new(
                        walk_root,
                        // The DepthMatcher is relative to file path depth. To fetch directories at depth=N, we
                        // need to pretend we are fetching files at depth=N+1.
                        walk_depth + 1,
                        None,
                    )) as DynMatcher);
                }
                (None, UnionMatcher::new_or_single(matchers), span)
            }
            PrefetchWork::FileContent(walk_root, depth, depth_offset) => {
                let span = info_if!(
                    depth >= INTERESTING_DEPTH,
                    span,
                    "file prefetch",
                    %walk_root,
                    depth,
                )
                .entered();

                (
                    Some(walk_root.clone()),
                    Arc::new(WalkMatcher::new(walk_root, depth, depth_offset)) as DynMatcher,
                    span,
                )
            }
        };

        // Piggy back on the original decision of whether this is an interesting (info level) prefetch or not.
        let interesting_walk = span
            .metadata()
            .is_some_and(|m| m.level() == &tracing::Level::INFO);

        let mut file_count = 0;
        let start_time = Instant::now();

        // This iterator is populated async by the manifest iterator.
        let files = manifest.files(matcher);

        const BATCH_SIZE: usize = 10_000;
        let mut batch = Vec::new();

        // Allow multiple concurrent file fetches to stack up.
        const MAX_CONCURRENT_FILE_FETCHES: usize = 5;
        let mut file_fetches: VecDeque<(FetchContext, Box<dyn Iterator<Item = _>>)> =
            VecDeque::new();

        let consume_file_fetch = |(fctx, iter): (FetchContext, Box<dyn Iterator<Item = _>>)| {
            iter.for_each(drop);
            if let Some(walk_root) = &file_content_root {
                walk_detector.files_preloaded(walk_root, fctx.remote_fetch_count());

                if should_pause_prefetch(&walk_detector, &config, walk_root) {
                    info_if!(interesting_walk, event, "pausing prefetch");
                    my_handle.pause();
                }
            }
        };

        let mut fetch_batch = |batch: &mut Vec<Key>| {
            if batch.is_empty() {
                return;
            }

            let span = tracing::debug_span!("prefetching file content", batch_size = batch.len());
            let _span = span.enter();

            file_count += batch.len();

            // If file fetches are full, wait for first one to finish.
            if file_fetches.len() >= MAX_CONCURRENT_FILE_FETCHES {
                consume_file_fetch(file_fetches.pop_front().unwrap());
            }

            // Check for cancellation since the consume_file_fetch() call can cancel the prefetch.
            if !my_handle.is_in_progress() {
                info_if!(interesting_walk, event, elapsed=?start_time.elapsed(), file_count, "prefetch canceled");
                return;
            }

            // Use IGNORE_RESULT optimization since we don't care about the data.
            let fctx = FetchContext::new_with_cause(
                FetchMode::AllowRemote | FetchMode::IGNORE_RESULT,
                FetchCause::EdenWalkPrefetch,
            );

            // An important implementation detail for us: the scmstore FileStore spawns a thread
            // when you fetch more than 1_000 keys (i.e. this method will operate asynchronously if
            // we fetch more than 1k files). If that assumption changes, we will need to change what
            // we do here.
            let fetch_res = file_store.get_content_iter(fctx.clone(), mem::take(batch));

            match fetch_res {
                Ok(iter) => {
                    file_fetches.push_back((fctx, iter));
                }
                Err(err) => {
                    tracing::error!(?err, "error prefetching file content");
                }
            }
        };

        for file in files {
            if !my_handle.is_in_progress() {
                info_if!(interesting_walk, event, elapsed=?start_time.elapsed(), file_count, "prefetch canceled");
                return;
            }

            // Skip the file content fetching if we are only prefetching directories.
            if file_content_root.is_none() {
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
            if !my_handle.is_in_progress() {
                info_if!(interesting_walk, event, elapsed=?start_time.elapsed(), file_count, "prefetch canceled");
                return;
            }

            consume_file_fetch(fetch);
        }

        // Make sure this doesn't get dropped early.
        drop(my_handle);

        info_if!(interesting_walk, event, elapsed=?start_time.elapsed(), file_count, "prefetch complete");
    });

    handle
}

/// Determine whether prefetching for `walk_root` should be paused by considering how much we have
/// prefetched so far, and how much of our prefetched data has actually been read by the walker.
fn should_pause_prefetch(
    detector: &walkdetector::Detector,
    config: &Config,
    walk_root: &RepoPath,
) -> bool {
    let (prefetch_count, read_count) = detector.files_preloaded(walk_root, 0);
    tracing::debug!(%walk_root, prefetch_count, read_count, "should_pause_prefetch");

    if prefetch_count == 0 {
        return false;
    }

    let prefetch_ratio = read_count as f64 / prefetch_count as f64;
    if prefetch_ratio > config.min_ratio {
        // If we haven't gone below our allowed ratio, keep prefetching.
        return false;
    }

    // Ratio isn't good - check if the absolute value of lag is too big. There is no guarantee that
    // files are read in the order we prefetch them, so we need to allow for sizeable lag. At the
    // same time, we want to prevent crazy over prefetching.
    config.max_initial_lag > 0 && prefetch_count - read_count > config.max_initial_lag
}

// A pathmatcher::Matcher impl that matches a simple directory prefix along with a depth filter.
struct WalkMatcher {
    depth_matcher: DepthMatcher,
    dir: RepoPathBuf,
}

impl WalkMatcher {
    fn new(dir: RepoPathBuf, walk_depth: usize, depth_offset: Option<usize>) -> Self {
        let start_depth = dir.depth();
        Self {
            depth_matcher: DepthMatcher::new(
                // If we have a starting depth offset, increase the matcher's min_depth. This is so we
                // skip prefetching work that is already being handled by another active prefetch for
                // the same root at a shallow depth.
                Some(start_depth + depth_offset.unwrap_or_default()),
                Some(start_depth + walk_depth),
            ),
            dir,
        }
    }
}

impl Matcher for WalkMatcher {
    fn matches_directory(&self, path: &RepoPath) -> anyhow::Result<DirectoryMatch> {
        // Check that either path is a prefix of self.dir (i.e. later might be under self.dir), or
        // self.dir is a prefix of path (i.e. we are under self.dir).
        if !path.starts_with(&self.dir, true) && !self.dir.starts_with(path, true) {
            return Ok(DirectoryMatch::Nothing);
        }

        self.depth_matcher.matches_directory(path)
    }

    fn matches_file(&self, path: &RepoPath) -> anyhow::Result<bool> {
        if !path.starts_with(&self.dir, true) {
            return Ok(false);
        }
        self.depth_matcher.matches_file(path)
    }
}

#[derive(Clone, Default, Debug)]
pub(crate) struct PrefetchHandle {
    state: Arc<AtomicU8>,
}

impl PrefetchHandle {
    const IN_PROGRESS: u8 = 0;
    const DONE: u8 = 1;
    const PAUSED: u8 = 2;

    pub(crate) fn cancel(&self) {
        let _ = self.state.compare_exchange(
            Self::IN_PROGRESS,
            Self::DONE,
            Ordering::AcqRel,
            Ordering::Relaxed,
        );
    }

    pub(crate) fn pause(&self) {
        let _ = self.state.compare_exchange(
            Self::IN_PROGRESS,
            Self::PAUSED,
            Ordering::AcqRel,
            Ordering::Relaxed,
        );
    }

    pub(crate) fn is_done(&self) -> bool {
        self.state.load(Ordering::Acquire) == Self::DONE
    }

    pub(crate) fn is_paused(&self) -> bool {
        self.state.load(Ordering::Acquire) == Self::PAUSED
    }

    pub(crate) fn is_in_progress(&self) -> bool {
        self.state.load(Ordering::Acquire) == Self::IN_PROGRESS
    }
}

impl Drop for PrefetchHandle {
    fn drop(&mut self) {
        self.cancel();
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
    use storemodel::KeyStore;

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

        let detector = walkdetector::Detector::new();

        // Prefetch for "dir/" at depth=0 (i.e. "dir/*").
        let handle = prefetch(
            Config::default(),
            mf.clone(),
            file_store.clone(),
            detector.clone(),
            PrefetchWork::FileContent("dir".to_string().try_into()?, 0, None),
        );

        // Wait for prefetch to complete.
        while !handle.is_done() {
            std::thread::sleep(Duration::from_millis(1));
        }

        // We only fetched "foo" (due to the depth limit).
        assert_eq!(
            store.fetches(),
            vec![Key::new(RepoPathBuf::new(), foo_hgid)]
        );

        // Prefetch for "dir/" at min_depth=1, max_depth=1 (i.e. "dir/dir2/*").
        let handle = prefetch(
            Config::default(),
            mf,
            file_store,
            detector,
            PrefetchWork::FileContent("dir".to_string().try_into()?, 1, Some(1)),
        );

        // Wait for prefetch to complete.
        while !handle.is_done() {
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

    #[test]
    fn test_prefetch_dirs() -> anyhow::Result<()> {
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

        for (path, _id, text, _p1, _p2) in mf.finalize(Vec::new())? {
            store.insert_data(Default::default(), &path, text.as_ref())?;
        }

        let detector = walkdetector::Detector::new();

        // Prefetch directories for "" at depth=0 (i.e. "*"). This should fetch directory "dir".
        let handle = prefetch(
            Config::default(),
            mf.clone(),
            file_store.clone(),
            detector.clone(),
            PrefetchWork::DirectoriesOnly(vec![("".to_string().try_into()?, 0)]),
        );

        // Wait for prefetch to complete.
        while !handle.is_done() {
            std::thread::sleep(Duration::from_millis(1));
        }

        // We fetched the root dir and "dir/" (but not "dir/dir2").
        assert_eq!(
            store
                .prefetches()
                .into_iter()
                .flatten()
                .map(|k| k.path)
                .collect::<Vec<RepoPathBuf>>(),
            vec!["".to_string().try_into()?, "dir".to_string().try_into()?]
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
        let mut detector = walkdetector::Detector::new();
        detector.set_walk_threshold(2);

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
            Config::default(),
            Arc::new(tree_resolver),
            file_store,
            Arc::new(RwLock::new(Some(stub_commit_id.to_hex()))),
            detector.clone(),
        );

        // Trigger a walk.
        detector.file_loaded(&foo_path, 0);
        detector.file_loaded(&bar_path, 0);

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
    fn test_batch_prefetch_dirs() -> anyhow::Result<()> {
        let store = Arc::new(TestStore::new());

        let file_store = store.clone() as Arc<dyn FileStore>;

        let foo_path: RepoPathBuf = "dir1/foo".to_string().try_into()?;
        let bar_path: RepoPathBuf = "dir2/bar".to_string().try_into()?;

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

        for (path, _id, text, _p1, _p2) in mf.finalize(Vec::new())? {
            store.insert_data(Default::default(), &path, text.as_ref())?;
        }

        let mut detector = walkdetector::Detector::new();
        detector.set_walk_threshold(2);

        let mut rng = ChaChaRng::from_seed([0u8; 32]);
        let stub_commit_id = HgId::random(&mut rng);

        let tree_resolver = StubTreeResolver([(stub_commit_id, mf)].into());

        let kick_manager = prefetch_manager(
            Config::default(),
            Arc::new(tree_resolver),
            file_store,
            Arc::new(RwLock::new(Some(stub_commit_id.to_hex()))),
            detector.clone(),
        );

        // Trigger a directory walk for each of "dir1" and "dir2".
        detector.dir_loaded(RepoPath::from_static_str("dir1/a"), 0, 0, 0);
        detector.dir_loaded(RepoPath::from_static_str("dir1/b"), 0, 0, 0);
        detector.dir_loaded(RepoPath::from_static_str("dir2/a"), 0, 0, 0);
        detector.dir_loaded(RepoPath::from_static_str("dir2/b"), 0, 0, 0);

        kick_manager.send(())?;

        // Wait for prefetch to finish.
        for _ in 0..10 {
            if store.prefetches().is_empty() {
                std::thread::sleep(Duration::from_millis(1));
            }
        }

        // Check that we fetched both "dir1" and "dir2" in the same batch.
        assert_eq!(
            store
                .prefetches()
                .into_iter()
                .map(|batch| batch.into_iter().map(|k| k.path).collect())
                .collect::<Vec<Vec<RepoPathBuf>>>(),
            vec![
                vec!["".to_string().try_into()?],
                vec![
                    "dir1".to_string().try_into()?,
                    "dir2".to_string().try_into()?
                ],
            ],
        );

        Ok(())
    }

    #[test]
    fn test_prefetch_handle() {
        let handle = PrefetchHandle::default();
        assert!(handle.is_in_progress());
        assert!(!handle.is_done());
        assert!(!handle.is_paused());

        drop(handle.clone());
        assert!(handle.is_done());
        assert!(!handle.is_in_progress());
        assert!(!handle.is_paused());

        // Has no effect.
        handle.pause();
        assert!(handle.is_done());
        assert!(!handle.is_in_progress());
        assert!(!handle.is_paused());

        let handle = PrefetchHandle::default();
        handle.pause();
        assert!(!handle.is_done());
        assert!(!handle.is_in_progress());
        assert!(handle.is_paused());
    }

    #[test]
    fn test_should_pause_prefetch() -> anyhow::Result<()> {
        let config = Config {
            min_ratio: 0.1,
            max_initial_lag: 20,
            min_interval: Duration::from_millis(1),
        };

        let mut detector = walkdetector::Detector::new();
        detector.set_walk_threshold(2);

        let walk_root: RepoPathBuf = "root".to_string().try_into()?;

        assert!(!should_pause_prefetch(&detector, &config, &walk_root));

        detector.file_loaded(RepoPath::from_str("root/a")?, 0);
        detector.file_loaded(RepoPath::from_str("root/b")?, 0);
        detector.file_read(RepoPath::from_str("root/c")?, 0);

        assert!(!should_pause_prefetch(&detector, &config, &walk_root));

        detector.files_preloaded(&walk_root, 15);

        // Still not paused - lag is below 20.
        assert!(!should_pause_prefetch(&detector, &config, &walk_root));

        // Now we are paused
        detector.files_preloaded(&walk_root, 10);
        assert!(should_pause_prefetch(&detector, &config, &walk_root));

        for i in 0..100 {
            detector.file_read(RepoPath::from_str(&format!("root/{i})"))?, 0);
        }
        detector.files_preloaded(&walk_root, 500);
        // Ratio is ~100/500 - above min_ratio of 0.1.
        assert!(!should_pause_prefetch(&detector, &config, &walk_root));

        // Now we are below the min ratio.
        detector.files_preloaded(&walk_root, 700);
        assert!(should_pause_prefetch(&detector, &config, &walk_root));

        Ok(())
    }
}
