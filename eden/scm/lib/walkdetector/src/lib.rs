/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#[cfg(test)]
mod tests;
mod walk_node;

use std::collections::HashMap;
use std::collections::HashSet;
use std::collections::hash_map::Entry;
use std::time::Instant;

use parking_lot::Mutex;
use types::PathComponentBuf;
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
    dirs: HashMap<RepoPathBuf, Dir>,
}

impl Default for Inner {
    fn default() -> Self {
        Self {
            min_dir_walk_threshold: DEFAULT_MIN_DIR_WALK_THRESHOLD,
            node: WalkNode::default(),
            dirs: Default::default(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Walk {
    depth: usize,
    last_access: Instant,
}

struct Dir {
    seen_files: HashSet<PathComponentBuf>,
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
            .list()
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

        if let Some(walk) = inner.node.get_containing(&dir_path) {
            tracing::trace!(dir=%dir_path, "dir in walk");
            walk.last_access = time;
            return;
        }

        let dir_threshold = inner.min_dir_walk_threshold;

        let mut entry = match inner.dirs.entry(dir_path) {
            Entry::Occupied(entry) => entry,
            Entry::Vacant(entry) => entry.insert_entry(Dir {
                seen_files: Default::default(),
            }),
        };

        let dir = entry.get_mut();
        dir.seen_files.insert(base_name);

        if entry.get().is_walked(dir_threshold) {
            // Transition Dir entry to Walk.
            let (dir_path, _dir) = entry.remove_entry();
            inner.insert_walk(time, &dir_path, 0);
        }
    }
}

impl Dir {
    /// Return whether this Dir should be considered "walked".
    fn is_walked(&self, dir_walk_threshold: usize) -> bool {
        self.seen_files.len() >= dir_walk_threshold
    }
}

impl Inner {
    /// Insert a new Walk rooted at `dir`.
    fn insert_walk(&mut self, time: Instant, dir: &RepoPath, mut walk_depth: usize) {
        tracing::debug!(%dir, depth=walk_depth, "new walk");

        if let Some((parent_dir, name)) = dir.split_last_component() {
            if let Some(parent_node) = self.node.get_node(parent_dir) {
                // If this walk already exists, there is no combining to be done.
                if parent_node.get(name.as_ref()).is_none() {
                    // We are adding a new walk - check if it has sibling walks that we
                    // want to merge into a walk on the parent.

                    let mut sibling_count = 0;
                    let max_sibling_depth = parent_node.child_walks().fold(0, |max, (_, walk)| {
                        sibling_count += 1;
                        max.max(walk.depth)
                    });

                    if sibling_count >= (self.min_dir_walk_threshold - 1) {
                        if tracing::enabled!(tracing::Level::DEBUG) {
                            let siblings_display = parent_node
                                .child_walks()
                                .map(|(name, walk)| {
                                    format!(
                                        "{}:{}",
                                        dir.parent().unwrap_or_default().join(name),
                                        walk.depth
                                    )
                                })
                                .collect::<Vec<_>>();
                            tracing::debug!(siblings=?siblings_display, "combining with siblings");
                        }

                        walk_depth = walk_depth.max(max_sibling_depth) + 1;
                        walk_depth = walk_depth.max(parent_node.walk.map_or(0, |w| w.depth));
                        self.insert_walk(time, parent_dir, walk_depth);
                        return;
                    }
                }
            }
        }

        tracing::debug!(%dir, depth=walk_depth, "inserting walk");
        self.node.insert(
            dir,
            Walk {
                depth: walk_depth,
                last_access: time,
            },
        );
    }
}
