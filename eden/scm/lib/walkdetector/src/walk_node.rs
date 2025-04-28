/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::HashMap;
use std::collections::HashSet;

use types::PathComponentBuf;
use types::RepoPath;
use types::RepoPathBuf;

use crate::Walk;
use crate::interesting_metadata;

/// Tree structure to track active walks. This makes it efficient to find a file's
/// "containing" walk, and to efficiently discover a walk's siblings, cousins, etc. in
/// order to merge walks.
#[derive(Default)]
pub(crate) struct WalkNode {
    pub(crate) walk: Option<Walk>,
    pub(crate) children: HashMap<PathComponentBuf, WalkNode>,

    // Child directories that have a walked descendant "advanced" past our current
    // walk.depth.
    pub(crate) advanced_children: HashSet<PathComponentBuf>,

    // Total file count in this directory (if hint available).
    pub(crate) total_files: Option<usize>,
    // Total directory count in this directory (if hint available).
    pub(crate) total_dirs: Option<usize>,
    // File names seen so far (only used before transitioning to walk).
    pub(crate) seen_files: HashSet<PathComponentBuf>,
}

impl WalkNode {
    /// Fetch active walk for `walk_root`, if any.
    pub(crate) fn get_walk(&mut self, walk_root: &RepoPath) -> Option<&mut Walk> {
        match walk_root.split_first_component() {
            Some((head, tail)) => self
                .children
                .get_mut(head)
                .and_then(|child| child.get_walk(tail)),
            None => self.walk.as_mut(),
        }
    }

    /// Get existing WalkNode entry for specified root, if any.
    pub(crate) fn get_node(&mut self, walk_root: &RepoPath) -> Option<&mut Self> {
        match walk_root.split_first_component() {
            Some((head, tail)) => self
                .children
                .get_mut(head)
                .and_then(|child| child.get_node(tail)),
            None => Some(self),
        }
    }

    /// Find node with active walk covering directory `dir`, if any.
    pub(crate) fn get_containing_node<'a, 'b>(
        &'a mut self,
        dir: &'b RepoPath,
    ) -> Option<(&'a mut Self, &'b RepoPath)> {
        match dir.split_first_component() {
            Some((head, tail)) => {
                if self.contains(dir, 0) {
                    Some((self, dir))
                } else {
                    self.children
                        .get_mut(head)
                        .and_then(|child| child.get_containing_node(tail))
                }
            }
            None => self.walk.map(|_| (self, dir)),
        }
    }

