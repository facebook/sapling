/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#[cfg(test)]
mod tests;
mod walk_node;

use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::LazyLock;
use std::sync::OnceLock;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicI64;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::time::Duration;

#[cfg(test)]
use coarsetime as _; // silence "unused dependency" warning
#[cfg(not(test))]
use coarsetime::Instant;
#[cfg(test)]
use mock_instant::Instant;
use parking_lot::RwLock;
use rand::Rng;
use tracing::Level;
use types::RepoPath;
use types::RepoPathBuf;
use walk_node::WalkNode;

// Goals:
//  - Aggressively detect walk and aggressively cancel walk.
//  - Passive - don't fetch or query any stores.
//  - Minimize memory usage.

#[derive(Clone, Default)]
pub struct Detector {
    config: Config,
    root: Option<PathBuf>,
    inner: Arc<RwLock<Inner>>,
}

struct Inner {
    // Last time we ran GC.
    last_gc_time: Instant,
    // Root node used to track walks.
    node: WalkNode,
}

impl Default for Inner {
    fn default() -> Self {
        Self {
            last_gc_time: Instant::now(),
            node: WalkNode::new(DEFAULT_GC_TIMEOUT),
        }
    }
}

impl Detector {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn reset_config(&mut self) {
        self.config = Config::default();
    }

    pub fn set_walk_threshold(&mut self, threshold: usize) {
        self.config.walk_threshold = threshold;
    }

    pub fn set_lax_depth(&mut self, depth: usize) {
        self.config.lax_depth = depth;
    }

    pub fn set_strict_multiplier(&mut self, multiplier: usize) {
        self.config.strict_multiplier = multiplier;
    }

    pub fn set_walk_ratio(&mut self, threshold: f64) {
        if threshold <= 0.0 {
            return;
        }
        self.config.walk_ratio = threshold;
    }

    pub fn set_gc_interval(&mut self, interval: Duration) {
        self.config.gc_interval = interval;
    }

    pub fn set_gc_timeout(&mut self, timeout: Duration) {
        self.config.gc_timeout = timeout;
        // Update root node's timeout as a special case. The root node is never deleted, so has no
        // chance to get current gc_timeout on creation.
        self.inner.write().node.gc_timeout = timeout;
    }

    /// Set root path. Used in some metric collection.
    pub fn set_root(&mut self, root: Option<PathBuf>) {
        self.root = root;
    }

    /// Return list of (walk root dir, walk depth) representing active file content walks.
    pub fn file_walks(&self) -> Vec<(RepoPathBuf, usize)> {
        self.walks(Some(WalkType::File))
            .into_iter()
            .map(|(root, depth, _)| (root, depth))
            .collect()
    }

    /// Return list of (walk root dir, walk depth) representing active directory walks.
    pub fn dir_walks(&self) -> Vec<(RepoPathBuf, usize)> {
        self.walks(Some(WalkType::Directory))
            .into_iter()
            .map(|(root, depth, _)| (root, depth))
            .collect()
    }

    /// Return list of (walk root dir, walk depth, walk type) representing all active walks.
    pub fn all_walks(&self) -> Vec<(RepoPathBuf, usize, WalkType)> {
        self.walks(None)
    }

    fn walks(&self, walk_type: Option<WalkType>) -> Vec<(RepoPathBuf, usize, WalkType)> {
        let inner = self.inner.read();

        let mut walks = if inner.needs_gc(&self.config) {
            // Only grab write lock if we need to GC.
            drop(inner);
            self.inner.write().maybe_gc(&self.config);
            self.inner.read().node.list_walks(walk_type)
        } else {
            inner.node.list_walks(walk_type)
        };

        walks.sort_by(|a, b| a.0.as_str().cmp(b.0.as_str()));

        walks
    }

