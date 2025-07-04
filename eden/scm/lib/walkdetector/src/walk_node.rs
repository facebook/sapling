/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::HashMap;
use std::collections::HashSet;
use std::time::Duration;

use types::PathComponentBuf;
use types::RepoPath;
use types::RepoPathBuf;

use crate::AtomicInstant;
use crate::Config;
use crate::Walk;
use crate::WalkType;
use crate::important_metadata;
use crate::interesting_metadata;
use crate::walk_threshold;

/// Tree structure to track active walks. This makes it efficient to find a file's
/// "containing" walk, and to efficiently discover a walk's siblings, cousins, etc. in
/// order to merge walks.
pub(crate) struct WalkNode {
    // File content walk, if any, rooted at this node.
    // The Duration is the GC timeout.
    pub(crate) file_walk: Option<Walk>,
    // Directory content walk, if any, rooted at this node.
    // The Duration is the GC timeout.
    pub(crate) dir_walk: Option<Walk>,

    pub(crate) last_access: AtomicInstant,
    pub(crate) gc_timeout: Duration,
    pub(crate) children: HashMap<PathComponentBuf, WalkNode>,

    // Child directories that have a walked descendant "advanced" past our current
    // walk.depth.
    pub(crate) advanced_file_children: HashMap<PathComponentBuf, usize>,
    pub(crate) advanced_dir_children: HashMap<PathComponentBuf, usize>,

    // Total file count in this directory (if hint available).
    pub(crate) total_files: Option<usize>,
    // Total directory count in this directory (if hint available).
    pub(crate) total_dirs: Option<usize>,
    // File names seen so far (only used before transitioning to walk).
    pub(crate) seen_files: HashSet<PathComponentBuf>,
    // Dir names seen so far (only used before transitioning to walk).
    pub(crate) seen_dirs: HashSet<PathComponentBuf>,
}

impl WalkNode {
    pub(crate) fn new(gc_timeout: Duration) -> Self {
        let node = Self {
            gc_timeout,
            file_walk: Default::default(),
            dir_walk: Default::default(),
            last_access: Default::default(),
            children: Default::default(),
            advanced_file_children: Default::default(),
            advanced_dir_children: Default::default(),
            total_files: Default::default(),
            total_dirs: Default::default(),
            seen_files: Default::default(),
            seen_dirs: Default::default(),
        };
        node.last_access.bump();
        node
    }

    /// Get existing WalkNode entry for specified dir, if any.
    pub(crate) fn get_node(&self, dir: &RepoPath) -> Option<&Self> {
        match dir.split_first_component() {
            Some((head, tail)) => self
                .children
                .get(head)
                .and_then(|child| child.get_node(tail)),
            None => Some(self),
        }
    }

    /// Find node with active walk covering directory `dir`, if any.
    pub(crate) fn get_owning_node<'a, 'b>(
        &'a self,
        walk_type: WalkType,
        dir: &'b RepoPath,
    ) -> Option<(&'a Self, &'b RepoPath)> {
        match dir.split_first_component() {
            Some((head, tail)) => {
                if self.contains(walk_type, dir, 0) {
                    Some((self, dir))
                } else {
                    self.children
                        .get(head)
                        .and_then(|child| child.get_owning_node(walk_type, tail))
                }
            }
            None => {
                if self.get_dominating_walk(walk_type).is_some() {
                    Some((self, dir))
                } else {
                    None
                }
            }
        }
    }

    /// Find node with active walk covering directory `dir`, if any.
    pub(crate) fn get_owning_node_mut<'a, 'b>(
        &'a mut self,
        walk_type: WalkType,
        dir: &'b RepoPath,
    ) -> Option<(&'a mut Self, &'b RepoPath)> {
        match dir.split_first_component() {
            Some((head, tail)) => {
                if self.contains(walk_type, dir, 0) {
                    Some((self, dir))
                } else {
                    self.children
                        .get_mut(head)
                        .and_then(|child| child.get_owning_node_mut(walk_type, tail))
                }
            }
            None => {
                if self.get_dominating_walk(walk_type).is_some() {
                    Some((self, dir))
                } else {
                    None
                }
            }
        }
    }

