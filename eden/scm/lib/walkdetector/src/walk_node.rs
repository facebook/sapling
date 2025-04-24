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
    walk: Option<Walk>,
    children: HashMap<PathComponentBuf, WalkNode>,
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

    /// Insert a new walk.
    pub(crate) fn insert(&mut self, walk_root: &RepoPath, walk: Walk) {
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
            None => self.walk = Some(walk),
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
}
