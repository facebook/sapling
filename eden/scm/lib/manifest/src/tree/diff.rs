/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::{cmp::Ordering, collections::VecDeque, mem};

use anyhow::Result;

use pathmatcher::{DirectoryMatch, Matcher};
use types::RepoPath;

use crate::{
    tree::{store::InnerStore, DirLink, Tree},
    DiffEntry, File,
};

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
enum Side {
    Left,
    Right,
}

/// A single item to process as part of the diffing process.
///
/// This may either be a single directory that was only present on one
/// side of the diff, or it may be a pair of directories (with the same
/// path) whose content is different on either side of the diff.
#[derive(Debug, Clone, Eq, PartialEq)]
enum DiffItem<'a> {
    Single(DirLink<'a>, Side),
    Changed(DirLink<'a>, DirLink<'a>),
}

impl<'a> DiffItem<'a> {
    fn process(
        self,
        next: &mut VecDeque<DiffItem<'a>>,
        lstore: &'a InnerStore,
        rstore: &'a InnerStore,
        matcher: &'a dyn Matcher,
    ) -> Result<Vec<DiffEntry>> {
        match self {
            DiffItem::Single(dir, side) => {
                let store = match side {
                    Side::Left => lstore,
                    Side::Right => rstore,
                };
                diff_single(dir, next, side, store, matcher)
            }
            DiffItem::Changed(left, right) => diff(left, right, next, lstore, rstore, matcher),
        }
    }

    fn path(&self) -> &RepoPath {
        match self {
            DiffItem::Single(d, _) => &d.path,
            DiffItem::Changed(d, _) => &d.path,
        }
    }

    fn left(dir: DirLink<'a>) -> Self {
        DiffItem::Single(dir, Side::Left)
    }

    fn right(dir: DirLink<'a>) -> Self {
        DiffItem::Single(dir, Side::Right)
    }
}

/// A breadth-first diff iterator over two trees.
///
/// This struct is an iterator that, given two trees, will iterate
/// over the directories layer-by-layer, outputting diff entries
/// for each mismatched file encountered. At the start of each layer
/// of the traversal, all of the modified directories in that layer
/// will be prefetched from the store, thereby reducing the total
/// number of tree fetches required to perform a full-tree diff while
/// only fetching tree nodes that have actually changed.
pub struct Diff<'a> {
    output: VecDeque<DiffEntry>,
    current: VecDeque<DiffItem<'a>>,
    next: VecDeque<DiffItem<'a>>,
    lstore: &'a InnerStore,
    rstore: &'a InnerStore,
    matcher: &'a dyn Matcher,
}

impl<'a> Diff<'a> {
    pub fn new(left: &'a Tree, right: &'a Tree, matcher: &'a dyn Matcher) -> Self {
        let lroot = DirLink::from_root(&left.root).expect("tree root is not a directory");
        let rroot = DirLink::from_root(&right.root).expect("tree root is not a directory");
        let mut current = VecDeque::new();

        // Don't even attempt to perform a diff if these trees are the same.
        if lroot.hgid != rroot.hgid || lroot.hgid.is_none() {
            current.push_back(DiffItem::Changed(lroot, rroot));
        }

        Diff {
            output: VecDeque::new(),
            current,
            next: VecDeque::new(),
            lstore: &left.store,
            rstore: &right.store,
            matcher,
        }
    }

    /// Prefetch the contents of the directories in the next layer of the traversal.
    ///
    /// Given that each tree owns its own store, we need to perform two prefetches
    /// to ensure that the keys for each tree are correctly prefetched from the
    /// corresponding store.
    fn prefetch(&self) -> Result<()> {
        let mut lkeys = Vec::new();
        let mut rkeys = Vec::new();

        // Group the keys in the next layer by which tree
        // they came from so that we can prefetch using
        // the correct store for each tree.
        for item in &self.next {
            match item {
                DiffItem::Single(dir, side) => {
                    match side {
                        Side::Left => dir.key().map(|key| lkeys.push(key)),
                        Side::Right => dir.key().map(|key| rkeys.push(key)),
                    };
                }
                DiffItem::Changed(left, right) => {
                    left.key().map(|key| lkeys.push(key));
                    right.key().map(|key| rkeys.push(key));
                }
            }
        }

        if !lkeys.is_empty() {
            self.lstore.prefetch(lkeys)?;
        }
        if !rkeys.is_empty() {
            self.rstore.prefetch(rkeys)?;
        }

        Ok(())
    }