    /// Observe a "heavy" or remote file (content) read of `path`.
    /// Returns whether an active walk changed (either created or removed).
    pub fn file_loaded(&self, path: impl AsRef<RepoPath>, pid: u32) -> bool {
        let path = path.as_ref();

        tracing::trace!(%path, "file_loaded");

        // Try lightweight read-only path.
        if self.mark_read(path, WalkType::File, true, pid).is_some() {
            return false;
        }

        let (dir_path, base_name) = match path.split_last_component() {
            // Shouldn't happen - implies a path of "" which is not valid for a file.
            None => return false,
            Some((dir, base)) => (dir, base),
        };

        let mut inner = self.inner.write();

        let mut walk_changed = inner.maybe_gc(&self.config);

        let walk_threshold = walk_threshold(&self.config, dir_path.depth());
        let walk_ratio = self.config.walk_ratio;

        let (owner, suffix) =
            inner
                .node
                .get_or_create_owning_node(&self.config, WalkType::File, dir_path);

        owner.last_access.bump();

        if let Some(walk) = owner.get_dominating_walk(WalkType::File) {
            tracing::trace!(walk_root=%path.strip_suffix(suffix, true).unwrap_or_default(), file=%path, "file already in walk");
            walk.inc_file_load(
                dir_path.strip_suffix(suffix, true).unwrap_or_default(),
                self.root.as_deref(),
                path,
            );
            return walk_changed;
        }

        let my_dir = owner;

        my_dir.seen_files.insert(base_name.to_owned());

        let seen_count = my_dir.seen_files.len();
        if my_dir.is_walked(WalkType::File, seen_count, 0, walk_threshold, walk_ratio) {
            my_dir.seen_files.clear();
            inner.insert_walk(
                &self.config,
                WalkType::File,
                Walk::for_type(WalkType::File, 0, seen_count as u64, pid),
                dir_path,
            );
            walk_changed = true;
        }

        walk_changed
    }

    /// Observe a "soft" or cached file (content) access of `path`.
    /// This will not be tracked as a new walk, but will reset TTL of an existing walk.
    /// Returns whether path was covered by an active walk.
    pub fn file_read(&self, path: impl AsRef<RepoPath>, pid: u32) -> bool {
        let path = path.as_ref();
        tracing::trace!(%path, "file_read");
        self.mark_read(path, WalkType::File, false, pid).is_some()
    }

    /// Observe a directory being loaded (i.e. "heavy" or remote read). `num_files` and `num_dirs`
    /// report the number of file and directory children of `path`, respectively. Pass zero if you
    /// don't know. Returns whether an active walk changed (either created or removed).
    pub fn dir_loaded(
        &self,
        path: impl AsRef<RepoPath>,
        num_files: usize,
        num_dirs: usize,
        pid: u32,
    ) -> bool {
        let path = path.as_ref();

        tracing::trace!(%path, num_files, num_dirs, "dir_loaded");

        let is_interesting_metadata = interesting_metadata(
            self.config.walk_threshold,
            self.config.walk_ratio,
            Some(num_files),
            Some(num_dirs),
        );

        // Try lightweight read-only path.
        if self
            .mark_read(path, WalkType::Directory, true, pid)
            .is_some()
        {
            if is_interesting_metadata {
                // Fill in interesting metadata that informs detection of file content walks.
                let mut inner = self.inner.write();
                inner.set_metadata(&self.config, path, num_files, num_dirs);
            }
            return false;
        }

        let mut inner = self.inner.write();

        let mut walk_changed = inner.maybe_gc(&self.config);

        if is_interesting_metadata {
            // Fill in interesting metadata that informs detection of file content walks.
            inner.set_metadata(&self.config, path, num_files, num_dirs);
        }

        let (dir_path, base_name) = match path.split_last_component() {
            None => return walk_changed,
            Some((dir, base)) => (dir, base),
        };

        let walk_threshold = walk_threshold(&self.config, dir_path.depth());
        let walk_ratio = self.config.walk_ratio;

        let (owner, suffix) =
            inner
                .node
                .get_or_create_owning_node(&self.config, WalkType::Directory, dir_path);

        owner.last_access.bump();

        if let Some(walk) = owner.get_dominating_walk(WalkType::Directory) {
            tracing::trace!(walk_root=%dir_path.strip_suffix(suffix, true).unwrap_or_default(), dir=%path, "dir already in walk");
            walk.inc_dir_load(
                dir_path.strip_suffix(suffix, true).unwrap_or_default(),
                self.root.as_deref(),
                path,
            );
            return walk_changed;
        }

        let my_dir = owner;

        my_dir.seen_dirs.insert(base_name.to_owned());

        let seen_count = my_dir.seen_dirs.len();
        if my_dir.is_walked(
            WalkType::Directory,
            seen_count,
            0,
            walk_threshold,
            walk_ratio,
        ) {
            my_dir.seen_dirs.clear();
            inner.insert_walk(
                &self.config,
                WalkType::Directory,
                Walk::for_type(WalkType::Directory, 0, seen_count as u64, pid),
                dir_path,
            );

            walk_changed = true;
        }

        walk_changed
    }

