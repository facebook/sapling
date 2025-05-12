/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::HashMap;
use std::collections::HashSet;
use std::time::Duration;
use std::time::Instant;

use types::PathComponentBuf;
use types::RepoPath;
use types::RepoPathBuf;

use crate::AtomicInstant;
use crate::Walk;
use crate::WalkType;
use crate::interesting_metadata;

/// Tree structure to track active walks. This makes it efficient to find a file's
/// "containing" walk, and to efficiently discover a walk's siblings, cousins, etc. in
/// order to merge walks.
#[derive(Default)]
pub(crate) struct WalkNode {
    // File content walk, if any, rooted at this node.
    pub(crate) file_walk: Option<Walk>,
    // Directory content walk, if any, rooted at this node.
    pub(crate) dir_walk: Option<Walk>,

    pub(crate) last_access: AtomicInstant,
    pub(crate) children: HashMap<PathComponentBuf, WalkNode>,

    // Child directories that have a walked descendant "advanced" past our current
    // walk.depth.
    pub(crate) advanced_file_children: HashSet<PathComponentBuf>,
    pub(crate) advanced_dir_children: HashSet<PathComponentBuf>,

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
                        .get_or_create_owning_node(walk_type, tail)
                } else {
                    self.children
                        .entry(head.to_owned())
                        .or_default()
                        .get_or_create_owning_node(walk_type, tail)
                }
            }
            None => (self, dir),
        }
    }

    /// Find or create node for `dir`.
    pub(crate) fn get_or_create_node<'a>(&'a mut self, dir: &'a RepoPath) -> &'a mut Self {
        match dir.split_first_component() {
            Some((head, tail)) => {
                if self.children.contains_key(head) {
                    self.children
                        .get_mut(head)
                        .unwrap()
                        .get_or_create_node(tail)
                } else {
                    self.children
                        .entry(head.to_owned())
                        .or_default()
                        .get_or_create_node(tail)
                }
            }
            None => self,
        }
    }

    /// Insert a new walk. Any redundant/contained walks will be removed. `walk` will not
    /// be inserted if it is contained by an ancestor walk.
    pub(crate) fn insert_walk(
        &mut self,
        walk_type: WalkType,
        walk_root: &RepoPath,
        mut walk: Walk,
        threshold: usize,
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
                        .insert_walk(walk_type, tail, walk, threshold)
                } else {
                    self.children
                        .entry(head.to_owned())
                        .or_default()
                        .insert_walk(walk_type, tail, walk, threshold)
                }
            }
            None => {
                self.clear_advanced_children(walk_type);

                // This can have a side effect of adding to self.advanced_children.
                self.remove_contained(walk_type, &walk, threshold);

                if self.advanced_children_len(walk_type) >= threshold {
                    walk.depth += 1;
                    self.insert_walk(walk_type, walk_root, walk, threshold)
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
            path: RepoPathBuf,
            list: &mut Vec<(RepoPathBuf, usize)>,
        ) {
            if let Some(walk) = node.get_walk_for_type(walk_type) {
                list.push((path.clone(), walk.depth));
            }

            for (name, child) in node.children.iter() {
                inner(child, walk_type, path.join(name.as_path_component()), list);
            }
        }

        let mut list = Vec::new();
        inner(self, walk_type, RepoPathBuf::new(), &mut list);
        list
    }

    /// Get most "powerful" walk that covers `walk_type`. Basically, a file walk covers a
    /// directory walk, so if walk_type=Directory, we return `self.file_walk ||
    /// self.dir_walk`.
    pub(crate) fn get_dominating_walk(&self, walk_type: WalkType) -> Option<&Walk> {
        match walk_type {
            WalkType::File => self.file_walk.as_ref(),
            WalkType::Directory => self.file_walk.as_ref().or(self.dir_walk.as_ref()),
        }
    }

    pub(crate) fn get_walk_for_type(&self, walk_type: WalkType) -> Option<&Walk> {
        match walk_type {
            WalkType::File => self.file_walk.as_ref(),
            WalkType::Directory => self.dir_walk.as_ref(),
        }
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

    pub(crate) fn insert_advanced_child(
        &mut self,
        walk_type: WalkType,
        name: PathComponentBuf,
    ) -> usize {
        match walk_type {
            WalkType::File => {
                self.advanced_file_children.insert(name);
                self.advanced_file_children.len()
            }
            WalkType::Directory => {
                self.advanced_dir_children.insert(name);
                self.advanced_dir_children.len()
            }
        }
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
    fn remove_contained(&mut self, walk_type: WalkType, new_walk: &Walk, threshold: usize) {
        // Returns whether a walk exists at depth+1.
        fn inner(
            new_walk: &Walk,
            node: &mut WalkNode,
            walk_type: WalkType,
            depth: usize,
            top: bool,
            threshold: usize,
        ) -> bool {
            let mut any_child_advanced = false;
            let mut new_advanced_children = Vec::new();
            node.children.retain(|name, child| {
                let mut child_advanced = false;

                if child
                    .get_walk_for_type(walk_type)
                    .is_some_and(|w| w.depth >= depth)
                {
                    child_advanced = true;
                } else if let Some(old_walk) = child.set_walk_for_type(walk_type, None) {
                    new_walk.absorb_counters(&old_walk);
                }

                if depth > 0 {
                    if inner(new_walk, child, walk_type, depth - 1, false, threshold) {
                        child_advanced = true;
                    }
                }

                if top && child_advanced {
                    // Record if this top-level child has advanced children, meaning a
                    // descendant walk that has pushed to depth+1.
                    tracing::trace!(%name, "inserting advanced child during removal");
                    new_advanced_children.push(name.to_owned());
                }

                any_child_advanced = any_child_advanced || child_advanced;

                child.has_walk()
                    || !child.children.is_empty()
                    // Keep node around if it has total file/dir hints that are likely to be useful.
                    || interesting_metadata(threshold, child.total_files, child.total_dirs)
            });

            for advanced in new_advanced_children {
                node.insert_advanced_child(walk_type, advanced);
            }

            any_child_advanced
        }

        inner(new_walk, self, walk_type, new_walk.depth, true, threshold);
    }

    /// Reports whether self has a walk and the walk fully contains a descendant walk
    /// rooted at `path` of depth `depth`.
    fn contains(&self, walk_type: WalkType, path: &RepoPath, depth: usize) -> bool {
        self.get_dominating_walk(walk_type)
            .is_some_and(|w| w.depth >= (path.components().count() + depth))
    }

    /// Return whether this Dir should be considered "walked".
    pub(crate) fn is_walked(&self, walk_type: WalkType, dir_walk_threshold: usize) -> bool {
        match walk_type {
            WalkType::File => {
                self.seen_files.len() >= dir_walk_threshold
                    || self
                        .total_files
                        .is_some_and(|total| total < dir_walk_threshold)
            }
            WalkType::Directory => {
                self.seen_dirs.len() >= dir_walk_threshold
                    || self
                        .total_dirs
                        .is_some_and(|total| total < dir_walk_threshold)
            }
        }
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
    pub(crate) fn gc(&mut self, timeout: Duration, now: Instant) -> (usize, usize, usize) {
        // Return (nodes_deleted, nodes_remaining, walks_deleted, keep_me)
        fn inner(
            path: &mut RepoPathBuf,
            node: &mut WalkNode,
            timeout: Duration,
            now: Instant,
        ) -> (usize, usize, usize, bool) {
            let mut walks_removed = 0;
            let mut deleted = 0;
            let mut retained = 0;

            node.children.retain(|name, child| {
                path.push(name);

                let (d, r, w, keep) = inner(path, child, timeout, now);

                deleted += d;
                retained += r;
                walks_removed += w;

                if !keep {
                    tracing::trace!(%path, has_walk=child.has_walk(), "GCing node");
                }

                path.pop();

                keep
            });

            let expired = node.expired(now, timeout);

            let keep_me = !expired || !node.children.is_empty();
            let has_walk = node.has_walk();

            if expired && has_walk {
                walks_removed += 1;
                node.log_walk_end(&path);
            }

            if expired && keep_me {
                tracing::trace!(%path, has_walk, "GCing node with children");
                node.clear_except_children();
            }

            if keep_me {
                retained += 1;
            } else {
                deleted += 1;
            }

            (deleted, retained, walks_removed, keep_me)
        }

        let (mut deleted, remaining, mut walks_deleted, keep_me) =
            inner(&mut RepoPathBuf::new(), self, timeout, now);
        if !keep_me {
            // We don't actually delete the root node, so take one off.
            deleted -= 1;

            // At top level we have no parent to remove us, so just unset our fields.
            tracing::trace!("GCing root node");

            if self.has_walk() {
                walks_deleted += 1;
                self.log_walk_end(RepoPath::empty());
            }

            self.clear_except_children();
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
    fn clear_except_children(&mut self) {
        self.file_walk.take();
        self.dir_walk.take();
        self.last_access.reset();
        self.advanced_file_children.clear();
        self.advanced_dir_children.clear();
        self.total_files.take();
        self.total_dirs.take();
        self.seen_files.clear();
    }

    fn has_walk(&self) -> bool {
        self.file_walk.is_some() || self.dir_walk.is_some()
    }

    pub(crate) fn expired(&self, now: Instant, timeout: Duration) -> bool {
        self.last_access
            .load()
            .is_none_or(|accessed| now - accessed >= timeout)
    }
}