    /// Process the next `DiffItem` for this layer (either a pair of modified directories
    /// or an added/removed directory), potentially generating new `DiffEntry`s for
    /// any changed files contained therein.
    ///
    /// If this method reaches the end of the current layer of the breadth-first traversal,
    /// it will perform I/O to prefetch the next layer of directories before continuing. As
    /// such, this function will occassionally block for an extended period of time.
    ///
    /// Returns `true` if there are more items to process after the current one. Once this
    /// method returns `false`, the traversal is complete.
    fn process_next_item(&mut self) -> Result<bool> {
        if self.current.is_empty() {
            self.prefetch()?;
            mem::swap(&mut self.current, &mut self.next);
        }

        let entries = match self.current.pop_front() {
            Some(item) => item.process(&mut self.next, &self.lstore, &self.rstore, self.matcher)?,
            None => return Ok(false),
        };

        self.output.extend(entries);
        Ok(true)
    }
}

impl<'a> Iterator for Diff<'a> {
    type Item = Result<DiffEntry>;

    fn next(&mut self) -> Option<Self::Item> {
        let span = tracing::debug_span!("tree::diff::next", path = "");
        let _scope = span.enter();
        while self.output.is_empty() {
            match self.process_next_item() {
                Ok(true) => continue,
                Ok(false) => break,
                Err(e) => return Some(Err(e)),
            }
        }
        let result = self.output.pop_front();
        if !span.is_disabled() {
            if let Some(ref result) = result {
                span.record("path", &result.path.as_repo_path().as_str());
            }
        }
        result.map(Ok)
    }
}

/// Process a directory that is only present on one side of the diff.
///
/// Returns diff entries of all of the files in this directory, and
/// adds any subdirectories to the next layer to be processed.
fn diff_single<'a>(
    dir: DirLink<'a>,
    next: &mut VecDeque<DiffItem<'a>>,
    side: Side,
    store: &'a InnerStore,
    matcher: &'a dyn Matcher,
) -> Result<Vec<DiffEntry>> {
    let (files, dirs) = dir.list(store)?;

    let items = dirs
        .into_iter()
        .filter(|d| matcher.matches_directory(&d.path) != DirectoryMatch::Nothing)
        .map(|d| DiffItem::Single(d, side));
    next.extend(items);

    let entries = files
        .into_iter()
        .filter(|f| matcher.matches_file(&f.path))
        .map(|f| match side {
            Side::Left => DiffEntry::left(f),
            Side::Right => DiffEntry::right(f),
        })
        .collect();

    Ok(entries)
}

/// Diff two directories.
///
/// The directories should correspond to the same path on either side of the
/// diff. Returns diff entries for any changed files, and adds any changed
/// directories to the next layer to be processed.
fn diff<'a>(
    left: DirLink<'a>,
    right: DirLink<'a>,
    next: &mut VecDeque<DiffItem<'a>>,
    lstore: &'a InnerStore,
    rstore: &'a InnerStore,
    matcher: &'a dyn Matcher,
) -> Result<Vec<DiffEntry>> {
    let (lfiles, ldirs) = left.list(lstore)?;
    let (rfiles, rdirs) = right.list(rstore)?;
    next.extend(diff_dirs(ldirs, rdirs, matcher));
    Ok(diff_files(lfiles, rfiles, matcher))
}

/// Given two sorted file lists, return diff entries for non-matching files.
fn diff_files<'a>(
    lfiles: Vec<File>,
    rfiles: Vec<File>,
    matcher: &'a dyn Matcher,
) -> Vec<DiffEntry> {
    let mut output = Vec::new();

    let mut add_to_output = |entry: DiffEntry| {
        if matcher.matches_file(&entry.path) {
            output.push(entry);
        }
    };

    debug_assert!(is_sorted(&lfiles));
    debug_assert!(is_sorted(&rfiles));

    let mut lfiles = lfiles.into_iter();
    let mut rfiles = rfiles.into_iter();
    let mut lfile = lfiles.next();
    let mut rfile = rfiles.next();

    loop {
        match (lfile, rfile) {
            (Some(l), Some(r)) => match l.path.cmp(&r.path) {
                Ordering::Less => {
                    add_to_output(DiffEntry::left(l));
                    lfile = lfiles.next();
                    rfile = Some(r);
                }
                Ordering::Greater => {
                    add_to_output(DiffEntry::right(r));
                    lfile = Some(l);
                    rfile = rfiles.next();
                }
                Ordering::Equal => {
                    if l.meta != r.meta {
                        add_to_output(DiffEntry::changed(l, r));
                    }
                    lfile = lfiles.next();
                    rfile = rfiles.next();
                }
            },
            (Some(l), None) => {
                add_to_output(DiffEntry::left(l));
                lfile = lfiles.next();
                rfile = None;
            }
            (None, Some(r)) => {
                add_to_output(DiffEntry::right(r));
                lfile = None;
                rfile = rfiles.next();
            }
            (None, None) => break,
        }
    }

    output
}

