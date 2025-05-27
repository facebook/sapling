/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#[cfg(test)]
mod tests;
mod walk_node;

use std::sync::LazyLock;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicI64;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::time::Duration;
use std::time::Instant;

use parking_lot::RwLock;
use types::RepoPath;
use types::RepoPathBuf;
use walk_node::WalkNode;

// Goals:
//  - Aggressively detect walk and aggressively cancel walk.
//  - Passive - don't fetch or query any stores.
//  - Minimize memory usage.

pub struct Detector {
    inner: RwLock<Inner>,
}

struct Inner {
    // "How many children must be accessed before we consider parent walked?"
    // This is the main threshold to tune detector aggro.
    walk_threshold: usize,
    // Walks with depth < lax_depth will use a stricter walk_threshold.
    lax_depth: usize,
    // Walk threshold multiplier for walks shallower than lax_depth.
    strict_multiplier: usize,
    // A minimum walk threshold as ratio of total directory size. This only applies when we know the
    // size of the directory. This slows down walk detection in humongous directories.
    walk_ratio: f64,
    // Last time we ran GC.
    last_gc_time: Instant,
    // How often we want to run GC.
    gc_interval: Duration,
    // How long after a node was last accessed until we GC it.
    gc_timeout: Duration,
    // Root node used to track walks.
    node: WalkNode,
    // Stub "now" value used by tests.
    stub_now: Option<Instant>,
}

impl Default for Inner {
    fn default() -> Self {
        Self {
            walk_threshold: DEFAULT_WALK_THRESHOLD,
            lax_depth: DEFAULT_LAX_DEPTH,
            strict_multiplier: DEFAULT_STRICT_MULTIPLIER,
            walk_ratio: DEFAULT_WALK_RATIO,
            last_gc_time: Instant::now(),
            gc_interval: DEFAULT_GC_INTERVAL,
            gc_timeout: DEFAULT_GC_TIMEOUT,
            node: WalkNode::default(),
            stub_now: None,
        }
    }
}

// How many children must be accessed in a directory to consider the directory "walked".
const DEFAULT_WALK_THRESHOLD: usize = 3;

// Walk threshold multiplier for shallow walks.
const DEFAULT_STRICT_MULTIPLIER: usize = 10;

// Depth at which we no longer get the strict multiplier. 0 means we never get the multiplier.
const DEFAULT_LAX_DEPTH: usize = 0;

// If we know the total dir size, make sure walk threshold is at least 5% of dir size.
const DEFAULT_WALK_RATIO: f64 = 0.05;

// How often we garbage collect stale walks.
const DEFAULT_GC_INTERVAL: Duration = Duration::from_secs(1);

// How stale a walk must be before we remove it.
const DEFAULT_GC_TIMEOUT: Duration = Duration::from_secs(2);

impl Detector {
    pub fn new() -> Self {
        Self {
            inner: Default::default(),
        }
    }

    pub fn set_walk_threshold(&self, threshold: usize) {
        self.inner.write().walk_threshold = threshold;
    }

    pub fn set_lax_depth(&self, depth: usize) {
        self.inner.write().lax_depth = depth;
    }

    pub fn set_strict_multiplier(&self, multiplier: usize) {
        self.inner.write().strict_multiplier = multiplier;
    }

    pub fn set_walk_ratio(&self, threshold: f64) {
        if threshold <= 0.0 {
            return;
        }
        self.inner.write().walk_ratio = threshold;
    }

    #[cfg(test)]
    pub fn set_now(&self, now: Instant) {
        self.inner.write().stub_now = Some(now);
    }

    pub fn set_gc_interval(&self, interval: Duration) {
        self.inner.write().gc_interval = interval;
    }

