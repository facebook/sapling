/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#[cfg(test)]
mod tests;
mod walk_node;

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
    node: WalkNode,
}

impl Default for Inner {
    fn default() -> Self {
        Self {
            min_dir_walk_threshold: DEFAULT_MIN_DIR_WALK_THRESHOLD,
            node: WalkNode::default(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Walk {
    depth: usize,
    last_access: Instant,
}

// How many children must be accessed in a directory to consider the directory "walked".
const DEFAULT_MIN_DIR_WALK_THRESHOLD: usize = 2;

impl Detector {
    pub fn new() -> Self {
        Self {
            inner: Default::default(),
        }
    }

    pub fn set_min_dir_walk_threshold(&self, threshold: usize) {
        self.inner.lock().min_dir_walk_threshold = threshold;
    }

    /// Return list of (walk root dir, walk depth) representing active walks.
    pub fn walks(&self) -> Vec<(RepoPathBuf, usize)> {
        let mut walks = self
            .inner
            .lock()
            .node
            .list_walks()
            .into_iter()
            .map(|(root, walk)| (root, walk.depth))
            .collect::<Vec<_>>();

        walks.sort_by(|a, b| a.0.as_str().cmp(b.0.as_str()));

        walks
    }

    /// Observe a file (content) read of `path` at time `time`.
    pub fn file_read(&self, time: Instant, mut path: RepoPathBuf) {
        tracing::trace!(?time, %path, "file_read");

        let (dir_path, base_name) = match path.pop() {
            // Shouldn't happen - implies a path of "" which is not valid for a file.
            None => return,
            Some(part) => (path, part),
        };

        let mut inner = self.inner.lock();

        let dir_threshold = inner.min_dir_walk_threshold;

        let (owner, suffix) = inner.node.get_or_create_owning_node(&dir_path);

        if let Some(walk) = owner.walk.as_mut() {
            tracing::trace!(walk_root=%dir_path.strip_suffix(suffix, true).unwrap_or_default(), dir=%dir_path, "dir in walk");
            walk.last_access = time;
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
    pub fn dir_read(&self, time: Instant, path: RepoPathBuf, num_files: usize, num_dirs: usize) {
        tracing::trace!(?time, %path, num_files, num_dirs, "dir_read");

        let mut inner = self.inner.lock();
        let (owner, _suffix) = inner.node.get_or_create_owning_node(&path);

        if owner.walk.is_some() {
            // directory already part of a walk - don't track metadata
        } else {
            owner.total_files = Some(num_files);
            owner.total_dirs = Some(num_dirs);
        }
    }
}

impl Inner {
    /// Insert a new Walk rooted at `dir`.
    fn insert_walk(&mut self, time: Instant, dir: &RepoPath, walk_depth: usize) {
        // TODO: consider moving "should merge" logic into `WalkNode::insert_walk` to do
        // more work in a single traversal.

        tracing::debug!(%dir, depth=walk_depth, "new walk");

        // Check if we should immediately promote this walk to parent directory. This is
        // similar to the ancestor advancement below, except that it can insert a new
        // walk.
        if let Some((parent_dir, parent_depth)) = self.should_merge_into_parent(dir, walk_depth) {
            self.insert_walk(time, parent_dir, parent_depth);
            return;
        }

        tracing::debug!(%dir, depth=walk_depth, "inserting walk");
        self.node.insert_walk(
            dir,
            Walk {
                depth: walk_depth,
                last_access: time,
            },
            self.min_dir_walk_threshold,
        );

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
        let (parent_dir, name) = dir.split_last_component()?;
        let parent_node = self.node.get_node(parent_dir)?;

        // If this walk already exists, there is no combining to be done.
        if parent_node.get_walk(name.as_ref()).is_some() {
            return None;
        }

        // Check if there are sibling walks that we want to merge into a walk
        // on the parent.

        let mut sibling_count = 0;
        let max_sibling_depth = parent_node.child_walks().fold(0, |max, (_, walk)| {
            sibling_count += 1;
            max.max(walk.depth)
        });

        if sibling_count >= (self.min_dir_walk_threshold - 1) {
            if tracing::enabled!(tracing::Level::DEBUG) {
                let siblings_display = parent_node
                    .child_walks()
                    .map(|(name, walk)| format!("{}:{}", parent_dir.join(name), walk.depth))
                    .collect::<Vec<_>>();
                tracing::debug!(siblings=?siblings_display, "combining with siblings");
            }

            walk_depth = walk_depth.max(max_sibling_depth);
            walk_depth = walk_depth.max(parent_node.walk.map_or(0, |w| w.depth));
            Some((parent_dir, walk_depth + 1))
        } else if parent_node
            .total_dirs
            .is_some_and(|total| total < self.min_dir_walk_threshold)
        {
            tracing::debug!("promoting due to few dirs");
            Some((parent_dir, walk_depth + 1))
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
        let (ancestor, suffix) = self.node.get_containing_node(parent_dir)?;
        let ancestor_dir = parent_dir.strip_suffix(suffix, true)?;
        let (head, _) = suffix.split_first_component()?;

        // Check if the containing walk's node has N children with descendants that
        // have pushed to the next depth. The idea is we want some confidence before
        // expanding a huge walk deeper, so we wait until we've seen depth
        // advancements that bubble up to at least N different children of the walk
        // root.
        if ancestor.advanced_children.insert(head.to_owned()) {
            if ancestor.advanced_children.len() >= self.min_dir_walk_threshold
                || ancestor
                    .total_dirs
                    .is_some_and(|total| total < self.min_dir_walk_threshold)
            {
                let depth = ancestor.walk.map_or(0, |w| w.depth) + 1;
                tracing::debug!(dir=%ancestor_dir, depth, "expanding walk boundary");
                return Some((ancestor_dir, depth));
            }
        }

        None
    }
}
