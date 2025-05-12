/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#[cfg(test)]
mod tests;
mod walk_node;

use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::time::Duration;
use std::time::Instant;

use parking_lot::Mutex;
use types::RepoPath;
use types::RepoPathBuf;
use walk_node::WalkNode;

// Goals:
//  - Aggressively detect walk and aggressively cancel walk.
//  - Passive - don't fetch or query any stores.
//  - Minimize memory usage.

pub struct Detector {
    inner: Mutex<Inner>,
}

struct Inner {
    min_dir_walk_threshold: usize,
    last_gc_time: Instant,
    gc_interval: Duration,
    gc_timeout: Duration,
    node: WalkNode,
    stub_now: Option<Instant>,
}

impl Default for Inner {
    fn default() -> Self {
        Self {
            min_dir_walk_threshold: DEFAULT_MIN_DIR_WALK_THRESHOLD,
            last_gc_time: Instant::now(),
            gc_interval: DEFAULT_GC_INTERVAL,
            gc_timeout: DEFAULT_GC_TIMEOUT,
            node: WalkNode::default(),
            stub_now: None,
        }
    }
}

#[derive(Default, Debug)]
pub struct Walk {
    depth: usize,
    file_loads: AtomicU64,
    file_reads: AtomicU64,
    dir_loads: AtomicU64,
    dir_reads: AtomicU64,
    logged_start: AtomicBool,
}

impl Walk {
    const BIG_WALK_THRESHOLD: u64 = 10_000;

    fn new(depth: usize) -> Self {
        Self {
            depth,
            ..Default::default()
        }
    }

    fn for_type(t: WalkType, depth: usize, initial_loads: u64) -> Self {
        let w = Self::new(depth);
        let counter = match t {
            WalkType::Directory => &w.dir_loads,
            WalkType::File => &w.file_loads,
        };
        counter.fetch_add(initial_loads, Ordering::Relaxed);
        w
    }

    fn absorb_counters(&self, other: &Self) {
        // Race conditions are not a big deal.
        if other.logged_start.load(Ordering::Relaxed) {
            self.logged_start.store(true, Ordering::Relaxed);
        }

        // Will not notice beginning of "big walk", but next file access (or GC) will.
        self.file_loads
            .fetch_add(other.file_loads.load(Ordering::Relaxed), Ordering::AcqRel);
        self.dir_loads
            .fetch_add(other.dir_loads.load(Ordering::Relaxed), Ordering::AcqRel);
        self.file_reads
            .fetch_add(other.file_reads.load(Ordering::Relaxed), Ordering::AcqRel);
        self.dir_reads
            .fetch_add(other.dir_reads.load(Ordering::Relaxed), Ordering::AcqRel);
    }

    fn inc_file_load(&self, root: &RepoPath, val: u64) {
        if self.file_loads.fetch_add(val, Ordering::AcqRel) == Self::BIG_WALK_THRESHOLD - 1 {
            self.maybe_log_big_walk(root);
        }
    }

    fn inc_file_read(&self, root: &RepoPath, val: u64) {
        if self.file_reads.fetch_add(val, Ordering::Relaxed) == Self::BIG_WALK_THRESHOLD - 1 {
            self.maybe_log_big_walk(root);
        }
    }

    fn inc_dir_load(&self, root: &RepoPath, val: u64) {
        if self.dir_loads.fetch_add(val, Ordering::AcqRel) == Self::BIG_WALK_THRESHOLD - 1 {
            self.maybe_log_big_walk(root);
        }
    }

    fn inc_dir_read(&self, root: &RepoPath, val: u64) {
        if self.dir_reads.fetch_add(val, Ordering::Relaxed) == Self::BIG_WALK_THRESHOLD - 1 {
            self.maybe_log_big_walk(root);
        }
    }

