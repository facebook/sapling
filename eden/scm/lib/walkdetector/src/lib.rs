/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#[cfg(test)]
mod tests;
mod walk_node;

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Walk {
    depth: usize,
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

    /// Return list of (walk root dir, walk depth) representing active walks.
    pub fn walks(&self) -> Vec<(RepoPathBuf, usize)> {
        let mut inner = self.inner.lock();

        let time = inner.now();
        inner.maybe_gc(time);

        let mut walks = inner
            .node
            .list_walks(WalkType::File)
            .into_iter()
            .map(|(root, walk)| (root, walk.depth))
            .collect::<Vec<_>>();

        walks.sort_by(|a, b| a.0.as_str().cmp(b.0.as_str()));

        walks
    }

    /// Observe a file (content) read of `path`.
    pub fn file_read(&self, mut path: RepoPathBuf) {
        tracing::trace!(%path, "file_read");

        let (dir_path, base_name) = match path.pop() {
            // Shouldn't happen - implies a path of "" which is not valid for a file.
            None => return,
            Some(part) => (path, part),
        };

        let mut inner = self.inner.lock();

        let time = inner.now();

        inner.maybe_gc(time);

        let dir_threshold = inner.min_dir_walk_threshold;

        let (owner, suffix) = inner
            .node
            .get_or_create_owning_node(WalkType::File, &dir_path);

        owner.last_access = Some(time);

        if owner.get_walk_for_type(WalkType::File).is_some() {
            tracing::trace!(walk_root=%dir_path.strip_suffix(suffix, true).unwrap_or_default(), dir=%dir_path, "dir in walk");
            return;
        }

        let my_dir = owner;

        my_dir.seen_files.insert(base_name);

        if my_dir.is_walked(dir_threshold) {
            my_dir.seen_files.clear();
            inner.insert_walk(time, &dir_path, 0);
        }
    }

    /// Observe a directory read. `num_files` and `num_dirs` report the number of file and
    /// directory children of `path`, respectively.
    pub fn dir_read(&self, path: RepoPathBuf, num_files: usize, num_dirs: usize) {
        tracing::trace!(%path, num_files, num_dirs, "dir_read");

        let mut inner = self.inner.lock();

        let time = inner.now();

        inner.maybe_gc(time);

        // Fill in interesting metadata that informs detection of file content walks.
        if interesting_metadata(
            inner.min_dir_walk_threshold,
            Some(num_files),
            Some(num_dirs),
        ) {
            let node = inner.node.get_or_create_node(&path);
            node.last_access = Some(time);
            node.total_dirs = Some(num_dirs);
            node.total_files = Some(num_files);
        }
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
    fn insert_walk(&mut self, time: Instant, dir: &RepoPath, walk_depth: usize) {
        // TODO: consider moving "should merge" logic into `WalkNode::insert_walk` to do
        // more work in a single traversal.

        tracing::debug!(%dir, depth=walk_depth, "inserting walk");
        let walk_node = self.node.insert_walk(
            WalkType::File,
            dir,
            Walk { depth: walk_depth },
            self.min_dir_walk_threshold,
        );
        walk_node.last_access = Some(time);

        // Check if we should immediately promote this walk to parent directory. This is
        // similar to the ancestor advancement below, except that it can insert a new
        // walk.
        if let Some((parent_dir, parent_depth)) = self.should_merge_into_parent(dir, walk_depth) {
            self.insert_walk(time, parent_dir, parent_depth);
            return;
        }

        // Check if we should merge with cousins (into grandparent).
        // TODO: combine this with the merge-into-parent heuristic.
        if self
            .maybe_merge_into_grandparent(time, dir, walk_depth)
            .is_some()
        {
            return;
        }

        // Check if we have a containing walk whose depth boundary should be increased.
        if let Some((ancestor_dir, new_depth)) = self.should_advance_ancestor_walk(dir) {
            self.insert_walk(time, ancestor_dir, new_depth);
        }
    }

    /// If a new walk at `dir` should instead be promoted to a walk at dir's parent dir,
    /// return (parent_dir, new_depth).
    fn should_merge_into_parent<'a>(
        &mut self,
        dir: &'a RepoPath,
        mut walk_depth: usize,
    ) -> Option<(&'a RepoPath, usize)> {
        tracing::debug!(%dir, "should_merge_into_parent");

        let (parent_dir, name) = dir.split_last_component()?;
        let parent_node = self.node.get_node(parent_dir)?;

        // Check if there are sibling walks that we want to merge into a walk
        // on the parent.

        let mut sibling_count = 0;
        let mut saw_self = false;
        let max_sibling_depth =
            parent_node
                .child_walks(WalkType::File)
                .fold(0, |max, (sibling, walk)| {
                    sibling_count += 1;
                    saw_self = name == sibling;
                    max.max(walk.depth)
                });

        // This walk hasn't been inserted - count as sibling.
        if !saw_self {
            sibling_count += 1;
        }

        tracing::debug!(%dir, sibling_count, parent_dirs=?parent_node.total_dirs);

        if sibling_count >= self.min_dir_walk_threshold {
            if tracing::enabled!(tracing::Level::DEBUG) {
                let siblings_display = parent_node
                    .child_walks(WalkType::File)
                    .map(|(name, walk)| format!("{}:{}", parent_dir.join(name), walk.depth))
                    .collect::<Vec<_>>();
                tracing::debug!(%dir, siblings=?siblings_display, "combining with siblings");
            }

            walk_depth = walk_depth.max(max_sibling_depth);
            walk_depth = walk_depth.max(
                parent_node
                    .get_walk_for_type(WalkType::File)
                    .map_or(0, |w| w.depth),
            );
            Some((parent_dir, walk_depth + 1))
        } else if parent_node
            .total_dirs
            .is_some_and(|total| total < self.min_dir_walk_threshold)
        {
            tracing::debug!(%dir, "promoting due to few dirs");
            Some((parent_dir, walk_depth + 1))
        } else {
            None
        }
    }

    fn maybe_merge_into_grandparent(
        &mut self,
        time: Instant,
        dir: &RepoPath,
        mut walk_depth: usize,
    ) -> Option<()> {
        let parent_dir = dir.parent()?;
        let grandparent_dir = parent_dir.parent()?;

        let (ancestor, suffix) = self.node.get_containing_node(WalkType::File, parent_dir)?;
        if suffix.is_empty() {
            return None;
        }

        let grandparent_node = ancestor.get_node(suffix.parent()?)?;

        let mut cousin_count = 0;
        grandparent_node.iter(|node, depth| -> bool {
            if depth > 2 {
                return false;
            }

            if depth == 2 {
                if let Some(walk) = node.get_walk_for_type(WalkType::File) {
                    cousin_count += 1;
                    walk_depth = walk_depth.max(walk.depth);
                }
            }

            true
        });

        if cousin_count >= self.min_dir_walk_threshold {
            tracing::debug!(%dir, %grandparent_dir, cousin_count, "combining with cousins");
            self.insert_walk(time, grandparent_dir, walk_depth + 2);
            Some(())
        } else if grandparent_node
            .total_dirs
            .is_some_and(|total| total < self.min_dir_walk_threshold)
        {
            tracing::debug!(%dir, "promoting cousins due to few dirs");
            self.insert_walk(time, grandparent_dir, walk_depth + 2);
            Some(())
        } else {
            None
        }
    }

    /// If a walk at `dir` suggests we can advance the depth of a containing walk, return
    /// (containing_dir, new_depth).
    fn should_advance_ancestor_walk<'a>(
        &mut self,
        dir: &'a RepoPath,
    ) -> Option<(&'a RepoPath, usize)> {
        let parent_dir = dir.parent()?;
        let (ancestor, suffix) = self.node.get_containing_node(WalkType::File, parent_dir)?;
        let ancestor_dir = parent_dir.strip_suffix(suffix, true)?;
        let (head, _) = suffix.split_first_component()?;

        // Check if the containing walk's node has N children with descendants that
        // have pushed to the next depth. The idea is we want some confidence before
        // expanding a huge walk deeper, so we wait until we've seen depth
        // advancements that bubble up to at least N different children of the walk
        // root.
        if ancestor.insert_advanced_child(WalkType::File, head.to_owned())
            >= self.min_dir_walk_threshold
        {
            let depth = ancestor
                .get_walk_for_type(WalkType::File)
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

    fn maybe_gc(&mut self, time: Instant) {
        if time - self.last_gc_time < self.gc_interval {
            return;
        }

        self.node.gc(self.gc_timeout, time);

        self.last_gc_time = time;
    }
}