    /// Find node with active walk covering `dir`, or create new node for `dir`. This is a
    /// single step to perform the common get-or-create operation in a single tree
    /// traversal.
    pub(crate) fn get_or_create_owning_node<'a>(
        &'a mut self,
        dir: &'a RepoPath,
    ) -> (&'a mut Self, &'a RepoPath) {
        match dir.split_first_component() {
            Some((head, tail)) => {
                if self.contains(dir, 0) {
                    (self, dir)
                } else if self.children.contains_key(head) {
                    self.children
                        .get_mut(head)
                        .unwrap()
                        .get_or_create_owning_node(tail)
                } else {
                    self.children
                        .entry(head.to_owned())
                        .or_default()
                        .get_or_create_owning_node(tail)
                }
            }
            None => (self, dir),
        }
    }

    /// Insert a new walk. Any redundant/contained walks will be removed. `walk` will not
    /// be inserted if it is contained by an ancestor walk.
    pub(crate) fn insert_walk(&mut self, walk_root: &RepoPath, mut walk: Walk, threshold: usize) {
        // If we completely overlap with the walk to be inserted, skip it. This shouldn't
        // happen, but I want to guarantee there are no overlapping walks.
        if self.contains(walk_root, walk.depth) {
            return;
        }

        match walk_root.split_first_component() {
            Some((head, tail)) => {
                if let Some(child) = self.children.get_mut(head) {
                    child.insert_walk(tail, walk, threshold);
                } else {
                    let mut child = WalkNode::default();
                    child.insert_walk(tail, walk, threshold);
                    if self.children.insert(head.to_owned(), child).is_some() {
                        tracing::warn!(name=%head, "WalkNode entry already existed");
                    }
                }
            }
            None => {
                self.walk = Some(walk);
                self.advanced_children.clear();
                self.remove_contained(walk.depth, threshold);

                if self.advanced_children.len() >= threshold {
                    walk.depth += 1;
                    self.insert_walk(walk_root, walk, threshold);
                }
            }
        }
    }

    pub(crate) fn insert_metadata(
        &mut self,
        walk_root: &RepoPath,
        total_files: usize,
        total_dirs: usize,
    ) {
        match walk_root.split_first_component() {
            Some((head, tail)) => {
                if let Some(child) = self.children.get_mut(head) {
                    child.insert_metadata(tail, total_files, total_dirs);
                } else {
                    let mut child = WalkNode::default();
                    child.insert_metadata(tail, total_files, total_dirs);
                    self.children.insert(head.to_owned(), child);
                }
            }
            None => {
                self.total_files = Some(total_files);
                self.total_dirs = Some(total_dirs);
            }
        }
    }

    /// List all active walks.
    pub(crate) fn list_walks(&self) -> Vec<(RepoPathBuf, Walk)> {
        fn inner(node: &WalkNode, path: RepoPathBuf, list: &mut Vec<(RepoPathBuf, Walk)>) {
            if let Some(walk) = &node.walk {
                list.push((path.clone(), walk.clone()));
            }

            for (name, child) in node.children.iter() {
                inner(child, path.join(name.as_path_component()), list);
            }
        }

        let mut list = Vec::new();
        inner(self, RepoPathBuf::new(), &mut list);
        list
    }

    pub(crate) fn child_walks(&self) -> impl Iterator<Item = (&PathComponentBuf, &Walk)> {
        self.children
            .iter()
            .filter_map(|(name, node)| node.walk.as_ref().map(|w| (name, w)))
    }

    /// Recursively remove all walks contained within a walk of depth `depth`.
    fn remove_contained(&mut self, depth: usize, threshold: usize) {
        // Returns whether a walk exists at depth+1.
        fn inner(node: &mut WalkNode, depth: usize, top: bool, threshold: usize) -> bool {
            let mut any_child_advanced = false;

            node.children.retain(|name, child| {
                let mut child_advanced = false;

                if child.walk.as_ref().is_some_and(|w| w.depth >= depth) {
                    child_advanced = true;
                } else {
                    child.walk = None;
                }

                if depth > 0 {
                    if inner(child, depth - 1, false, threshold) {
                        child_advanced = true;
                    }
                }

                if top && child_advanced {
                    // Record if this top-level child has advanced children, meaning a
                    // descendant walk that has pushed to depth+1.
                    tracing::trace!(%name, "inserting advanced child during removal");
                    node.advanced_children.insert(name.to_owned());
                }

                any_child_advanced = any_child_advanced || child_advanced;

                child.walk.is_some()
                    || !child.children.is_empty()
                    // Keep node around if it has total file/dir hints that are likely to be useful.
                    || interesting_metadata(threshold, child.total_files, child.total_dirs)
            });

            any_child_advanced
        }

        inner(self, depth, true, threshold);
    }

    /// Reports whether self has a walk and the walk fully contains a descendant walk
    /// rooted at `path` of depth `depth`.
    fn contains(&self, path: &RepoPath, depth: usize) -> bool {
        self.walk
            .is_some_and(|w| w.depth >= (path.components().count() + depth))
    }

    /// Return whether this Dir should be considered "walked".
    pub(crate) fn is_walked(&self, dir_walk_threshold: usize) -> bool {
        self.seen_files.len() >= dir_walk_threshold
            || self
                .total_files
                .is_some_and(|total| total < dir_walk_threshold)
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
}