    fn maybe_log_big_walk(&self, root: &RepoPath) {
        if self.logged_start.swap(true, Ordering::AcqRel) {
            return;
        }

        tracing::info!(
            %root,
            file_loads = self.file_loads.load(Ordering::Relaxed),
            file_reads = self.file_reads.load(Ordering::Relaxed),
            dir_loads = self.dir_loads.load(Ordering::Relaxed),
            dir_reads = self.dir_reads.load(Ordering::Relaxed),
            "big walk started",
        );
    }

    fn log_end(&self, root: &RepoPath) {
        if self.file_loads.load(Ordering::Relaxed) >= Self::BIG_WALK_THRESHOLD
            || self.file_reads.load(Ordering::Relaxed) >= Self::BIG_WALK_THRESHOLD
            || self.dir_loads.load(Ordering::Relaxed) >= Self::BIG_WALK_THRESHOLD
            || self.dir_reads.load(Ordering::Relaxed) >= Self::BIG_WALK_THRESHOLD
        {
            tracing::info!(
                %root,
                file_loads = self.file_loads.load(Ordering::Relaxed),
                file_reads = self.file_reads.load(Ordering::Relaxed),
                dir_loads = self.dir_loads.load(Ordering::Relaxed),
                dir_reads = self.dir_reads.load(Ordering::Relaxed),
                "big walk ended",
            );
        }
    }