    /// Observe a "soft" or cached directory access of `path`. This will not be tracked as a new
    /// walk, but will reset TTL of an existing walk. `num_files` and `num_dirs` report the number
    /// of file and directory children of `path`, respectively. Pass zero if you don't know. Returns
    /// whether path was covered by an active walk.
    pub fn dir_read(
        &self,
        path: impl AsRef<RepoPath>,
        num_files: usize,
        num_dirs: usize,
        pid: u32,
    ) -> bool {
        let path = path.as_ref();
        tracing::trace!(%path, num_files, num_dirs, "dir_read");

        // Remember interesting directory metadata, even though directory is being loaded from cache.
        // It could still be involved in ongoing or future walk activity that involves remote fetches.
        if important_metadata(
            self.config.walk_threshold,
            self.config.walk_ratio,
            Some(num_files),
            Some(num_dirs),
        ) {
            self.inner
                .write()
                .set_metadata(&self.config, path, num_files, num_dirs);
        }

        self.mark_read(path, WalkType::Directory, false, pid)
            .is_some()
    }

    /// Record that a certain number of files have been preloaded. This has no functional purpose -
    /// it is purely to measure the efficiency of this library when paired with a prefetch
    /// mechanism. Returns total number of files preloaded and read (i.e. cache hits) so far.
    pub fn files_preloaded(
        &self,
        walk_root: impl AsRef<RepoPath>,
        preload_count: u64,
    ) -> (u64, u64) {
        let walk_root = walk_root.as_ref();
        tracing::trace!(%walk_root, preload_count, "files_preloaded");

        let inner = self.inner.read();

        if let Some(node) = inner.node.get_node(walk_root) {
            if let Some(walk) = node.get_walk_for_type(WalkType::File) {
                let prev_preload = walk
                    .file_preloads
                    .fetch_add(preload_count, Ordering::Relaxed);
                return (
                    prev_preload + preload_count,
                    walk.file_reads.load(Ordering::Relaxed),
                );
            }
        }

        (0, 0)
    }

    /// "touch" any walk of type wt that covers `path` to update metrics and keep the walk alive.
    /// Returns the root of walk, if any.
    fn mark_read<'a>(
        &'a self,
        path: &'a RepoPath,
        wt: WalkType,
        heavy_read: bool,
        pid: u32,
    ) -> Option<&'a RepoPath> {
        let dir = path.parent()?;

        let inner = self.inner.read();

        // Bump last_access, but don't insert any new nodes/walks.
        if let Some((walk_node, suffix)) = inner.node.get_owning_node(wt, dir) {
            walk_node.last_access.bump();

            if let Some(walk) = walk_node.get_dominating_walk(wt) {
                let walk_root = dir.strip_suffix(suffix, true).unwrap_or_default();
                match (wt, heavy_read) {
                    (WalkType::File, false) => {
                        walk.inc_file_read(walk_root, self.root.as_deref(), path)
                    }
                    (WalkType::File, true) => {
                        walk.inc_file_load(walk_root, self.root.as_deref(), path)
                    }
                    (WalkType::Directory, false) => {
                        walk.inc_dir_read(walk_root, self.root.as_deref(), path)
                    }
                    (WalkType::Directory, true) => {
                        walk.inc_dir_load(walk_root, self.root.as_deref(), path)
                    }
                };

                walk.maybe_swap_pid(pid, 1);
            }

            let walk_root = dir.strip_suffix(suffix, true).unwrap_or_default();

            tracing::trace!(%walk_root, ?wt, %path, "already in walk (fastpath)");

            return Some(walk_root);
        }

        None
    }
}

// Whether directory metadata is interesting and we should retain it until normal expiration.
fn interesting_metadata(
    threshold: usize,
    walk_ratio: f64,
    num_files: Option<usize>,
    num_dirs: Option<usize>,
) -> bool {
    // "interesting" means the directory size metadata might influence our walk detection decisions.
    // Basically, we care about very small or very large directories.

    if important_metadata(threshold, walk_ratio, num_files, num_dirs) {
        return true;
    }

    // Work backwards from walk threshold and walk ratio to calculate what size of directory would
    // start to increase our walk threshold (due to the walk ratio).
    let big_dir_threshold: usize = ((threshold + 1) as f64 / walk_ratio) as usize;

    // We don't care about empty directories because we will never see "activity" for an empty
    // directory, so probably won't ever make use of the size hint. Marking empty directories as
    // "not interesting" significantly reduces the number of nodes we create during big walks.
    num_dirs.is_some_and(|dirs| dirs > 0 && dirs < threshold || dirs >= big_dir_threshold)
        || num_files
            .is_some_and(|files| files > 0 && files < threshold || files >= big_dir_threshold)
}