    pub fn set_gc_timeout(&self, timeout: Duration) {
        self.inner.write().gc_timeout = timeout;
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
        let mut inner = self.inner.write();

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

        // Try lightweight read-only path.
        if let Some(walk_root) = self.mark_read(path, WalkType::File, true) {
            tracing::trace!(%walk_root, file=%path, "file already in walk (fastpath)");
            return false;
        }

        let (dir_path, base_name) = match path.split_last_component() {
            // Shouldn't happen - implies a path of "" which is not valid for a file.
            None => return false,
            Some((dir, base)) => (dir, base),
        };

        let mut inner = self.inner.write();

        let time = inner.now();

        let mut walk_changed = inner.maybe_gc(time);

        let walk_threshold = inner.walk_threshold(dir_path.components().count());
        let walk_ratio = inner.walk_ratio;

        let (owner, suffix) = inner
            .node
            .get_or_create_owning_node(WalkType::File, dir_path);

        owner.last_access.store(time);

        if let Some(walk) = owner.get_dominating_walk(WalkType::File) {
            tracing::trace!(walk_root=%path.strip_suffix(suffix, true).unwrap_or_default(), file=%path, "file already in walk");
            walk.inc_file_load(dir_path.strip_suffix(suffix, true).unwrap_or_default(), 1);
            return walk_changed;
        }

        let my_dir = owner;

        my_dir.seen_files.insert(base_name.to_owned());

        let seen_count = my_dir.seen_files.len();
        if my_dir.is_walked(WalkType::File, seen_count, walk_threshold, walk_ratio) {
            my_dir.seen_files.clear();
            inner.insert_walk(
                time,
                WalkType::File,
                Walk::for_type(WalkType::File, 0, seen_count as u64),
                dir_path,
            );
            walk_changed = true;
        }

        walk_changed
    }

    /// Observe a "soft" or cached file (content) access of `path`.
    /// This will not be tracked as a new walk, but will reset TTL of an existing walk.
    /// Returns whether path was covered by an active walk.
    pub fn file_read(&self, path: impl AsRef<RepoPath>) -> bool {
        let path = path.as_ref();
        tracing::trace!(%path, "file_read");
        self.mark_read(path, WalkType::File, false).is_some()
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

        // Try lightweight read-only path.
        if let Some(walk_root) = self.mark_read(path, WalkType::Directory, true) {
            tracing::trace!(%walk_root, dir=%path, "dir already in walk (fastpath)");
            return false;
        }

        let mut inner = self.inner.write();

        let time = inner.now();

        let mut walk_changed = inner.maybe_gc(time);

        // Fill in interesting metadata that informs detection of file content walks.
        if interesting_metadata(
            inner.walk_threshold,
            inner.walk_ratio,
            Some(num_files),
            Some(num_dirs),
        ) {
            let node = inner.node.get_or_create_node(path);
            node.last_access.store(time);
            node.total_dirs = Some(num_dirs);
            node.total_files = Some(num_files);
        }

        let (dir_path, base_name) = match path.split_last_component() {
            None => return walk_changed,
            Some((dir, base)) => (dir, base),
        };

        let walk_threshold = inner.walk_threshold(dir_path.components().count());
        let walk_ratio = inner.walk_ratio;

        let (owner, suffix) = inner
            .node
            .get_or_create_owning_node(WalkType::Directory, dir_path);

        owner.last_access.store(time);

        if let Some(walk) = owner.get_dominating_walk(WalkType::Directory) {
            tracing::trace!(walk_root=%dir_path.strip_suffix(suffix, true).unwrap_or_default(), dir=%path, "dir already in walk");
            walk.inc_dir_load(dir_path.strip_suffix(suffix, true).unwrap_or_default(), 1);
            return walk_changed;
        }

        let my_dir = owner;

        my_dir.seen_dirs.insert(base_name.to_owned());

        let seen_count = my_dir.seen_dirs.len();
        if my_dir.is_walked(WalkType::Directory, seen_count, walk_threshold, walk_ratio) {
            my_dir.seen_dirs.clear();
            inner.insert_walk(
                time,
                WalkType::Directory,
                Walk::for_type(WalkType::Directory, 0, seen_count as u64),
                dir_path,
            );

            walk_changed = true;
        }

        walk_changed
    }

    /// Observe a "soft" or cached directory access of `path`.
    /// This will not be tracked as a new walk, but will reset TTL of an existing walk.
    /// Returns whether path was covered by an active walk.
    pub fn dir_read(&self, path: impl AsRef<RepoPath>) -> bool {
        let path = path.as_ref();
        tracing::trace!(%path, "dir_read");
        self.mark_read(path, WalkType::Directory, false).is_some()
    }

