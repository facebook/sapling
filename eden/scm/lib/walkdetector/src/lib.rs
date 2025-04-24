/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#[cfg(test)]
mod tests;

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
    min_dir_walk_threhsold: usize,
    inner: Mutex<Inner>,
}

#[derive(Default)]
struct Inner {
    walks: HashMap<RepoPathBuf, Walk>,
    dirs: HashMap<RepoPathBuf, Dir>,
}

#[derive(Debug)]
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
            min_dir_walk_threhsold: DEFAULT_MIN_DIR_WALK_THRESHOLD,
            inner: Default::default(),
        }
    }

    /// Return list of (walk root dir, walk depth) representing active walks.
    pub fn walks(&self) -> Vec<(RepoPathBuf, usize)> {
        self.inner
            .lock()
            .walks
            .iter()
            .map(|(root, walk)| (root.to_owned(), walk.depth))
            .collect()
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

        let mut entry = match inner.dirs.entry(dir_path) {
            Entry::Occupied(entry) => entry,
            Entry::Vacant(entry) => entry.insert_entry(Dir {
                seen_files: Default::default(),
            }),
        };

        let dir = entry.get_mut();
        dir.seen_files.insert(base_name);

        if entry.get().is_walked(self.min_dir_walk_threhsold) {
            // Transition Dir entry to Walk.
            let (dir_path, _dir) = entry.remove_entry();
            inner.insert_walk(time, dir_path);
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
    fn insert_walk(&mut self, time: Instant, dir: RepoPathBuf) {
        self.walks.insert(
            dir,
            Walk {
                depth: 0,
                last_access: time,
            },
        );
    }
}