// Whether directory metadata is important and we should retain indefinitely.
fn important_metadata(
    threshold: usize,
    walk_ratio: f64,
    num_files: Option<usize>,
    num_dirs: Option<usize>,
) -> bool {
    // "important" means the directory size metadata should be retained indefinitely.
    // Basically, we really care about very large directories.

    // Work backwards from walk threshold and walk ratio to calculate what size of directory would
    // start to increase our walk threshold (due to the walk ratio).
    let big_dir_threshold: usize = ((threshold + 1) as f64 / walk_ratio) as usize;

    // We mainly care about directories with lots of children directories since they can very
    // quickly cause exponential growth. We also care about directories with a lot of files, but
    // less so since it is a one-time cost vs. an indication of potential extreme growth in the
    // future. For example, a directory with 100 sub-directories is something we should slow down
    // for, but a directory with 100 files is not that big of a deal. So, increase threshold by 10x
    // for the file check. This greatly reduces the number of WalkNodes we keep indefinitely after
    // large walks.
    num_dirs.is_some_and(|dirs| dirs >= big_dir_threshold)
        || num_files.is_some_and(|files| files >= 10 * big_dir_threshold)
}

impl Inner {
    /// Insert a new Walk rooted at `dir`.
    #[tracing::instrument(level = "debug", skip(self, config))]
    fn insert_walk(&mut self, config: &Config, walk_type: WalkType, walk: Walk, dir: &RepoPath) {
        // TODO: consider moving "should merge" logic into `WalkNode::insert_walk` to do
        // more work in a single traversal.

        let walk_node = self
            .node
            .insert_walk(config, walk_type, dir, walk, dir.depth());
        walk_node.last_access.bump();

        // Check if we should immediately promote this walk to parent directory. This is
        // similar to the ancestor advancement below, except that it can insert a new
        // walk.
        if self
            .maybe_merge_into_parent(config, walk_type, dir)
            .is_some()
        {
            return;
        }

        // Check if we should merge with cousins (into grandparent). This is similar to
        // maybe_merge_into_parent in that it can insert a new walk.
        if self
            .maybe_merge_into_grandparent(config, walk_type, dir)
            .is_some()
        {
            return;
        }

        // Check if we have a containing walk whose depth boundary should be increased.
        if let Some((ancestor_dir, new_depth)) =
            self.should_advance_ancestor_walk(config, walk_type, dir)
        {
            self.insert_walk(config, walk_type, Walk::new(new_depth), ancestor_dir);
        }
    }