    /// Record that a certain number of files have been preloaded. This has no functional purpose -
    /// it is purely to measure the efficiency of this library when paired with a prefetch
    /// mechanism.
    pub fn files_preloaded(&self, walk_root: impl AsRef<RepoPath>, preload_count: u64) {
        let walk_root = walk_root.as_ref();
        tracing::trace!(%walk_root, preload_count, "files_preloaded");

        let inner = self.inner.read();

        if let Some(node) = inner.node.get_node(walk_root) {
            if let Some(walk) = node.get_walk_for_type(WalkType::File) {
                walk.file_preloads
                    .fetch_add(preload_count, Ordering::Relaxed);
            }
        }
    }

    /// "touch" any walk of type wt that covers `path` to update metrics and keep the walk alive.
    /// Returns the root of walk, if any.
    fn mark_read<'a>(
        &'a self,
        path: &'a RepoPath,
        wt: WalkType,
        heavy_read: bool,
    ) -> Option<&'a RepoPath> {
        let dir = path.parent()?;

        let inner = self.inner.read();

        let time = inner.now();

        // Bump last_access, but don't insert any new nodes/walks.
        if let Some((walk_node, suffix)) = inner.node.get_owning_node(wt, dir) {
            if walk_node.expired(time, inner.gc_timeout) {
                // Don't resurrect a node that should be expired by GC.
                return None;
            }

            walk_node.last_access.store(time);

            if let Some(walk) = walk_node.get_dominating_walk(wt) {
                let walk_root = dir.strip_suffix(suffix, true).unwrap_or_default();
                match (wt, heavy_read) {
                    (WalkType::File, false) => walk.inc_file_read(walk_root, 1),
                    (WalkType::File, true) => walk.inc_file_load(walk_root, 1),
                    (WalkType::Directory, false) => walk.inc_dir_read(walk_root, 1),
                    (WalkType::Directory, true) => walk.inc_dir_load(walk_root, 1),
                };
            }

            return Some(dir.strip_suffix(suffix, true).unwrap_or_default());
        }

        None
    }
}

fn interesting_metadata(
    threshold: usize,
    walk_ratio: f64,
    num_files: Option<usize>,
    num_dirs: Option<usize>,
) -> bool {
    // "interesting" means the directory size metadata might influence our walk detection decisions.
    // Basically, we care about very small or very large directories.

    // Work backwards from walk threshold and walk ratio to calculate what size of directory would
    // start to increase our walk threshold (due to the walk ratio).
    let big_dir_threshold: usize = (threshold as f64 / walk_ratio) as usize;

    num_dirs.is_some_and(|dirs| dirs < threshold || dirs >= big_dir_threshold)
        || num_files.is_some_and(|files| files < threshold || files > big_dir_threshold)
}

