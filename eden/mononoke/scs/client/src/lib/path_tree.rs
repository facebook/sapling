/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;
use std::iter::Extend;

#[derive(Debug)]
pub(crate) enum PathItem {
    // Requested item.  Either a file, or a whole directory tree.
    Target,

    // Requested item, but only if it is a directory.
    TargetDir,

    // Directory with requested items inside.
    Dir(PathTree),
}

#[derive(Default, Debug)]
pub(crate) struct PathTree {
    elems: BTreeMap<String, PathItem>,
}

impl PathTree {
    pub fn new() -> Self {
        PathTree {
            elems: BTreeMap::new(),
        }
    }

    pub fn insert(&mut self, path: &str) {
        if let Some((elem, rest)) = path.split_once('/') {
            if rest.is_empty() {
                // Path ending in `/` - include the target if it is a
                // directory.
                match self.elems.get_mut(elem) {
                    Some(PathItem::Target) | Some(PathItem::TargetDir) => {
                        // This target is already requested.
                    }
                    Some(item @ PathItem::Dir(_)) => {
                        // Requesting both a path and something under that path
                        // is the same as requesting just the path.  Upgrade this
                        // directory to a full target.
                        *item = PathItem::TargetDir;
                    }
                    None => {
                        self.elems.insert(elem.to_string(), PathItem::TargetDir);
                    }
                }
            } else {
                // Path with some more elements to come.
                match self.elems.get_mut(elem) {
                    Some(PathItem::Target) | Some(PathItem::TargetDir) => {
                        // Requesting both a path and something under that path
                        // is the same as requesting just the path.
                    }
                    Some(PathItem::Dir(tree)) => tree.insert(rest),
                    None => {
                        let mut tree = PathTree::new();
                        tree.insert(rest);
                        self.elems.insert(elem.to_string(), PathItem::Dir(tree));
                    }
                }
            }
        } else {
            match self.elems.get_mut(path) {
                Some(PathItem::Target) => {
                    // This target is already requested.
                }
                Some(item @ (PathItem::Dir(_) | PathItem::TargetDir)) => {
                    // Requesting both a path and something under that path
                    // is the same as requesting just the path.  Upgrade this
                    // directory to a full target.
                    *item = PathItem::Target;
                }
                None => {
                    self.elems.insert(path.to_string(), PathItem::Target);
                }
            }
        }
    }

    pub fn remove(&mut self, path: &str) -> Option<PathItem> {
        self.elems.remove(path)
    }
}

impl<'a> Extend<&'a str> for PathTree {
    fn extend<T>(&mut self, items: T)
    where
        T: IntoIterator<Item = &'a str>,
    {
        for item in items {
            self.insert(item);
        }
    }
}

impl Extend<String> for PathTree {
    fn extend<T>(&mut self, items: T)
    where
        T: IntoIterator<Item = String>,
    {
        for item in items {
            self.insert(&item);
        }
    }
}