    /// If `dir` (and its siblings) imply a walk at `dir.parent()`, insert a walk at `dir.parent()`.
    fn maybe_merge_into_parent<'a>(
        &mut self,
        config: &Config,
        walk_type: WalkType,
        dir: &'a RepoPath,
    ) -> Option<()> {
        tracing::trace!(%dir, "maybe_merge_into_parent");

        let parent_dir = dir.parent()?;
        let parent_node = self.node.get_node(parent_dir)?;

        let walk_depth = should_merge_into_ancestor(
            walk_threshold(config, parent_dir.depth()),
            config.walk_ratio,
            walk_type,
            dir,
            parent_node,
            1,
        )?;

        self.insert_walk(config, walk_type, Walk::new(walk_depth), parent_dir);

        Some(())
    }

    /// If `dir` (and its cousins) imply a walk at `dir.parent().parent()`, insert a walk
    /// at `dir.parent().parent()`.
    fn maybe_merge_into_grandparent(
        &mut self,
        config: &Config,
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

        let grandparent_node = ancestor.get_node(suffix.parent()?)?;

        let walk_depth = should_merge_into_ancestor(
            walk_threshold(config, grandparent_dir.depth()),
            config.walk_ratio,
            walk_type,
            dir,
            grandparent_node,
            2,
        )?;

        self.insert_walk(config, walk_type, Walk::new(walk_depth), grandparent_dir);

        Some(())
    }

    /// If a walk at `dir` suggests we can advance the depth of a containing walk, return
    /// (containing_dir, new_depth).
    fn should_advance_ancestor_walk<'a>(
        &mut self,
        config: &Config,
        walk_type: WalkType,
        dir: &'a RepoPath,
    ) -> Option<(&'a RepoPath, usize)> {
        let parent_dir = dir.parent()?;
        let (ancestor, suffix) = self.node.get_owning_node_mut(walk_type, parent_dir)?;
        let ancestor_dir = parent_dir.strip_suffix(suffix, true)?;

        let head = if suffix.is_empty() {
            // If we have no suffix, then `ancestor_dir == parent_dir`. The name of the advanced
            // child is the last part of `dir`.
            dir.split_last_component()?.1
        } else {
            // If we have a suffix, that means `ancestor_dir != parent_dir`. The name of the
            // advanced child is the first part of the suffix.
            suffix.split_first_component()?.0
        };

        let ancestor_depth = ancestor_dir.depth();
        let threshold = walk_threshold(config, ancestor_depth);

        // Check if the containing walk's node has N children with descendants that
        // have pushed to the next depth. The idea is we want some confidence before
        // expanding a huge walk deeper, so we wait until we've seen depth
        // advancements that bubble up to at least N different children of the walk
        // root.
        let (num_advanced_children, num_advanced_descendants, child_seen_count) =
            ancestor.insert_advanced_child(walk_type, head.to_owned());

        tracing::trace!(
            %ancestor_dir,
            num_advanced_children,
            num_advanced_descendants,
            child_seen_count,
            threshold,
            total_dirs=ancestor.total_dirs(),
            "should_advance_ancestor_walk",
        );

        let ancestor_walk_depth = ancestor
            .get_dominating_walk(walk_type)
            .map_or(0, |w| w.depth);

        // Check that both:
        //    - We are walked at depth=0 based on num_advanced_children. This ensures the walk is
        //    "wide" enough (i.e. enough of our direct children directories have advanced walks).
        //    - We are walked at depth=N+1 based on num_advanced_descendants. This ensures we don't
        //    prematurely advance across a huge directory at depth N+1.
        let is_walked = ancestor.is_walked(
            WalkType::Directory,
            num_advanced_children,
            0,
            threshold,
            config.walk_ratio,
        ) && ancestor.is_walked(
            WalkType::Directory,
            num_advanced_descendants,
            ancestor_walk_depth,
            threshold,
            config.walk_ratio,
        );

        if is_walked {
            let depth = ancestor_walk_depth + 1;
            tracing::debug!(dir=%ancestor_dir, depth, "expanding walk boundary");
            return Some((ancestor_dir, depth));
        } else if !suffix.is_empty() {
            // If suffix isn't empty (i.e. dir isn't a direct child of ancestor), check if
            // ancestor's direct child `name` should be split off into a separate walk. This helps
            // in cases such as root directory "foo" has a child directory "foo/bar" that is very
            // wide and deep. Ideally the walk at "foo" could advance to cover everything, but at
            // some point "foo/bar" will be its only child with activity. Splitting off a walk for
            // "foo/bar" allows all the activity under "foo/bar" to be consolidated easily to a
            // single walk.

            let threshold = walk_threshold(config, ancestor_depth + 1);
            let split_off_child = match ancestor.get_node(head.as_ref()) {
                // If we have directory size hints, use them.
                Some(child) => {
                    child.get_walk_for_type(walk_type).is_none()
                        // Similar to above, check for evidence of advanced walk activity for
                        // direct children and at depth N+1.
                        && child.is_walked(
                            WalkType::Directory,
                            child_seen_count,
                            0,
                            threshold,
                            config.walk_ratio,
                        )
                        && child.is_walked(
                            WalkType::Directory,
                            child_seen_count,
                            ancestor_walk_depth.saturating_sub(1),
                            threshold,
                            config.walk_ratio,
                        )
                }
                // Otherwise use default threshold.
                None => child_seen_count >= threshold,
            };

            if split_off_child {
                // If parent=ancestor/foo/bar/parent and suffix=foo/bar/parent,
                // we want "ancestor/foo", so strip suffix[1:] from parent.
                let child_path =
                    parent_dir.strip_suffix(suffix.split_first_component()?.1, true)?;
                tracing::debug!(%child_path, depth=ancestor_walk_depth, "splitting off walk for child");
                return Some((child_path, ancestor_walk_depth));
            }
        }

        None
    }

    #[allow(clippy::useless_conversion)]
    fn needs_gc(&self, config: &Config) -> bool {
        self.last_gc_time.elapsed() >= config.gc_interval.into()
    }

    /// Returns whether a walk was removed.
    fn maybe_gc(&mut self, config: &Config) -> bool {
        if !self.needs_gc(config) {
            return false;
        }

        let start = std::time::Instant::now();

        let (deleted_nodes, remaining_nodes, deleted_walks) = self.node.gc(config);

        let elapsed = start.elapsed();

        if deleted_nodes > 0 || deleted_walks > 0 || elapsed.as_millis() > 5 {
            tracing::debug!(
                ?elapsed,
                deleted_nodes,
                remaining_nodes,
                deleted_walks,
                "GC complete"
            );
        }

        self.last_gc_time = Instant::now();

        deleted_walks > 0
    }

    fn set_metadata(&mut self, config: &Config, dir: &RepoPath, num_files: usize, num_dirs: usize) {
        tracing::trace!(%dir, num_files, num_dirs, "setting directory metadata");
        self.node.set_metadata(config, dir, num_files, num_dirs);
    }
}