/// Given two sorted directory lists, return diff items for non-matching directories.
fn diff_dirs<'a>(
    ldirs: Vec<DirLink<'a>>,
    rdirs: Vec<DirLink<'a>>,
    matcher: &'a dyn Matcher,
) -> Vec<DiffItem<'a>> {
    let mut output = Vec::new();

    let mut add_to_output = |item: DiffItem<'a>| {
        if matcher.matches_directory(item.path()) != DirectoryMatch::Nothing {
            output.push(item);
        }
    };

    debug_assert!(is_sorted(&ldirs));
    debug_assert!(is_sorted(&rdirs));

    let mut ldirs = ldirs.into_iter();
    let mut rdirs = rdirs.into_iter();
    let mut ldir = ldirs.next();
    let mut rdir = rdirs.next();

    loop {
        match (ldir, rdir) {
            (Some(l), Some(r)) => match l.path.cmp(&r.path) {
                Ordering::Less => {
                    add_to_output(DiffItem::left(l));
                    ldir = ldirs.next();
                    rdir = Some(r);
                }
                Ordering::Greater => {
                    add_to_output(DiffItem::right(r));
                    ldir = Some(l);
                    rdir = rdirs.next();
                }
                Ordering::Equal => {
                    // We only need to diff the directories if their hashes don't match.
                    // The exception is if both hashes are None (indicating the trees
                    // have not yet been persisted), in which case we must manually compare
                    // all of the entries since we can't tell if they are the same.
                    if l.hgid != r.hgid || l.hgid.is_none() {
                        add_to_output(DiffItem::Changed(l, r));
                    }
                    ldir = ldirs.next();
                    rdir = rdirs.next();
                }
            },
            (Some(l), None) => {
                add_to_output(DiffItem::left(l));
                ldir = ldirs.next();
                rdir = None;
            }
            (None, Some(r)) => {
                add_to_output(DiffItem::right(r));
                ldir = None;
                rdir = rdirs.next();
            }
            (None, None) => break,
        }
    }

    output
}