    /// Find node with active walk covering `dir`, or create new node for `dir`. This is a
    /// single step to perform the common get-or-create operation in a single tree
    /// traversal.
    pub(crate) fn get_or_create_owning_node<'a>(
        &'a mut self,
        config: &Config,
        walk_type: WalkType,
        dir: &'a RepoPath,
    ) -> (&'a mut Self, &'a RepoPath) {
        match dir.split_first_component() {
            Some((head, tail)) => {
                if self.contains(walk_type, dir, 0) {
                    (self, dir)
                } else if self.children.contains_key(head) {
                    self.children
                        .get_mut(head)
                        .unwrap()
                        .get_or_create_owning_node(config, walk_type, tail)
                } else {
                    self.children
                        .entry(head.to_owned())
                        .or_insert_with(|| Self::new(config.gc_timeout))
                        .get_or_create_owning_node(config, walk_type, tail)
                }
            }
            None => {
                // Perform a JIT "light" GC.
                if self.expired() {
                    self.clear_except_children(config);
                }
                (self, dir)
            }
        }
    }

    /// Find or create node for `dir`.
    pub(crate) fn get_or_create_node<'a>(
        &'a mut self,
        config: &Config,
        dir: &'a RepoPath,
    ) -> &'a mut Self {
        match dir.split_first_component() {
            Some((head, tail)) => {
                if self.children.contains_key(head) {
                    self.children
                        .get_mut(head)
                        .unwrap()
                        .get_or_create_node(config, tail)
                } else {
                    self.children
                        .entry(head.to_owned())
                        .or_insert_with(|| Self::new(config.gc_timeout))
                        .get_or_create_node(config, tail)
                }
            }
            None => {
                // Perform a JIT "light" GC.
                if self.expired() {
                    self.clear_except_children(config);
                }
                self
            }
        }
    }

    /// Insert a new walk. Any redundant/contained walks will be removed. `walk` will not
    /// be inserted if it is contained by an ancestor walk.
    pub(crate) fn insert_walk(
        &mut self,
        config: &Config,
        walk_type: WalkType,
        walk_root: &RepoPath,
        mut walk: Walk,
        root_depth: usize,
    ) -> &mut Self {
        // If we completely overlap with the walk to be inserted, skip it. This shouldn't
        // happen, but I want to guarantee there are no overlapping walks.
        if self.contains(walk_type, walk_root, walk.depth) {
            if let Some(existing) = self.get_walk_for_type(walk_type) {
                existing.absorb_counters(&walk);
            }
            return self;
        }

        match walk_root.split_first_component() {
            Some((head, tail)) => {
                if self.children.contains_key(head) {
                    self.children
                        .get_mut(head)
                        .unwrap()
                        .insert_walk(config, walk_type, tail, walk, root_depth)
                } else {
                    self.children
                        .entry(head.to_owned())
                        .or_insert_with(|| Self::new(config.gc_timeout))
                        .insert_walk(config, walk_type, tail, walk, root_depth)
                }
            }
            None => {
                let threshold = walk_threshold(config, root_depth);
                let walk_ratio = config.walk_ratio;

                self.clear_advanced_children(walk_type);

                // This can have a side effect of adding to self.advanced_children.
                self.remove_contained(walk_type, &walk, threshold, walk_ratio);

                let seen_count = self.advanced_children_len(walk_type);
                if self.is_walked(WalkType::Directory, seen_count, threshold, walk_ratio) {
                    walk.depth += 1;
                    self.insert_walk(config, walk_type, walk_root, walk, root_depth)
                } else {
                    self.set_walk_for_type(walk_type, Some(walk));
                    self
                }
            }
        }
    }

    /// List all active walks.
    pub(crate) fn list_walks(&self, walk_type: WalkType) -> Vec<(RepoPathBuf, usize)> {
        fn inner(
            node: &WalkNode,
            walk_type: WalkType,
            path: &mut RepoPathBuf,
            list: &mut Vec<(RepoPathBuf, usize)>,
        ) {
            if let Some(walk) = node.get_walk_for_type(walk_type) {
                list.push((path.clone(), walk.depth));
            }

            for (name, child) in node.children.iter() {
                path.push(name.as_path_component());
                inner(child, walk_type, path, list);
                path.pop();
            }
        }

        let mut list = Vec::new();
        inner(self, walk_type, &mut RepoPathBuf::new(), &mut list);
        list
    }

    /// Get most "powerful" walk that covers `walk_type`. Basically, a file walk covers a
    /// directory walk, so if walk_type=Directory, we return `self.file_walk ||
    /// self.dir_walk`.
    pub(crate) fn get_dominating_walk(&self, walk_type: WalkType) -> Option<&Walk> {
        let walk = match walk_type {
            WalkType::File => self.file_walk.as_ref(),
            WalkType::Directory => self.file_walk.as_ref().or(self.dir_walk.as_ref()),
        };

        walk.and_then(|walk| if self.expired() { None } else { Some(walk) })
    }

    pub(crate) fn get_walk_for_type(&self, walk_type: WalkType) -> Option<&Walk> {
        let walk = match walk_type {
            WalkType::File => self.file_walk.as_ref(),
            WalkType::Directory => self.dir_walk.as_ref(),
        };

        walk.and_then(|walk| if self.expired() { None } else { Some(walk) })
    }

    /// Set walk of `walk_type` to new_walk. Returns old walk, if any.
    fn set_walk_for_type(&mut self, walk_type: WalkType, new_walk: Option<Walk>) -> Option<Walk> {
        // File walk implies directory walk, so clear out contained directory walk.
        match (walk_type, &new_walk, &self.dir_walk) {
            (
                WalkType::File,
                Some(new_walk @ Walk { depth, .. }),
                Some(
                    dir_walk @ Walk {
                        depth: dir_depth, ..
                    },
                ),
            ) if depth >= dir_depth => {
                new_walk.absorb_counters(dir_walk);
                self.dir_walk = None;
            }
            _ => {}
        }

        let old_walk = match walk_type {
            WalkType::File => &mut self.file_walk,
            WalkType::Directory => &mut self.dir_walk,
        };

        if let (Some(old), Some(new)) = (&old_walk, &new_walk) {
            new.absorb_counters(old);
        }

        std::mem::replace(old_walk, new_walk)
    }

    /// Mark name as an advanced child of self (i.e. a descendant under name has a walk that has advanced one level deeper than our walk).
    /// Returns (total advanced children, name advanced count).
    pub(crate) fn insert_advanced_child(
        &mut self,
        walk_type: WalkType,
        name: PathComponentBuf,
    ) -> (usize, usize) {
        let map = match walk_type {
            WalkType::File => &mut self.advanced_file_children,
            WalkType::Directory => &mut self.advanced_dir_children,
        };

        let counter = {
            let counter = map.entry(name).or_default();
            *counter += 1;
            *counter
        };

        (map.len(), counter)
    }

    fn advanced_children_len(&self, walk_type: WalkType) -> usize {
        match walk_type {
            WalkType::File => self.advanced_file_children.len(),
            WalkType::Directory => self.advanced_dir_children.len(),
        }
    }

    fn clear_advanced_children(&mut self, walk_type: WalkType) {
        match walk_type {
            WalkType::File => self.advanced_file_children.clear(),
            WalkType::Directory => self.advanced_dir_children.clear(),
        }
    }

    /// Recursively remove all walks contained within a walk of depth `depth`.
    fn remove_contained(
        &mut self,
        walk_type: WalkType,
        new_walk: &Walk,
        threshold: usize,
        ratio: f64,
    ) {
        // Returns whether a walk exists at depth+1.
        fn inner(
            new_walk: &Walk,
            path: &mut RepoPathBuf,
            node: &mut WalkNode,
            walk_type: WalkType,
            depth: usize,
            top: bool,
            threshold: usize,
            ratio: f64,
        ) -> bool {
            let mut any_child_advanced = false;
            let mut new_advanced_children = Vec::new();
            node.children.retain(|name, child| {
                let mut child_advanced = false;

                path.push(name);

                if child
                    .get_walk_for_type(walk_type)
                    .is_some_and(|w| w.depth >= depth)
                {
                    child_advanced = true;
                } else if let Some(old_walk) = child.set_walk_for_type(walk_type, None) {
                    new_walk.absorb_counters(&old_walk);
                }

                if depth > 0 {
                    if inner(
                        new_walk,
                        path,
                        child,
                        walk_type,
                        depth - 1,
                        false,
                        threshold,
                        ratio,
                    ) {
                        child_advanced = true;
                    }
                }

                if top && child_advanced {
                    // Record if this top-level child has advanced children, meaning a
                    // descendant walk that has pushed to depth+1.
                    new_advanced_children.push(name.to_owned());
                }

                any_child_advanced = any_child_advanced || child_advanced;

                let retain = child.has_walk() && !child.expired()
                    || !child.children.is_empty()
                    // Keep node around if it has total file/dir hints that are likely to be useful.
                    || interesting_metadata(threshold, ratio, child.total_files, child.total_dirs);

                if !retain {
                    tracing::trace!(%path, "dropping node during insert");
                }

                path.pop();

                retain
            });

            for advanced in new_advanced_children {
                tracing::trace!(dir=%path, child=%advanced, "inserting advanced child during removal");
                node.insert_advanced_child(walk_type, advanced);
            }

            any_child_advanced
        }

        inner(
            new_walk,
            &mut RepoPathBuf::new(),
            self,
            walk_type,
            new_walk.depth,
            true,
            threshold,
            ratio,
        );
    }

    /// Reports whether self has a walk and the walk fully contains a descendant walk
    /// rooted at `path` of depth `depth`.
    fn contains(&self, walk_type: WalkType, path: &RepoPath, depth: usize) -> bool {
        self.get_dominating_walk(walk_type)
            .is_some_and(|w| w.depth >= (path.depth() + depth))
    }

    /// Return whether this Dir should be considered "walked".
    pub(crate) fn is_walked(
        &self,
        walk_type: WalkType,
        seen_count: usize,
        mut walk_threshold: usize,
        walk_ratio: f64,
    ) -> bool {
        if seen_count == 0 {
            return false;
        }

        let total_count = match walk_type {
            WalkType::File => self.total_files,
            WalkType::Directory => self.total_dirs,
        };

        // If we have the total size hint, adjust the threshold for extreme cases.
        if let Some(total) = total_count {
            // If dir is too small we know we will never reach the threshold. Adjust threshold down
            // until it is smaller than dir size.
            while walk_threshold > total {
                walk_threshold /= 2;
            }

            // Conversely, if directory is very large we don't want to detect a walk too
            // aggressively. Ensure the threshold is at least `walk_ratio` of the total directory
            // size. For example, a if `walk_ratio` is 0.1 and the directory size is 10_000, we will
            // raise the `walk_threshold` to 1_000.
            if total > 0 && (walk_threshold as f64) / (total as f64) < walk_ratio {
                walk_threshold = ((total as f64) * walk_ratio) as usize;
            }
        }

        seen_count >= walk_threshold
    }

    pub(crate) fn iter(&self, mut cb: impl FnMut(&WalkNode, usize) -> bool) {
        fn inner(node: &WalkNode, cb: &mut impl FnMut(&WalkNode, usize) -> bool, depth: usize) {
            if !cb(node, depth) {
                return;
            }

            for child in node.children.values() {
                inner(child, cb, depth + 1);
            }
        }

        inner(self, &mut cb, 0);
    }

    /// Delete nodes not accessed within timeout.
    /// Returns (nodes_deleted, nodes_remaining, walks_deleted).
    pub(crate) fn gc(&mut self, config: &Config) -> (usize, usize, usize) {
        // Return (nodes_deleted, nodes_remaining, walks_deleted, keep_me)
        fn inner(
            config: &Config,
            path: &mut RepoPathBuf,
            node: &mut WalkNode,
        ) -> (usize, usize, usize, bool) {
            let mut walks_removed = 0;
            let mut deleted = 0;
            let mut retained = 0;

            node.children.retain(|name, child| {
                path.push(name);

                let (d, r, w, keep) = inner(config, path, child);

                deleted += d;
                retained += r;
                walks_removed += w;

                if !keep {
                    tracing::trace!(%path, has_walk=child.has_walk(), "GCing node");
                }

                path.pop();

                keep
            });

            let expired = node.expired();

            let important_metadata = important_metadata(
                config.walk_threshold,
                config.walk_ratio,
                node.total_files,
                node.total_dirs,
            );
            let keep_me = !expired || !node.children.is_empty() || important_metadata;
            let has_walk = node.has_walk();

            if expired && has_walk {
                walks_removed += 1;
                node.log_walk_end(path);
            }

            if expired && keep_me {
                tracing::trace!(%path, has_walk, "GCing node with children");
                node.clear_except_children(config);
            }

            if keep_me {
                retained += 1;
            } else {
                deleted += 1;
            }

            (deleted, retained, walks_removed, keep_me)
        }

        let (mut deleted, remaining, mut walks_deleted, keep_me) =
            inner(config, &mut RepoPathBuf::new(), self);
        if !keep_me {
            // We don't actually delete the root node, so take one off.
            deleted -= 1;

            // Log root GC event only if the root node had some activity (and hence had last_access
            // set).
            if self.last_access.load().is_some() {
                tracing::trace!("GCing root node");
            }

            if self.has_walk() {
                walks_deleted += 1;
                self.log_walk_end(RepoPath::empty());
            }

            // At top level we have no parent to remove us, so just unset our fields.
            self.clear_except_children(config);
        }

        (deleted, remaining, walks_deleted)
    }

    fn log_walk_end(&self, root: &RepoPath) {
        if let Some(walk) = &self.file_walk {
            walk.log_end(root);
        }

        if let Some(walk) = &self.dir_walk {
            walk.log_end(root);
        }
    }

    // Clear all fields except children.
    fn clear_except_children(&mut self, config: &Config) {
        self.file_walk.take();
        self.dir_walk.take();
        self.last_access.reset();
        self.advanced_file_children.clear();
        self.advanced_dir_children.clear();
        self.seen_files.clear();

        // Retain important metadata indefinintely.
        if !important_metadata(
            config.walk_threshold,
            config.walk_ratio,
            self.total_files,
            self.total_dirs,
        ) {
            self.total_files.take();
            self.total_dirs.take();
        }
    }

    // NB: does not check if self.expired(), so caller must check if appropriate.
    fn has_walk(&self) -> bool {
        self.file_walk.is_some() || self.dir_walk.is_some()
    }

    #[allow(clippy::useless_conversion)]
    pub(crate) fn expired(&self) -> bool {
        self.last_access
            .load()
            .is_none_or(|accessed| accessed.elapsed() >= self.gc_timeout.into())
    }
}
