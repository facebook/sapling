/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;
use std::iter::Extend;

#[derive(Default)]
pub(crate) struct PathFilter {
    // None means include everything.
    include: Option<PathTree>,

    // None means don't exclude anything.
    exclude: Option<PathTree>,
}

impl PathFilter {
    pub(crate) fn new(include: Option<PathTree>, exclude: Option<PathTree>) -> Self {
        Self { include, exclude }
    }

    /// Return whether to include file `name`.
    pub(crate) fn matches_file(&mut self, name: &str) -> bool {
        let include = match &mut self.include {
            None => true,
            Some(tree) => match tree.remove(name) {
                None => false,
                Some(PathItem::TargetDir | PathItem::Dir(_)) => false,
                Some(PathItem::Target) => true,
            },
        };
        if !include {
            return false;
        }

        let exclude = match &mut self.exclude {
            None => false,
            Some(tree) => match tree.remove(name) {
                None => false,
                Some(PathItem::TargetDir | PathItem::Dir(_)) => false,
                Some(PathItem::Target) => true,
            },
        };

        !exclude
    }

    /// Return sub-filter relative to `name` if `name` should be included, else None.
    pub(crate) fn matches_dir(&mut self, name: &str) -> Option<Self> {
        let sub_include = match &mut self.include {
            None => None,
            Some(tree) => match tree.remove(name) {
                None => return None,
                Some(PathItem::Target | PathItem::TargetDir) => None,
                Some(PathItem::Dir(sub_tree)) => Some(sub_tree),
            },
        };

        let sub_exclude = match &mut self.exclude {
            None => None,
            Some(tree) => match tree.remove(name) {
                None => None,
                Some(PathItem::Target | PathItem::TargetDir) => return None,
                Some(PathItem::Dir(sub_tree)) => Some(sub_tree),
            },
        };

        Some(Self {
            include: sub_include,
            exclude: sub_exclude,
        })
    }
}

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

#[cfg(test)]
mod test {
    use mononoke_macros::mononoke;

    use super::*;

    #[mononoke::test]
    fn test_include_filter() {
        let mut tree = PathTree::new();
        tree.insert("file");
        tree.insert("dir1");
        tree.insert("dir2/dir3");

        let mut filter = PathFilter::new(Some(tree), None);

        assert!(filter.matches_file("file"));
        assert!(!filter.matches_file("other_file"));

        assert!(filter.matches_dir("other_dir").is_none());

        // Everything under dir/ matches.
        let mut sub_filter = filter.matches_dir("dir1").unwrap();
        assert!(sub_filter.matches_file("anything"));
        assert!(
            sub_filter
                .matches_dir("anything")
                .unwrap()
                .matches_file("anything")
        );

        // Not everything under dir2/ matches.
        let mut sub_filter = filter.matches_dir("dir2").unwrap();
        assert!(!sub_filter.matches_file("anything"));
        assert!(sub_filter.matches_dir("anything").is_none());
        // But we do match anything under dir2/dir3/
        assert!(
            sub_filter
                .matches_dir("dir3")
                .unwrap()
                .matches_file("anything")
        );
    }

    #[mononoke::test]
    fn test_exclude_filter() {
        let mut tree = PathTree::new();
        tree.insert("file");
        tree.insert("dir1");
        tree.insert("dir2/dir3");

        let mut filter = PathFilter::new(None, Some(tree));

        assert!(filter.matches_file("other_file"));
        assert!(!filter.matches_file("file"));

        assert!(
            filter
                .matches_dir("other_dir")
                .unwrap()
                .matches_file("file")
        );

        assert!(filter.matches_dir("dir1").is_none());

        let mut sub_filter = filter.matches_dir("dir2").unwrap();
        assert!(
            sub_filter
                .matches_dir("anything")
                .unwrap()
                .matches_file("anything")
        );
        assert!(sub_filter.matches_dir("dir3").is_none());
    }

    #[mononoke::test]
    fn test_include_and_exclude_filter() {
        let mut include = PathTree::new();
        include.insert("include_file");
        include.insert("include_dir");

        let mut exclude = PathTree::new();
        exclude.insert("exclude_file");
        exclude.insert("include_dir/exclude_file");
        exclude.insert("include_dir/exclude_dir");

        let mut filter = PathFilter::new(Some(include), Some(exclude));

        assert!(filter.matches_file("include_file"));
        assert!(!filter.matches_file("exclude_file"));

        let mut sub_filter = filter.matches_dir("include_dir").unwrap();
        assert!(sub_filter.matches_file("anything"));
        assert!(
            sub_filter
                .matches_dir("anything")
                .unwrap()
                .matches_file("anything")
        );
        assert!(!sub_filter.matches_file("exclude_file"));
        assert!(sub_filter.matches_dir("exclude_dir").is_none());
    }
}