fn walk_threshold(config: &Config, walk_root_depth: usize) -> usize {
    // Add a threshold multiplier for walk roots that are shallower than `lax_depth`. This
    // makes it harder for walks to percolate "too high".
    if walk_root_depth < config.lax_depth {
        (config.strict_multiplier * config.walk_threshold).max(config.walk_threshold)
    } else {
        config.walk_threshold
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
    ancestor.iter(|node, depth| -> bool {
        if depth == ancestor_distance {
            if node.get_walk_for_type(walk_type).is_some() {
                kin_count += 1;
            }
        }

        depth < ancestor_distance
    });

    tracing::trace!(
        %dir,
        kin_count,
        ancestor_distance,
        walk_threshold,
        walk_ratio,
        total_dirs=ancestor.total_dirs(),
        "should_merge_into_ancestor",
    );

    if ancestor.is_walked(
        WalkType::Directory, // We are combining directories here, so always use directory count.
        kin_count,
        ancestor_distance - 1,
        walk_threshold,
        walk_ratio,
    ) {
        tracing::debug!(
            %dir,
            kin_count,
            ancestor_distance,
            walk_threshold,
            walk_ratio,
            total_dirs=ancestor.total_dirs(),
            "combining with collateral kin",
        );
        Some(ancestor_distance)
    } else {
        None
    }
}

// How many children must be accessed in a directory to consider the directory "walked".
const DEFAULT_WALK_THRESHOLD: usize = 3;

// Walk threshold multiplier for walks near the top of the repo.
const DEFAULT_STRICT_MULTIPLIER: usize = 20;

// Depth at which we no longer get the strict multiplier. 0 means we never get the multiplier.
const DEFAULT_LAX_DEPTH: usize = 0;

// If we know the total dir size, make sure walk threshold is at least 3% of dir size.
// This avoids detecting walks for enormous directories too quickly.
const DEFAULT_WALK_RATIO: f64 = 0.03;

// How often we garbage collect stale nodes.
// We do not rely on running a full GC to expire old walks, only to clean up memory.
const DEFAULT_GC_INTERVAL: Duration = Duration::from_secs(10);

// How stale a walk must be before we remove it.
const DEFAULT_GC_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone)]
struct Config {
    // "How many children must be accessed before we consider parent walked?"
    // This is the main threshold to tune detector aggro.
    walk_threshold: usize,
    // Walks with depth < lax_depth will use a stricter walk_threshold.
    lax_depth: usize,
    // Walk threshold multiplier for walks rooted shallower than lax_depth.
    strict_multiplier: usize,
    // A minimum walk threshold as ratio of total directory size. This only applies when we know the
    // size of the directory. This slows down walk detection in humongous directories.
    walk_ratio: f64,
    // How often we run a full GC to clean up memory. We do not rely on a full GC to functionally
    // expire inactive walks.
    gc_interval: Duration,
    // How long after a node was last accessed until we GC it.
    gc_timeout: Duration,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            walk_threshold: DEFAULT_WALK_THRESHOLD,
            lax_depth: DEFAULT_LAX_DEPTH,
            strict_multiplier: DEFAULT_STRICT_MULTIPLIER,
            walk_ratio: DEFAULT_WALK_RATIO,
            gc_interval: DEFAULT_GC_INTERVAL,
            gc_timeout: DEFAULT_GC_TIMEOUT,
        }
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

    // Most commonly seen pid for this walk.
    pid: AtomicU64,
    pid_detail: OnceLock<String>,

    // When walk first started - used to estimate walk duration.
    start_time: AtomicInstant,

    // Whether we have already logged the start of this walk.
    logged_start: AtomicBool,

    // Whether we have logged the end of this walk.
    #[cfg(test)]
    logged_end: Arc<AtomicBool>,
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
        let start_time = AtomicInstant::new();
        start_time.bump();
        Self {
            depth,
            start_time,
            ..Default::default()
        }
    }

    fn for_type(t: WalkType, depth: usize, initial_loads: u64, pid: u32) -> Self {
        let w = Self::new(depth);
        w.pid.store(pid as u64, Ordering::Relaxed);
        let counter = match t {
            WalkType::Directory => &w.dir_loads,
            WalkType::File => &w.file_loads,
        };
        counter.fetch_add(initial_loads, Ordering::Relaxed);
        w
    }

    fn absorb_counters(&self, other: &Self) {
        // If we are absorbing another walk whose start has already been logged, don't log the start
        // again. There might be a lot of start events as walks coalesce; the important event is
        // logged when the fully coalesced walk ends.
        if other.logged_start.load(Ordering::Relaxed) {
            // Race conditions are not a big deal.
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

        if self.maybe_swap_pid(
            other.pid.load(Ordering::Relaxed) as u32,
            other.total_accesses(),
        ) {
            // If we took the other walk's pid, then also take its pid detail, if present.
            if let Some(detail) = other.pid_detail.get() {
                let _ = self.pid_detail.set(detail.to_string());
            }
        }

        // Take the earliest start time.
        if let Some(other_start) = other.start_time.load() {
            if self
                .start_time
                .load()
                .is_none_or(|my_start| other_start < my_start)
            {
                self.start_time.store(other_start);
            }
        }
    }

    fn inc_file_load(&self, walk_root: &RepoPath, repo_root: Option<&Path>, path: &RepoPath) {
        if self.file_loads.fetch_add(1, Ordering::AcqRel) >= Self::BIG_WALK_THRESHOLD - 1 {
            self.maybe_log_big_walk(walk_root, repo_root, path);
        }
    }

    fn inc_file_read(&self, walk_root: &RepoPath, repo_root: Option<&Path>, path: &RepoPath) {
        if self.file_reads.fetch_add(1, Ordering::Relaxed) >= Self::BIG_WALK_THRESHOLD - 1 {
            self.maybe_log_big_walk(walk_root, repo_root, path);
        }
    }

    fn inc_dir_load(&self, walk_root: &RepoPath, repo_root: Option<&Path>, path: &RepoPath) {
        if self.dir_loads.fetch_add(1, Ordering::AcqRel) >= Self::BIG_WALK_THRESHOLD - 1 {
            self.maybe_log_big_walk(walk_root, repo_root, path);
        }
    }

    fn inc_dir_read(&self, walk_root: &RepoPath, repo_root: Option<&Path>, path: &RepoPath) {
        if self.dir_reads.fetch_add(1, Ordering::Relaxed) >= Self::BIG_WALK_THRESHOLD - 1 {
            self.maybe_log_big_walk(walk_root, repo_root, path);
        }
    }

    fn total_accesses(&self) -> u64 {
        self.dir_loads.load(Ordering::Relaxed)
            + self.dir_reads.load(Ordering::Relaxed)
            + self.file_loads.load(Ordering::Relaxed)
            + self.file_reads.load(Ordering::Relaxed)
    }

    /// Probabilistically set self.pid=pid based on size of numerator relative to size of self.
    /// Returns whether we took the new pid.
    fn maybe_swap_pid(&self, pid: u32, numerator: u64) -> bool {
        // If we already have pid detail filled in - don't swap to a new pid, lest they mismatch.
        if self.pid_detail.get().is_some() {
            return false;
        }

        let current_pid = self.pid();

        // Don't swap to a 0 pid to favor having some pid info available.
        if pid > 0
            && pid != current_pid
            && (current_pid == 0 || // Always take the new pid if current pid not set.
                // Give the new pid a probabilistic chance to "take over" based on its weight.
                rand::rng().random_range(0..self.total_accesses().max(1)) < numerator)
        {
            self.pid.store(pid as u64, Ordering::Relaxed);
            true
        } else {
            false
        }
    }

    fn maybe_log_big_walk(&self, walk_root: &RepoPath, repo_root: Option<&Path>, path: &RepoPath) {
        // Init the pid detail at the start of the walk. If we wait until the walk ends, the walking
        // process may have exited. We init pid_detail before checking logged_start because it is
        // possible that logged_start is true before pid_detail is set due to absorbing a logged
        // walk.
        self.pid_detail.get_or_init(|| {
            let mut pid = self.pid();

            // In practice we don't get pid on mac since EdenFS uses NFS. If this file path crossed
            // us into "big walk" territory, do an expensive lsof lookup to try to get the pid.
            if pid == 0 {
                if let Some(root) = repo_root {
                    if let Some(full_path) = root.join(path.as_str()).to_str() {
                        #[cfg(target_os = "macos")]
                        {
                            pid =
                                procinfo::macos::file_path_to_pid(std::path::Path::new(full_path));
                        }

                        #[cfg(not(target_os = "macos"))]
                        {
                            // It would make sense to fall back to the "lsof" logic for other
                            // platforms as well, but we currently only have it implemented for mac.
                            let _ = full_path;
                            pid = 0;
                        }

                        if pid > 0 {
                            self.pid.store(pid as u64, Ordering::Relaxed);
                        }
                    }
                }
            }

            // Short circuit procinfo call if we aren't going to log it.
            let tracing_enabled = tracing::enabled!(Level::INFO)
                || tracing::enabled!(target: "big_walk", Level::DEBUG);

            if tracing_enabled && pid > 0 {
                procinfo::ancestors(pid)
            } else {
                String::new()
            }
        });

        if self.logged_start.swap(true, Ordering::AcqRel) {
            return;
        }

        tracing::info!(
            root=%walk_root,
            file_loads = self.file_loads.load(Ordering::Relaxed),
            file_reads = self.file_reads.load(Ordering::Relaxed),
            file_preloads = self.file_preloads.load(Ordering::Relaxed),
            dir_loads = self.dir_loads.load(Ordering::Relaxed),
            dir_reads = self.dir_reads.load(Ordering::Relaxed),
            walker_detail = self.pid_detail.get(),
            "big walk started",
        );
    }

    fn log_end(&self, root: &RepoPath, end_time: Instant) {
        #[cfg(test)]
        self.logged_end.store(true, Ordering::Relaxed);

        if self.file_loads.load(Ordering::Relaxed) >= Self::BIG_WALK_THRESHOLD
            || self.file_reads.load(Ordering::Relaxed) >= Self::BIG_WALK_THRESHOLD
            || self.file_preloads.load(Ordering::Relaxed) >= Self::BIG_WALK_THRESHOLD
            || self.dir_loads.load(Ordering::Relaxed) >= Self::BIG_WALK_THRESHOLD
            || self.dir_reads.load(Ordering::Relaxed) >= Self::BIG_WALK_THRESHOLD
        {
            let duration = self
                .start_time
                .load()
                .map(|t| end_time.duration_since(t).as_secs())
                .unwrap_or_default();

            tracing::info!(
                %root,
                file_loads = self.file_loads.load(Ordering::Relaxed),
                file_reads = self.file_reads.load(Ordering::Relaxed),
                file_preloads = self.file_preloads.load(Ordering::Relaxed),
                dir_loads = self.dir_loads.load(Ordering::Relaxed),
                dir_reads = self.dir_reads.load(Ordering::Relaxed),
                walk_depth = self.depth,
                walker_detail = self.pid_detail.get(),
                walk_duration = duration,
                "big walk ended",
            );

            tracing::debug!(
                target: "big_walk",
                walk_root = root.as_str(),
                file_loads = self.file_loads.load(Ordering::Relaxed),
                file_reads = self.file_reads.load(Ordering::Relaxed),
                file_preloads = self.file_preloads.load(Ordering::Relaxed),
                dir_loads = self.dir_loads.load(Ordering::Relaxed),
                dir_reads = self.dir_reads.load(Ordering::Relaxed),
                walk_depth = self.depth,
                walker_detail = self.pid_detail.get(),
                walk_duration = duration,
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

    fn pid(&self) -> u32 {
        self.pid.load(Ordering::Relaxed) as u32
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

    /// Set time to "now".
    fn bump(&self) {
        self.store(Instant::now());
    }

    fn store(&self, value: Instant) {
        let epoch = *EPOCH;

        // It is theoretically possible for `value` to be smaller than EPOCH.
        let int_value = if value < epoch {
            0
        } else {
            value.duration_since(epoch).as_millis() as i64
        };

        self.0.store(int_value, Ordering::Relaxed);
    }

    #[allow(clippy::useless_conversion)]
    fn load(&self) -> Option<Instant> {
        match self.0.load(Ordering::Relaxed) {
            v if v < 0 => None,
            v => Some(*EPOCH + Duration::from_millis(v as u64).into()),
        }
    }

    /// Reset to "not set" state. `load()` will return `None` until `store()` is called.
    fn reset(&self) {
        self.0.store(i64::MIN, Ordering::Relaxed);
    }
}