    #[cfg(test)]
    fn counters(&self) -> (u64, u64, u64, u64) {
        (
            self.file_loads.load(Ordering::Relaxed),
            self.file_reads.load(Ordering::Relaxed),
            self.dir_loads.load(Ordering::Relaxed),
            self.dir_reads.load(Ordering::Relaxed),
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WalkType {
    File,
    Directory,
}

// How many children must be accessed in a directory to consider the directory "walked".
const DEFAULT_MIN_DIR_WALK_THRESHOLD: usize = 2;

// How often we garbage collect stale walks.
const DEFAULT_GC_INTERVAL: Duration = Duration::from_secs(5);

// How stale a walk must be before we remove it.
const DEFAULT_GC_TIMEOUT: Duration = Duration::from_secs(5);

impl Detector {
    pub fn new() -> Self {
        Self {
            inner: Default::default(),
        }
    }

    pub fn set_min_dir_walk_threshold(&self, threshold: usize) {
        self.inner.lock().min_dir_walk_threshold = threshold;
    }

    #[cfg(test)]
    pub fn set_now(&self, now: Instant) {
        self.inner.lock().stub_now = Some(now);
    }

    pub fn set_gc_interval(&self, interval: Duration) {
        self.inner.lock().gc_interval = interval;
    }

    pub fn set_gc_timeout(&self, timeout: Duration) {
        self.inner.lock().gc_timeout = timeout;
    }

    /// Return list of (walk root dir, walk depth) representing active file content walks.
    pub fn file_walks(&self) -> Vec<(RepoPathBuf, usize)> {
        self.walks(WalkType::File)
    }

    /// Return list of (walk root dir, walk depth) representing active directory walks.
    pub fn dir_walks(&self) -> Vec<(RepoPathBuf, usize)> {
        self.walks(WalkType::Directory)
    }

    fn walks(&self, walk_type: WalkType) -> Vec<(RepoPathBuf, usize)> {
        let mut inner = self.inner.lock();

        let time = inner.now();
        inner.maybe_gc(time);

        let mut walks = inner.node.list_walks(walk_type);

        walks.sort_by(|a, b| a.0.as_str().cmp(b.0.as_str()));

        walks
    }

    /// Observe a "heavy" or remote file (content) read of `path`.
    /// Returns whether an active walk changed (either created or removed).
    pub fn file_loaded(&self, path: impl AsRef<RepoPath>) -> bool {
        let path = path.as_ref();

        tracing::trace!(%path, "file_loaded");

        let (dir_path, base_name) = match path.split_last_component() {
            // Shouldn't happen - implies a path of "" which is not valid for a file.
            None => return false,
            Some((dir, base)) => (dir, base),
        };

        let mut inner = self.inner.lock();

        let time = inner.now();

        let mut walk_changed = inner.maybe_gc(time);

        let dir_threshold = inner.min_dir_walk_threshold;

        let (owner, suffix) = inner
            .node
            .get_or_create_owning_node(WalkType::File, dir_path);

        owner.last_access = Some(time);

        if let Some(walk) = owner.get_dominating_walk(WalkType::File) {
            tracing::trace!(walk_root=%dir_path.strip_suffix(suffix, true).unwrap_or_default(), dir=%dir_path, "file's dir already in walk");
            walk.inc_file_load(dir_path.strip_suffix(suffix, true).unwrap_or_default(), 1);
            return walk_changed;
        }

        let my_dir = owner;

        my_dir.seen_files.insert(base_name.to_owned());

        if my_dir.is_walked(WalkType::File, dir_threshold) {
            let file_count = my_dir.seen_files.len();
            my_dir.seen_files.clear();
            inner.insert_walk(
                time,
                WalkType::File,
                Walk::for_type(WalkType::File, 0, file_count as u64),
                dir_path,
            );
            walk_changed = true;
        }

        walk_changed
    }

    /// Observe a "soft" or cached file (content) access of `path`.
    /// This will not be tracked as a new walk, but will reset TTL of an existing walk.
    /// Returns whether an active walk changed (due to GC).
    pub fn file_read(&self, path: impl AsRef<RepoPath>) -> bool {
        let path = path.as_ref();
        tracing::trace!(%path, "file_read");
        self.mark_read(path, WalkType::File)
    }

    /// Observe a directory being loaded (i.e. "heavy" or remote read). `num_files` and
    /// `num_dirs` report the number of file and directory children of `path`,
    /// respectively. Returns whether an active walk changed (either created or removed).
    pub fn dir_loaded(
        &self,
        path: impl AsRef<RepoPath>,
        num_files: usize,
        num_dirs: usize,
    ) -> bool {
        let path = path.as_ref();

        tracing::trace!(%path, num_files, num_dirs, "dir_loaded");

        let mut inner = self.inner.lock();

        let time = inner.now();

        let mut walk_changed = inner.maybe_gc(time);

        // Fill in interesting metadata that informs detection of file content walks.
        if interesting_metadata(
            inner.min_dir_walk_threshold,
            Some(num_files),
            Some(num_dirs),
        ) {
            let node = inner.node.get_or_create_node(path);
            node.last_access = Some(time);
            node.total_dirs = Some(num_dirs);
            node.total_files = Some(num_files);
        }

        let (dir_path, base_name) = match path.split_last_component() {
            None => return walk_changed,
            Some((dir, base)) => (dir, base),
        };

        let dir_threshold = inner.min_dir_walk_threshold;

        let (owner, suffix) = inner
            .node
            .get_or_create_owning_node(WalkType::Directory, dir_path);

        owner.last_access = Some(time);

        if let Some(walk) = owner.get_dominating_walk(WalkType::Directory) {
            tracing::trace!(walk_root=%dir_path.strip_suffix(suffix, true).unwrap_or_default(), dir=%dir_path, "dir is already covered by an existing walk");
            walk.inc_dir_load(dir_path.strip_suffix(suffix, true).unwrap_or_default(), 1);
            return walk_changed;
        }

        let my_dir = owner;

        my_dir.seen_dirs.insert(base_name.to_owned());

        if my_dir.is_walked(WalkType::Directory, dir_threshold) {
            let dir_count = my_dir.seen_dirs.len();
            my_dir.seen_dirs.clear();
            inner.insert_walk(
                time,
                WalkType::Directory,
                Walk::for_type(WalkType::Directory, 0, dir_count as u64),
                dir_path,
            );

            walk_changed = true;
        }

        walk_changed
    }

    /// Observe a "soft" or cached directory access of `path`.
    /// This will not be tracked as a new walk, but will reset TTL of an existing walk.
    /// Returns whether an active walk changed (due to GC).
    pub fn dir_read(&self, path: impl AsRef<RepoPath>) -> bool {
        let path = path.as_ref();
        tracing::trace!(%path, "dir_read");
        self.mark_read(path, WalkType::Directory)
    }

    fn mark_read(&self, path: &RepoPath, wt: WalkType) -> bool {
        let Some(dir) = path.parent() else {
            return false;
        };

        let mut inner = self.inner.lock();

        let time = inner.now();

        // We need to run GC because we don't want to bump last_access on a node that should be collected.
        let walk_changed = inner.maybe_gc(time);

        // Bump last_access, but don't insert any new nodes/walks.
        if let Some((walk_node, suffix)) = inner.node.get_owning_node(wt, dir) {
            walk_node.last_access = Some(time);
            if let Some(walk) = walk_node.get_dominating_walk(wt) {
                let walk_root = dir.strip_suffix(suffix, true).unwrap_or_default();
                match wt {
                    WalkType::File => {
                        walk.inc_file_read(walk_root, 1);
                    }
                    WalkType::Directory => {
                        walk.inc_dir_read(walk_root, 1);
                    }
                }
            }
        }

        walk_changed
    }
}

fn interesting_metadata(
    threshold: usize,
    num_files: Option<usize>,
    num_dirs: Option<usize>,
) -> bool {
    num_dirs.is_some_and(|dirs| dirs > 0 && dirs < threshold)
        || num_files.is_some_and(|files| files > 0 && files < threshold)
}

impl Inner {
    /// Insert a new Walk rooted at `dir`.
    #[tracing::instrument(level = "debug", skip(self, time))]
    fn insert_walk(&mut self, time: Instant, walk_type: WalkType, walk: Walk, dir: &RepoPath) {
        // TODO: consider moving "should merge" logic into `WalkNode::insert_walk` to do
        // more work in a single traversal.

        let walk_node = self
            .node
            .insert_walk(walk_type, dir, walk, self.min_dir_walk_threshold);
        walk_node.last_access = Some(time);

        // Check if we should immediately promote this walk to parent directory. This is
        // similar to the ancestor advancement below, except that it can insert a new
        // walk.
        if self.maybe_merge_into_parent(time, walk_type, dir).is_some() {
            return;
        }

        // Check if we should merge with cousins (into grandparent). This is similar to
        // maybe_merge_into_grandparent in that it can insert a new walk.
        if self
            .maybe_merge_into_grandparent(time, walk_type, dir)
            .is_some()
        {
            return;
        }

        // Check if we have a containing walk whose depth boundary should be increased.
        if let Some((ancestor_dir, new_depth)) = self.should_advance_ancestor_walk(walk_type, dir) {
            self.insert_walk(time, walk_type, Walk::new(new_depth), ancestor_dir);
        }
    }

    /// If `dir` (and its siblings) imply a walk at `dir.parent()`, insert a walk at `dir.parent()`.
    fn maybe_merge_into_parent<'a>(
        &mut self,
        time: Instant,
        walk_type: WalkType,
        dir: &'a RepoPath,
    ) -> Option<()> {
        tracing::trace!(%dir, "maybe_merge_into_parent");

        let parent_dir = dir.parent()?;
        let parent_node = self.node.get_node(parent_dir)?;

        let walk_depth = should_merge_into_ancestor(
            self.min_dir_walk_threshold,
            walk_type,
            dir,
            parent_node,
            1,
        )?;

        self.insert_walk(time, walk_type, Walk::new(walk_depth), parent_dir);

        Some(())
    }

    /// If `dir` (and its cousins) imply a walk at `dir.parent().parent()`, insert a walk
    /// at `dir.parent().parent()`.
    fn maybe_merge_into_grandparent(
        &mut self,
        time: Instant,
        walk_type: WalkType,
        dir: &RepoPath,
    ) -> Option<()> {
        tracing::trace!(%dir, "maybe_merge_into_parent");

        let parent_dir = dir.parent()?;
        let grandparent_dir = parent_dir.parent()?;

        // Merging cousins willy nilly is too aggressive. We require that the cousins' parents are
        // already contained by a walk. This means we are only advancing a walk across one level,
        // not two.
        let (ancestor, suffix) = self.node.get_owning_node(walk_type, parent_dir)?;
        // If suffix is empty, the walk is for parent_dir itself. We want a higher walk.
        if suffix.is_empty() {
            return None;
        }

        let grandparent_node = ancestor.get_node(suffix.parent()?)?;
        let walk_depth = should_merge_into_ancestor(
            self.min_dir_walk_threshold,
            walk_type,
            dir,
            grandparent_node,
            2,
        )?;

        self.insert_walk(time, walk_type, Walk::new(walk_depth), grandparent_dir);

        Some(())
    }

    /// If a walk at `dir` suggests we can advance the depth of a containing walk, return
    /// (containing_dir, new_depth).
    fn should_advance_ancestor_walk<'a>(
        &mut self,
        walk_type: WalkType,
        dir: &'a RepoPath,
    ) -> Option<(&'a RepoPath, usize)> {
        let parent_dir = dir.parent()?;
        let (ancestor, suffix) = self.node.get_owning_node(walk_type, parent_dir)?;
        let ancestor_dir = parent_dir.strip_suffix(suffix, true)?;
        let (head, _) = suffix.split_first_component()?;

        // Check if the containing walk's node has N children with descendants that
        // have pushed to the next depth. The idea is we want some confidence before
        // expanding a huge walk deeper, so we wait until we've seen depth
        // advancements that bubble up to at least N different children of the walk
        // root.
        if ancestor.insert_advanced_child(walk_type, head.to_owned()) >= self.min_dir_walk_threshold
        {
            let depth = ancestor
                .get_dominating_walk(walk_type)
                .map_or(0, |w| w.depth)
                + 1;
            tracing::debug!(dir=%ancestor_dir, depth, "expanding walk boundary");
            return Some((ancestor_dir, depth));
        }

        None
    }

    fn now(&self) -> Instant {
        self.stub_now.unwrap_or_else(Instant::now)
    }

    /// Returns whether a walk was removed.
    fn maybe_gc(&mut self, time: Instant) -> bool {
        if time - self.last_gc_time < self.gc_interval {
            return false;
        }

        let start = self.now();

        let (deleted_nodes, remaining_nodes, deleted_walks) = self.node.gc(self.gc_timeout, time);

        let elapsed = start.elapsed();

        if deleted_nodes > 0 || deleted_walks > 0 || elapsed > Duration::from_millis(5) {
            tracing::debug!(elapsed=?start.elapsed(), deleted_nodes, remaining_nodes, deleted_walks, "GC complete");
        }

        self.last_gc_time = time;

        deleted_walks > 0
    }
}

/// Check if existing walks at nodes `ancestor_distance` below `ancestor` should be "merged" into a
/// new walk at `ancestor`. Returns depth of new walk to be inserted, if any.
fn should_merge_into_ancestor(
    min_dir_walk_threshold: usize,
    walk_type: WalkType,
    dir: &RepoPath,
    ancestor: &WalkNode,
    ancestor_distance: usize,
) -> Option<usize> {
    let mut kin_count = 0;
    let mut walk_depth = 0;
    ancestor.iter(|node, depth| -> bool {
        if depth == ancestor_distance {
            if let Some(walk) = node.get_walk_for_type(walk_type) {
                kin_count += 1;
                walk_depth = walk_depth.max(walk.depth);
            }
        }

        depth < ancestor_distance
    });

    if kin_count >= min_dir_walk_threshold {
        tracing::debug!(%dir, kin_count, ancestor_distance, "combining with collateral kin");
        Some(walk_depth + ancestor_distance)
    } else if ancestor
        .total_dirs
        .is_some_and(|total| total < min_dir_walk_threshold)
    {
        tracing::debug!(%dir, ancestor_distance, "promoting collateral kin due to few dirs");
        Some(walk_depth + ancestor_distance)
    } else {
        None
    }
}
