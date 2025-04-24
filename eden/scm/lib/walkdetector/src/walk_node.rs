/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::HashMap;

use types::PathComponentBuf;
use types::RepoPath;
use types::RepoPathBuf;

use crate::Walk;

/// Tree structure to track active walks. This makes it efficient to find a file's
/// "containing" walk, and to efficiently discover a walk's siblings, cousins, etc. in
/// order to merge walks.
#[derive(Default)]
pub(crate) struct WalkNode {
    pub(crate) walk: Option<Walk>,
    pub(crate) children: HashMap<PathComponentBuf, WalkNode>,
}

impl WalkNode {
    /// Fetch active walk for `walk_root`, if any.
    pub(crate) fn get(&mut self, walk_root: &RepoPath) -> Option<&mut Walk> {
        match walk_root.split_first_component() {
            Some((head, tail)) => self
                .children
                .get_mut(head)
                .and_then(|child| child.get(tail)),
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

    /// Find active walk covering directory `dir`, if any.
    pub(crate) fn get_containing(&mut self, dir: &RepoPath) -> Option<&mut Walk> {
        match dir.split_first_component() {
            Some((head, tail)) => {
                if self.contains(dir, 0) {
                    self.walk.as_mut()
                } else {
                    self.children
                        .get_mut(head)
                        .and_then(|child| child.get_containing(tail))
                }
            }
            None => self.walk.as_mut(),
        }
    }

    /// Insert a new walk. Any redundant/contained walks will be removed. `walk` will not
    /// be inserted if it is contained by an ancestor walk.
    pub(crate) fn insert(&mut self, walk_root: &RepoPath, walk: Walk) {
        // If we completely overlap with the walk to be inserted, skip it. This shouldn't
        // happen, but I want to guarantee there are no overlapping walks.
        if self.contains(walk_root, walk.depth) {
            return;
        }

        match walk_root.split_first_component() {
            Some((head, tail)) => {
                if let Some(child) = self.children.get_mut(head) {
                    child.insert(tail, walk);
                } else {
                    let mut child = WalkNode::default();
                    child.insert(tail, walk);
                    if self.children.insert(head.to_owned(), child).is_some() {
                        tracing::warn!(name=%head, "WalkNode entry already existed");
                    }
                }
            }
            None => {
                self.walk = Some(walk);
                self.remove_contained(walk.depth);
            }
        }
    }

    /// List all active walks.
    pub(crate) fn list(&self) -> Vec<(RepoPathBuf, Walk)> {
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
    fn remove_contained(&mut self, depth: usize) {
        self.children.retain(|_name, child| {
            if depth > 0 {
                child.remove_contained(depth - 1);
            }

            if child.walk.as_ref().is_some_and(|w| w.depth < depth) {
                child.walk = None;
            }

            child.walk.is_some() || !child.children.is_empty()
        });
    }

    /// Reports whether self has a walk and the walk fully contains a descendant walk
    /// rooted at `path` of depth `depth`.
    fn contains(&self, path: &RepoPath, depth: usize) -> bool {
        self.walk
            .is_some_and(|w| w.depth >= (path.components().count() + depth))
    }
}
