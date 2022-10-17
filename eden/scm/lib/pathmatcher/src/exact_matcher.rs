/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Pattern matcher that matches an exact set of paths.

use std::borrow::Cow;
use std::collections::HashMap;

use anyhow::Result;
use types::RepoPath;

use crate::DirectoryMatch;
use crate::Matcher;

/// A [Matcher] that only matches an exact list of file paths.
#[derive(Clone, Debug)]
pub struct ExactMatcher {
    root: Node,
}

impl ExactMatcher {
    /// Create [ExactMatcher] using an exact list of file paths.
    ///
    /// The matcher will only match files explicitly listed.
    pub fn new(paths: impl Iterator<Item = impl AsRef<RepoPath>>, case_sensitive: bool) -> Self {
        let mut root = Node::new(case_sensitive);
        for path in paths {
            root.insert(path.as_ref());
        }
        ExactMatcher { root }
    }

    /// Insert a new path into the set of paths matched.
    pub fn add(&mut self, path: &RepoPath) {
        self.root.insert(path);
    }
}

impl Matcher for ExactMatcher {
    fn matches_directory(&self, path: &RepoPath) -> Result<DirectoryMatch> {
        match self.root.find(path) {
            Some(node) if !node.children.is_empty() => Ok(DirectoryMatch::ShouldTraverse),
            _ => Ok(DirectoryMatch::Nothing),
        }
    }

    fn matches_file(&self, path: &RepoPath) -> Result<bool> {
        match self.root.find(path) {
            Some(node) => Ok(node.is_file),
            None => Ok(false),
        }
    }
}

#[derive(Clone, Debug)]
struct Node {
    /// Child nodes (for directories).
    children: HashMap<String, Node>,

    /// Whether this node represents a specific file.
    is_file: bool,

    /// True if it should do case insensitive comparisons.
    case_sensitive: bool,
}

impl Node {
    fn new(case_sensitive: bool) -> Self {
        Node {
            children: HashMap::new(),
            is_file: false,
            case_sensitive,
        }
    }

    /// Find the node corresponding to the given path (rooted at this directory),
    /// or [`None`] if there is no node.
    fn find(&self, path: &RepoPath) -> Option<&Node> {
        let mut node = self;
        let mut components = path.components();
        while let Some(component) = components.next() {
            let component: Cow<str> = if node.case_sensitive {
                Cow::Borrowed(component.as_str())
            } else {
                Cow::Owned(component.as_str().to_lowercase())
            };
            node = node.children.get(component.as_ref())?;
        }
        Some(node)
    }

    /// Insert the given path (rooted at this directory) as a file.
    fn insert(&mut self, path: &RepoPath) {
        let mut node = self;

        let mut components = path.components().peekable();
        while let Some(component) = components.next() {
            let component = if node.case_sensitive {
                component.as_str().to_string()
            } else {
                component.as_str().to_lowercase()
            };
            let entry = node.children.entry(component);
            let new_node = entry.or_insert_with(|| Node::new(node.case_sensitive));
            // If this is the final path component, then this component represents a file.
            let is_file = components.peek().is_none();

            if is_file {
                new_node.is_file = true;
                break;
            } else {
                node = new_node;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic() {
        let paths = ["file1", "d1/d2", "d1/d2/file2", "d1/file3", "d2/file4"];
        let paths = paths
            .iter()
            .map(|p| RepoPath::from_str(p).unwrap().to_owned());
        let m = ExactMatcher::new(paths, true);

        // Test regular file matching.
        let cases = [
            ("", false), // empty path shouldn't match
            ("file1", true),
            ("d1/d2/file2", true),
            ("d1/file3", true),
            ("d2/file4", true),
            ("bad_file", false),
            ("bad_dir/f3", false),
            ("d1/bad", false),
            ("d1/d2/bad", false),
            ("d1", false),   // regular directories shouldn't match
            ("d1/d2", true), // directories that are also files should match
            ("d1/d2/file", false),
            ("file", false), // name prefixes shouldn't match
        ];
        for (path, should_match) in cases {
            let matches = m.matches_file(RepoPath::from_str(path).unwrap()).unwrap();
            assert_eq!(should_match, matches, "Matching {:?}", path);
        }

        // Test directory prefix lookups.
        use DirectoryMatch::*;
        let cases = [
            ("", ShouldTraverse),
            ("d1", ShouldTraverse),
            ("d1/d2", ShouldTraverse),
            ("d1/d2/d3", Nothing),
            ("d1/fake2", Nothing),
            ("fake1", Nothing),
        ];
        for (path, expected) in cases {
            let actual = m
                .matches_directory(RepoPath::from_str(path).unwrap())
                .unwrap();
            assert_eq!(expected, actual, "Directory match {:?}", path);
        }
    }

    #[test]
    fn test_case_insensitive() {
        let paths = ["dir/file"];
        let paths = paths
            .iter()
            .map(|p| RepoPath::from_str(p).unwrap().to_owned());
        let case_sensitive = [true, false];
        for sensitive in case_sensitive {
            let m = ExactMatcher::new(paths.clone(), sensitive);
            let cases = ["dir/file", "DIR/file", "dir/FILE", "DIR/FILE"];
            for path in cases {
                let matches = m.matches_file(RepoPath::from_str(path).unwrap()).unwrap();
                let expected = path == "dir/file" || !sensitive;
                assert_eq!(expected, matches, "Matching {:?}", path);
            }
        }
    }
}