fn is_sorted<T: Ord>(iter: impl IntoIterator<Item = T>) -> bool {
    let mut iter = iter.into_iter();
    if let Some(mut prev) = iter.next() {
        for i in iter {
            if i < prev {
                return false;
            }
            prev = i;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::Arc;

    use pathmatcher::{AlwaysMatcher, TreeMatcher};
    use types::testutil::*;

    use crate::{
        tree::{link::DirLink, store::TestStore, testutil::*, Link},
        DiffType, FileMetadata, FileType, Manifest,
    };

    #[test]
    fn test_diff_entry_from_file() {
        let path = repo_path_buf("foo/bar");
        let meta = make_meta("a");
        let file = File {
            path: path.clone(),
            meta: meta.clone(),
        };

        let left = DiffEntry::left(file.clone());
        let expected = DiffEntry::new(path.clone(), DiffType::LeftOnly(meta.clone()));
        assert_eq!(left, expected);

        let right = DiffEntry::right(file.clone());
        let expected = DiffEntry::new(path.clone(), DiffType::RightOnly(meta.clone()));
        assert_eq!(right, expected);

        let meta2 = make_meta("b");
        let file2 = File {
            path: path.clone(),
            meta: meta2.clone(),
        };

        let changed = DiffEntry::changed(file, file2);
        let expected = DiffEntry::new(path, DiffType::Changed(meta, meta2));
        assert_eq!(changed, expected);
    }

    #[test]
    fn test_diff_single() {
        let tree = make_tree(&[("a", "1"), ("b/f", "2"), ("c", "3"), ("d/f", "4")]);
        let dir = DirLink::from_root(&tree.root).unwrap();
        let mut next = VecDeque::new();

        let matcher = AlwaysMatcher::new();
        let entries = diff_single(dir, &mut next, Side::Left, &tree.store, &matcher).unwrap();

        let expected_entries = vec![
            DiffEntry::new(
                repo_path_buf("a"),
                DiffType::LeftOnly(FileMetadata {
                    hgid: hgid("1"),
                    file_type: FileType::Regular,
                }),
            ),
            DiffEntry::new(
                repo_path_buf("c"),
                DiffType::LeftOnly(FileMetadata {
                    hgid: hgid("3"),
                    file_type: FileType::Regular,
                }),
            ),
        ];
        assert_eq!(entries, expected_entries);

        let dummy = Link::ephemeral();
        let expected_next = VecDeque::from(vec![
            DiffItem::Single(
                DirLink::from_link(&dummy, repo_path_buf("b")).unwrap(),
                Side::Left,
            ),
            DiffItem::Single(
                DirLink::from_link(&dummy, repo_path_buf("d")).unwrap(),
                Side::Left,
            ),
        ]);

        assert_eq!(next, expected_next);
    }

    #[test]
    fn test_diff_files() {
        let lfiles = vec![
            make_file("a", "1"),
            make_file("b", "2"),
            make_file("c", "3"),
            make_file("e", "4"),
        ];
        let rfiles = vec![
            make_file("a", "1"),
            make_file("c", "3"),
            make_file("d", "5"),
            make_file("e", "6"),
        ];

        let matcher = AlwaysMatcher::new();
        let entries = diff_files(lfiles, rfiles, &matcher);
        let expected = vec![
            DiffEntry::new(
                repo_path_buf("b"),
                DiffType::LeftOnly(FileMetadata {
                    hgid: hgid("2"),
                    file_type: FileType::Regular,
                }),
            ),
            DiffEntry::new(
                repo_path_buf("d"),
                DiffType::RightOnly(FileMetadata {
                    hgid: hgid("5"),
                    file_type: FileType::Regular,
                }),
            ),
            DiffEntry::new(
                repo_path_buf("e"),
                DiffType::Changed(
                    FileMetadata {
                        hgid: hgid("4"),
                        file_type: FileType::Regular,
                    },
                    FileMetadata {
                        hgid: hgid("6"),
                        file_type: FileType::Regular,
                    },
                ),
            ),
        ];

        assert_eq!(entries, expected);
    }

    #[test]
    fn test_diff() {
        let ltree = make_tree(&[
            ("changed", "1"),
            ("d1/changed", "1"),
            ("d1/leftonly", "1"),
            ("d1/same", "1"),
            ("d2/changed", "1"),
            ("d2/leftonly", "1"),
            ("d2/same", "1"),
            ("leftonly", "1"),
            ("same", "1"),
        ]);
        let rtree = make_tree(&[
            ("changed", "2"),
            ("d1/changed", "2"),
            ("d1/rightonly", "1"),
            ("d1/same", "1"),
            ("d2/changed", "2"),
            ("d2/rightonly", "1"),
            ("d2/same", "1"),
            ("rightonly", "1"),
            ("same", "1"),
        ]);

        let matcher = AlwaysMatcher::new();
        let diff = Diff::new(&ltree, &rtree, &matcher);
        let entries = diff
            .collect::<Result<Vec<_>>>()
            .unwrap()
            .into_iter()
            .map(|entry| entry.path)
            .collect::<Vec<_>>();

        let expected = vec![
            repo_path_buf("changed"),
            repo_path_buf("leftonly"),
            repo_path_buf("rightonly"),
            repo_path_buf("d1/changed"),
            repo_path_buf("d1/leftonly"),
            repo_path_buf("d1/rightonly"),
            repo_path_buf("d2/changed"),
            repo_path_buf("d2/leftonly"),
            repo_path_buf("d2/rightonly"),
        ];
        assert_eq!(entries, expected);
    }

    #[test]
    fn test_diff_matcher() {
        let ltree = make_tree(&[
            ("changed", "1"),
            ("d1/changed", "1"),
            ("d1/leftonly", "1"),
            ("d1/same", "1"),
            ("d2/changed", "1"),
            ("d2/leftonly", "1"),
            ("d2/same", "1"),
            ("leftonly", "1"),
            ("same", "1"),
        ]);
        let rtree = make_tree(&[
            ("changed", "2"),
            ("d1/changed", "2"),
            ("d1/rightonly", "1"),
            ("d1/same", "1"),
            ("d2/changed", "2"),
            ("d2/rightonly", "1"),
            ("d2/same", "1"),
            ("rightonly", "1"),
            ("same", "1"),
        ]);

        let matcher = TreeMatcher::from_rules(["d1/**"].iter()).unwrap();
        let diff = Diff::new(&ltree, &rtree, &matcher);
        let entries = diff
            .collect::<Result<Vec<_>>>()
            .unwrap()
            .into_iter()
            .map(|entry| entry.path)
            .collect::<Vec<_>>();

        let expected = vec![
            repo_path_buf("d1/changed"),
            repo_path_buf("d1/leftonly"),
            repo_path_buf("d1/rightonly"),
        ];
        assert_eq!(entries, expected);
    }

    #[test]
    fn test_diff_generic() {
        let mut left = make_tree(&[("a1/b1/c1/d1", "10"), ("a1/b2", "20"), ("a3/b1", "40")]);
        let mut right = make_tree(&[("a1/b2", "40"), ("a2/b2/c2", "30"), ("a3/b1", "40")]);

        assert_eq!(
            Diff::new(&left, &right, &AlwaysMatcher::new())
                .collect::<Result<Vec<_>>>()
                .unwrap(),
            vec!(
                DiffEntry::new(
                    repo_path_buf("a1/b2"),
                    DiffType::Changed(make_meta("20"), make_meta("40"))
                ),
                DiffEntry::new(
                    repo_path_buf("a2/b2/c2"),
                    DiffType::RightOnly(make_meta("30"))
                ),
                DiffEntry::new(
                    repo_path_buf("a1/b1/c1/d1"),
                    DiffType::LeftOnly(make_meta("10"))
                ),
            )
        );

        left.flush().unwrap();
        right.flush().unwrap();

        assert_eq!(
            Diff::new(&left, &right, &AlwaysMatcher::new())
                .collect::<Result<Vec<_>>>()
                .unwrap(),
            vec!(
                DiffEntry::new(
                    repo_path_buf("a1/b2"),
                    DiffType::Changed(make_meta("20"), make_meta("40"))
                ),
                DiffEntry::new(
                    repo_path_buf("a2/b2/c2"),
                    DiffType::RightOnly(make_meta("30"))
                ),
                DiffEntry::new(
                    repo_path_buf("a1/b1/c1/d1"),
                    DiffType::LeftOnly(make_meta("10"))
                ),
            )
        );
        right
            .insert(repo_path_buf("a1/b1/c1/d1"), make_meta("10"))
            .unwrap();
        left.insert(repo_path_buf("a1/b2"), make_meta("40"))
            .unwrap();
        left.insert(repo_path_buf("a2/b2/c2"), make_meta("30"))
            .unwrap();

        assert!(Diff::new(&left, &right, &AlwaysMatcher::new())
            .next()
            .is_none());
    }

    #[test]
    fn test_diff_does_not_evaluate_durable_on_hgid_equality() {
        // Leaving the store empty intentionaly so that we get a panic if anything is read from it.
        let left = Tree::durable(Arc::new(TestStore::new()), hgid("10"));
        let right = Tree::durable(Arc::new(TestStore::new()), hgid("10"));
        assert!(Diff::new(&left, &right, &AlwaysMatcher::new())
            .next()
            .is_none());

        let right = Tree::durable(Arc::new(TestStore::new()), hgid("20"));
        assert!(Diff::new(&left, &right, &AlwaysMatcher::new())
            .next()
            .unwrap()
            .is_err());
    }

    #[test]
    fn test_diff_one_file_one_directory() {
        let mut left = Tree::ephemeral(Arc::new(TestStore::new()));
        left.insert(repo_path_buf("a1/b1"), make_meta("10"))
            .unwrap();
        left.insert(repo_path_buf("a2"), make_meta("20")).unwrap();

        let mut right = Tree::ephemeral(Arc::new(TestStore::new()));
        right.insert(repo_path_buf("a1"), make_meta("30")).unwrap();
        right
            .insert(repo_path_buf("a2/b2"), make_meta("40"))
            .unwrap();

        assert_eq!(
            Diff::new(&left, &right, &AlwaysMatcher::new())
                .collect::<Result<Vec<_>>>()
                .unwrap(),
            vec!(
                DiffEntry::new(repo_path_buf("a1"), DiffType::RightOnly(make_meta("30"))),
                DiffEntry::new(repo_path_buf("a2"), DiffType::LeftOnly(make_meta("20"))),
                DiffEntry::new(repo_path_buf("a1/b1"), DiffType::LeftOnly(make_meta("10"))),
                DiffEntry::new(repo_path_buf("a2/b2"), DiffType::RightOnly(make_meta("40"))),
            )
        );
    }

    #[test]
    fn test_diff_left_empty() {
        let mut left = Tree::ephemeral(Arc::new(TestStore::new()));
        let mut right = make_tree(&[("a1/b1/c1/d1", "10"), ("a1/b2", "20"), ("a2/b2/c2", "30")]);

        assert_eq!(
            Diff::new(&left, &right, &AlwaysMatcher::new())
                .collect::<Result<Vec<_>>>()
                .unwrap(),
            vec!(
                DiffEntry::new(repo_path_buf("a1/b2"), DiffType::RightOnly(make_meta("20"))),
                DiffEntry::new(
                    repo_path_buf("a2/b2/c2"),
                    DiffType::RightOnly(make_meta("30"))
                ),
                DiffEntry::new(
                    repo_path_buf("a1/b1/c1/d1"),
                    DiffType::RightOnly(make_meta("10"))
                ),
            )
        );

        left.flush().unwrap();
        right.flush().unwrap();

        assert_eq!(
            Diff::new(&left, &right, &AlwaysMatcher::new())
                .collect::<Result<Vec<_>>>()
                .unwrap(),
            vec!(
                DiffEntry::new(repo_path_buf("a1/b2"), DiffType::RightOnly(make_meta("20"))),
                DiffEntry::new(
                    repo_path_buf("a2/b2/c2"),
                    DiffType::RightOnly(make_meta("30"))
                ),
                DiffEntry::new(
                    repo_path_buf("a1/b1/c1/d1"),
                    DiffType::RightOnly(make_meta("10"))
                ),
            )
        );
    }

    #[test]
    fn test_diff_matcher_2() {
        let left = make_tree(&[("a1/b1/c1/d1", "10"), ("a1/b2", "20"), ("a3/b1", "40")]);
        let right = make_tree(&[("a1/b2", "40"), ("a2/b2/c2", "30"), ("a3/b1", "40")]);

        assert_eq!(
            Diff::new(
                &left,
                &right,
                &TreeMatcher::from_rules(["a1/b1/**"].iter()).unwrap()
            )
            .collect::<Result<Vec<_>>>()
            .unwrap(),
            vec!(DiffEntry::new(
                repo_path_buf("a1/b1/c1/d1"),
                DiffType::LeftOnly(make_meta("10"))
            ),)
        );
        assert_eq!(
            Diff::new(
                &left,
                &right,
                &TreeMatcher::from_rules(["a1/b2"].iter()).unwrap()
            )
            .collect::<Result<Vec<_>>>()
            .unwrap(),
            vec!(DiffEntry::new(
                repo_path_buf("a1/b2"),
                DiffType::Changed(make_meta("20"), make_meta("40"))
            ),)
        );
        assert_eq!(
            Diff::new(
                &left,
                &right,
                &TreeMatcher::from_rules(["a2/b2/**"].iter()).unwrap()
            )
            .collect::<Result<Vec<_>>>()
            .unwrap(),
            vec!(DiffEntry::new(
                repo_path_buf("a2/b2/c2"),
                DiffType::RightOnly(make_meta("30"))
            ),)
        );
        assert_eq!(
            Diff::new(
                &left,
                &right,
                &TreeMatcher::from_rules(["*/b2/**"].iter()).unwrap()
            )
            .collect::<Result<Vec<_>>>()
            .unwrap(),
            vec!(
                DiffEntry::new(
                    repo_path_buf("a1/b2"),
                    DiffType::Changed(make_meta("20"), make_meta("40"))
                ),
                DiffEntry::new(
                    repo_path_buf("a2/b2/c2"),
                    DiffType::RightOnly(make_meta("30"))
                ),
            )
        );
        assert!(Diff::new(
            &left,
            &right,
            &TreeMatcher::from_rules(["a3/**"].iter()).unwrap()
        )
        .next()
        .is_none());
    }

    #[test]
    fn test_diff_on_sort_order_edge() {
        let left = make_tree(&[("foo/bar-test/a.txt", "10"), ("foo/bartest/b.txt", "20")]);
        let mut right = left.clone();
        right
            .insert(repo_path_buf("foo/bar/c.txt"), make_meta("30"))
            .unwrap();

        assert_eq!(
            Diff::new(&left, &right, &AlwaysMatcher::new())
                .collect::<Result<Vec<_>>>()
                .unwrap(),
            vec![DiffEntry::new(
                repo_path_buf("foo/bar/c.txt"),
                DiffType::RightOnly(make_meta("30"))
            ),],
        );
    }
}