impl Inner {
    /// Insert a new Walk rooted at `dir`.
    #[tracing::instrument(level = "debug", skip(self, time))]
    fn insert_walk(&mut self, time: Instant, walk_type: WalkType, walk: Walk, dir: &RepoPath) {
        // TODO: consider moving "should merge" logic into `WalkNode::insert_walk` to do
        // more work in a single traversal.

        let walk_node = self.node.insert_walk(
            walk_type,
            dir,
            walk,
            self.walk_threshold(dir.components().count()),
            self.walk_ratio,
        );
        walk_node.last_access.store(time);

        // Check if we should immediately promote this walk to parent directory. This is
        // similar to the ancestor advancement below, except that it can insert a new
        // walk.
        if self.maybe_merge_into_parent(time, walk_type, dir).is_some() {
            return;
        }

        // Check if we should merge with cousins (into grandparent). This is similar to
        // maybe_merge_into_parent in that it can insert a new walk.
        if self
            .maybe_merge_into_grandparent(time, walk_type, dir)
            .is_some()
        {
            return;
        }

        // Check if we have a containing walk whose depth boundary should be increased.
        if let Some((ancestor_dir, new_depth)) =
            self.should_advance_ancestor_walk(time, walk_type, dir)
        {
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

        // Touch the ancestor's last access time. We don't want the ancestor's walk to GC while we
        // are still "making progress" towards advancing its walk.
        parent_node.last_access.store(time);

        let walk_depth = should_merge_into_ancestor(
            self.walk_threshold(parent_dir.components().count()),
            self.walk_ratio,
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
        tracing::trace!(%dir, "maybe_merge_into_grandparent");

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

        // Touch the ancestor's last access time. We don't want the ancestor's walk to GC while we
        // are still "making progress" towards advancing its walk.
        ancestor.last_access.store(time);

        let grandparent_node = ancestor.get_node(suffix.parent()?)?;
        let walk_depth = should_merge_into_ancestor(
            self.walk_threshold(grandparent_dir.components().count()),
            self.walk_ratio,
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
        time: Instant,
        walk_type: WalkType,
        dir: &'a RepoPath,
    ) -> Option<(&'a RepoPath, usize)> {
        let parent_dir = dir.parent()?;
        let (ancestor, suffix) = self.node.get_owning_node_mut(walk_type, parent_dir)?;
        let ancestor_dir = parent_dir.strip_suffix(suffix, true)?;

        let head = if suffix.is_empty() {
            // If we have no suffix, then `ancestor_dir == parent_dir`. Tha name of the advanced
            // child is the last part of `dir`.
            dir.split_last_component()?.1
        } else {
            // If we have a suffix, that means `ancestor_dir != parent_dir`. The name of the
            // advanced child is the first part of the suffix.
            suffix.split_first_component()?.0
        };

        // Touch the ancestor's last access time. We don't want the ancestor's walk to GC while we
        // are still "making progress" towards advancing its walk.
        ancestor.last_access.store(time);

        let walk_threshold = walk_threshold(
            self.walk_threshold,
            self.lax_depth,
            self.strict_multiplier,
            ancestor_dir.components().count(),
        );

        // Check if the containing walk's node has N children with descendants that
        // have pushed to the next depth. The idea is we want some confidence before
        // expanding a huge walk deeper, so we wait until we've seen depth
        // advancements that bubble up to at least N different children of the walk
        // root.
        let advanced_count = ancestor.insert_advanced_child(walk_type, head.to_owned());
        if ancestor.is_walked(
            WalkType::Directory,
            advanced_count,
            walk_threshold,
            self.walk_ratio,
        ) {
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

    fn walk_threshold(&self, walk_root_depth: usize) -> usize {
        walk_threshold(
            self.walk_threshold,
            self.lax_depth,
            self.strict_multiplier,
            walk_root_depth,
        )
    }
}

fn walk_threshold(
    base_threshold: usize,
    lax_depth: usize,
    strict_multiplier: usize,
    walk_root_depth: usize,
) -> usize {
    // Add a threshold multiplier for walk roots that are shallower than `lax_depth`. This
    // makes it harder for walks to percolate "too high".
    if walk_root_depth < lax_depth {
        (strict_multiplier * base_threshold).max(base_threshold)
    } else {
        base_threshold
    }
}

/// Check if existing walks at nodes `ancestor_distance` below `ancestor` should be "merged" into a
/// new walk at `ancestor`. Returns depth of new walk to be inserted, if any.
fn should_merge_into_ancestor(
    walk_threshold: usize,
    walk_ratio: f64,
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

    if ancestor.is_walked(
        WalkType::Directory, // We are combining directories here, so always use directory count.
        kin_count,
        walk_threshold,
        walk_ratio,
    ) {
        tracing::debug!(%dir, kin_count, ancestor_distance, walk_threshold, walk_ratio, ?ancestor.total_dirs, "combining with collateral kin");
        Some(walk_depth + ancestor_distance)
    } else {
        None
    }
}

#[derive(Default)]
pub struct Walk {
    // Depth of this walk (relative to the root directory of the walk).
    depth: usize,

    // How many heavy/remote file fetches we've observed under this walk.
    file_loads: AtomicU64,
    // How many light/cached file fetches we've observed under this walk.
    file_reads: AtomicU64,
    // How many remote file prefetches we've been informed about for this walk.
    file_preloads: AtomicU64,
    // How many heavy/remote dir fetches we've observed under this walk.
    dir_loads: AtomicU64,
    // How many light/cached dir fetches we've observed under this walk.
    dir_reads: AtomicU64,

    // Whether we have already logged the start of this walk.
    logged_start: AtomicBool,
}

impl std::fmt::Debug for Walk {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Don't include all the counters - they are very noisy in traces.
        f.debug_struct("Walk").field("depth", &self.depth).finish()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WalkType {
    File,
    Directory,
}

impl Walk {
    /// Arbitrary threshold for logging a "big" walk.
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
        self.file_preloads.fetch_add(
            other.file_preloads.load(Ordering::Relaxed),
            Ordering::AcqRel,
        );
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
            file_preloads = self.file_preloads.load(Ordering::Relaxed),
            dir_loads = self.dir_loads.load(Ordering::Relaxed),
            dir_reads = self.dir_reads.load(Ordering::Relaxed),
            "big walk started",
        );
    }

    fn log_end(&self, root: &RepoPath) {
        if self.file_loads.load(Ordering::Relaxed) >= Self::BIG_WALK_THRESHOLD
            || self.file_reads.load(Ordering::Relaxed) >= Self::BIG_WALK_THRESHOLD
            || self.file_preloads.load(Ordering::Relaxed) >= Self::BIG_WALK_THRESHOLD
            || self.dir_loads.load(Ordering::Relaxed) >= Self::BIG_WALK_THRESHOLD
            || self.dir_reads.load(Ordering::Relaxed) >= Self::BIG_WALK_THRESHOLD
        {
            tracing::info!(
                %root,
                file_loads = self.file_loads.load(Ordering::Relaxed),
                file_reads = self.file_reads.load(Ordering::Relaxed),
                file_preloads = self.file_preloads.load(Ordering::Relaxed),
                dir_loads = self.dir_loads.load(Ordering::Relaxed),
                dir_reads = self.dir_reads.load(Ordering::Relaxed),
                "big walk ended",
            );

            tracing::debug!(
                target: "big_walk",
                walk_root = %root,
                file_loads = self.file_loads.load(Ordering::Relaxed),
                file_reads = self.file_reads.load(Ordering::Relaxed),
                file_preloads = self.file_preloads.load(Ordering::Relaxed),
                dir_loads = self.dir_loads.load(Ordering::Relaxed),
                dir_reads = self.dir_reads.load(Ordering::Relaxed),
            );
        }
    }

    #[cfg(test)]
    fn counters(&self) -> (u64, u64, u64, u64, u64) {
        (
            self.file_loads.load(Ordering::Relaxed),
            self.file_reads.load(Ordering::Relaxed),
            self.file_preloads.load(Ordering::Relaxed),
            self.dir_loads.load(Ordering::Relaxed),
            self.dir_reads.load(Ordering::Relaxed),
        )
    }
}

/// Base epoch value for AtomicInstant. Instant::now isn't const, so we use LazyLock. We could use
/// SystemTime::UNIX_EPOCH, but then we lose monotonicity.
static EPOCH: LazyLock<Instant> = LazyLock::new(Instant::now);

// Atomic instant with millisecond precision relative to EPOCH.
// Negative value means "not set".
struct AtomicInstant(AtomicI64);

impl Default for AtomicInstant {
    fn default() -> Self {
        Self::new()
    }
}

impl AtomicInstant {
    /// Initialize an empty AtomicInstant. `load()` will return `None` until `store()` is called.
    fn new() -> Self {
        // Eagerly initialize EPOCH to reduce the chance an Instant::now() is smaller than EPOCH.
        let _ = *EPOCH;

        Self(AtomicI64::new(i64::MIN))
    }

    fn store(&self, value: Instant) {
        self.0.store(
            // It is theoretically possible for `value`` to be smaller than EPOCH. Do a saturating
            // subtraction to 0, just in case. `duration_since` says it may panic in this case
            // in the future.
            value
                .checked_duration_since(*EPOCH)
                .unwrap_or_default()
                .as_millis() as i64,
            Ordering::Relaxed,
        );
    }

    fn load(&self) -> Option<Instant> {
        match self.0.load(Ordering::Relaxed) {
            v if v < 0 => None,
            v => Some(*EPOCH + Duration::from_millis(v as u64)),
        }
    }

    /// Reset to "not set" state. `load()` will return `None` until `store()` is called.
    fn reset(&self) {
        self.0.store(i64::MIN, Ordering::Relaxed);
    }
}
