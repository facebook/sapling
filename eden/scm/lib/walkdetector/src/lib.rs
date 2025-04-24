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

use anyhow::Result;
use parking_lot::Mutex;
use types::PathComponentBuf;
use types::RepoPath;
use types::RepoPathBuf;

// Goals:
//  - Aggressively detect walk and aggressively cancel walk.
//  - Passive - don't fetch or query any stores.
//  - Minimize memory usage.

pub struct Detector {
    inner: Mutex<Inner>,
}

struct Inner {
    min_dir_walk_threshold: usize,
    walks: HashMap<RepoPathBuf, Walk>,
    dirs: HashMap<RepoPathBuf, Dir>,
}

impl Default for Inner {
    fn default() -> Self {
        Self {
            min_dir_walk_threshold: DEFAULT_MIN_DIR_WALK_THRESHOLD,
            walks: Default::default(),
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

    #[cfg(test)]
    fn set_min_dir_walk_threshold(&mut self, threshold: usize) {
        self.inner.lock().min_dir_walk_threshold = threshold;
    }

    /// Return list of (walk root dir, walk depth) representing active walks.
    pub fn walks(&self) -> Vec<(RepoPathBuf, usize)> {
        let mut walks = self
            .inner
            .lock()
            .walks
            .iter()
            .map(|(root, walk)| (root.to_owned(), walk.depth))
            .collect::<Vec<_>>();

        walks.sort_by(|a, b| a.0.as_str().cmp(b.0.as_str()));

        walks
    }

    /// Observe a file (content) read of `path` at time `time`.
    pub fn file_read(&self, time: Instant, mut path: RepoPathBuf) -> Result<()> {
        tracing::trace!(?time, %path, "file_read");

        let (dir_path, base_name) = match path.pop() {
            // Shouldn't happen - implies a path of "" which is not valid for a file.
            None => return Ok(()),
            Some(part) => (path, part),
        };

        let mut inner = self.inner.lock();

        if let Some(walk) = inner.containing_walk(&dir_path) {
            walk.last_access = time;
            return Ok(());
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
            inner.insert_walk(time, dir_path, 0);
        }

        Ok(())
    }
}

impl Dir {
    /// Return whether this Dir should be considered "walked".
    fn is_walked(&self, dir_walk_threshold: usize) -> bool {
        self.seen_files.len() >= dir_walk_threshold
    }
}

impl Inner {
    /// Return Walk that contains `dir`, if any.
    fn containing_walk(&mut self, dir: &RepoPath) -> Option<&mut Walk> {
        self.walks.iter_mut().find_map(|(root, walk)| {
            if let Some(suffix) = dir.strip_prefix(root, true) {
                if suffix.components().count() <= walk.depth {
                    return Some(walk);
                }
            }

            None
        })
    }

    /// Insert a new Walk rooted at `dir`.
    fn insert_walk(&mut self, time: Instant, dir: RepoPathBuf, mut walk_depth: usize) {
        let mut siblings = Vec::new();

        for root in self.walks.keys() {
            if dir.parent() == root.parent() {
                siblings.push(root.to_owned());
            }
        }

        if siblings.len() >= (self.min_dir_walk_threshold - 1) {
            let max_sibling_depth = siblings.iter().fold(0, |max, sibling_path| {
                max.max(self.walks.remove(sibling_path).map_or(0, |c| c.depth))
            });

            if let Some(parent) = dir.parent() {
                walk_depth = max_sibling_depth.max(walk_depth) + 1;

                if let Some(parent_walk) = self.walks.get_mut(parent) {
                    parent_walk.last_access = time;
                    parent_walk.depth = parent_walk.depth.max(walk_depth);
                } else {
                    self.insert_walk(time, parent.to_owned(), walk_depth);
                }
            }
        } else {
            self.walks.insert(
                dir,
                Walk {
                    depth: walk_depth,
                    last_access: time,
                },
            );
        }
    }
}
