/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::{
    cmp::Ordering,
    collections::VecDeque,
    sync::mpsc::{channel, Receiver, Sender},
    sync::Arc,
    thread::JoinHandle,
    time::Duration,
};

use anyhow::Result;

use manifest::{DiffEntry, File};
use pathmatcher::{DirectoryMatch, Matcher};
use progress_model::ProgressBar;
use types::RepoPath;

use crate::{store::InnerStore, DirLink, TreeManifest};

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
enum DiffItem {
    Single(DirLink, Side),
    Changed(DirLink, DirLink),
}

impl DiffItem {
    fn process(
        self,
        fetcher: &mut Sender<DiffItem>,
        store: &InnerStore,
        matcher: &dyn Matcher,
        pending: &mut u64,
    ) -> Result<Vec<DiffEntry>> {
        match self {
            DiffItem::Single(dir, side) => diff_single(dir, fetcher, side, store, matcher, pending),
            DiffItem::Changed(left, right) => diff(left, right, fetcher, store, matcher, pending),
        }
    }

    fn path(&self) -> &RepoPath {
        match self {
            DiffItem::Single(d, _) => &d.path,
            DiffItem::Changed(d, _) => &d.path,
        }
    }

    fn left(dir: DirLink) -> Self {
        DiffItem::Single(dir, Side::Left)
    }

    fn right(dir: DirLink) -> Self {
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
    store: &'a InnerStore,
    matcher: &'a dyn Matcher,
    progress_bar: Option<&'a Arc<ProgressBar>>,
    #[allow(dead_code)]
    fetch_thread: JoinHandle<()>,
    sender: Sender<DiffItem>,
    receiver: Receiver<DiffItem>,
    pending: u64,
}

impl<'a> Diff<'a> {
    pub fn new(
        left: &'a TreeManifest,
        right: &'a TreeManifest,
        matcher: &'a dyn Matcher,
    ) -> Result<Self> {
        let lroot = DirLink::from_root(&left.root).expect("tree root is not a directory");
        let rroot = DirLink::from_root(&right.root).expect("tree root is not a directory");

        let (send_prefetch, receive_prefetch) = channel();
        let (send_done, receive_done) = channel();
        let mut pending = 0;

        let store = left.store.clone();
        let fetch_thread =
            std::thread::spawn(move || prefetch_thread(receive_prefetch, send_done, store));

        // Don't even attempt to perform a diff if these trees are the same.
        if lroot.hgid() != rroot.hgid() || lroot.hgid().is_none() {
            pending += 1;
            send_prefetch.send(DiffItem::Changed(lroot, rroot))?;
        }

        Ok(Diff {
            output: VecDeque::new(),
            store: &left.store,
            matcher,
            progress_bar: None,
            fetch_thread,
            sender: send_prefetch,
            receiver: receive_done,
            pending,
        })
    }

    pub fn attach_progress_bar(&mut self, bar: &'a Arc<ProgressBar>) {
        self.progress_bar = Some(bar);
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
        if self.pending == 0 {
            return Ok(false);
        }

        let item = self.receiver.recv()?;
        self.pending -= 1;

        let entries = item.process(
            &mut self.sender,
            &self.store,
            self.matcher,
            &mut self.pending,
        )?;
        self.output.extend(entries);

        if let Some(bar) = self.progress_bar {
            // Increase "depth" by one as we descend to next BFS level.
            bar.increase_position(1);
        }

        Ok(self.pending != 0)
    }
}

fn prefetch_thread<'a>(receiver: Receiver<DiffItem>, sender: Sender<DiffItem>, store: InnerStore) {
    let limit = 100000;
    let timeout = Duration::from_millis(1);
    let mut received = Vec::with_capacity(limit);
    'outer: loop {
        // Wait for a prefetch request
        match receiver.recv() {
            Ok(request) => received.push(request),
            Err(_) => break,
        };

        // Grab a bunch of them at once.
        loop {
            use std::sync::mpsc::RecvTimeoutError::*;
            match receiver.recv_timeout(timeout) {
                Ok(request) => received.push(request),
                Err(Timeout) => break,
                Err(Disconnected) => {
                    break 'outer;
                }
            };
            if received.len() >= limit {
                break;
            }
        }

        // Prefetch them
        let mut keys = Vec::with_capacity(received.len());
        for item in received.iter() {
            match item {
                DiffItem::Single(dir, side) => {
                    match side {
                        Side::Left => dir.key().map(|key| keys.push(key)),
                        Side::Right => dir.key().map(|key| keys.push(key)),
                    };
                }
                DiffItem::Changed(left, right) => {
                    left.key().map(|key| keys.push(key));
                    right.key().map(|key| keys.push(key));
                }
            }
        }

        if !keys.is_empty() {
            let _ = store.prefetch(keys);
        }

        // Notify that we finished
        for item in received.drain(..) {
            if sender.send(item).is_err() {
                break 'outer;
            }
        }
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
fn diff_single(
    dir: DirLink,
    fetcher: &mut Sender<DiffItem>,
    side: Side,
    store: &InnerStore,
    matcher: &dyn Matcher,
    pending: &mut u64,
) -> Result<Vec<DiffEntry>> {
    let (files, dirs) = dir.list(store)?;

    for d in dirs.into_iter() {
        if matcher.matches_directory(&d.path)? != DirectoryMatch::Nothing {
            *pending += 1;
            fetcher.send(DiffItem::Single(d, side))?;
        }
    }
    let mut entries = Vec::new();
    for f in files.into_iter() {
        if matcher.matches_file(&f.path)? {
            let entry = match side {
                Side::Left => DiffEntry::left(f),
                Side::Right => DiffEntry::right(f),
            };
            entries.push(entry);
        }
    }
    Ok(entries)
}

/// Diff two directories.
///
/// The directories should correspond to the same path on either side of the
/// diff. Returns diff entries for any changed files, and adds any changed
/// directories to the next layer to be processed.
fn diff(
    left: DirLink,
    right: DirLink,
    fetcher: &mut Sender<DiffItem>,
    store: &InnerStore,
    matcher: &dyn Matcher,
    pending: &mut u64,
) -> Result<Vec<DiffEntry>> {
    let (lfiles, ldirs) = left.list(store)?;
    let (rfiles, rdirs) = right.list(store)?;
    for item in diff_dirs(ldirs, rdirs, matcher)? {
        *pending += 1;
        fetcher.send(item)?;
    }
    diff_files(lfiles, rfiles, matcher)
}

/// Given two sorted file lists, return diff entries for non-matching files.
fn diff_files(
    lfiles: Vec<File>,
    rfiles: Vec<File>,
    matcher: &dyn Matcher,
) -> Result<Vec<DiffEntry>> {
    let mut output = Vec::new();

    let mut add_to_output = |entry: DiffEntry| -> Result<()> {
        if matcher.matches_file(&entry.path)? {
            output.push(entry);
        }
        Ok(())
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
                    add_to_output(DiffEntry::left(l))?;
                    lfile = lfiles.next();
                    rfile = Some(r);
                }
                Ordering::Greater => {
                    add_to_output(DiffEntry::right(r))?;
                    lfile = Some(l);
                    rfile = rfiles.next();
                }
                Ordering::Equal => {
                    if l.meta != r.meta {
                        add_to_output(DiffEntry::changed(l, r))?;
                    }
                    lfile = lfiles.next();
                    rfile = rfiles.next();
                }
            },
            (Some(l), None) => {
                add_to_output(DiffEntry::left(l))?;
                lfile = lfiles.next();
                rfile = None;
            }
            (None, Some(r)) => {
                add_to_output(DiffEntry::right(r))?;
                lfile = None;
                rfile = rfiles.next();
            }
            (None, None) => break,
        }
    }

    Ok(output)
}

/// Given two sorted directory lists, return diff items for non-matching directories.
fn diff_dirs(
    ldirs: Vec<DirLink>,
    rdirs: Vec<DirLink>,
    matcher: &dyn Matcher,
) -> Result<Vec<DiffItem>> {
    let mut output = Vec::new();

    let mut add_to_output = |item: DiffItem| -> Result<()> {
        if matcher.matches_directory(item.path())? != DirectoryMatch::Nothing {
            output.push(item);
        }
        Ok(())
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
                    add_to_output(DiffItem::left(l))?;
                    ldir = ldirs.next();
                    rdir = Some(r);
                }
                Ordering::Greater => {
                    add_to_output(DiffItem::right(r))?;
                    ldir = Some(l);
                    rdir = rdirs.next();
                }
                Ordering::Equal => {
                    // We only need to diff the directories if their hashes don't match.
                    // The exception is if both hashes are None (indicating the trees
                    // have not yet been persisted), in which case we must manually compare
                    // all of the entries since we can't tell if they are the same.
                    if l.hgid() != r.hgid() || l.hgid().is_none() {
                        add_to_output(DiffItem::Changed(l, r))?;
                    }
                    ldir = ldirs.next();
                    rdir = rdirs.next();
                }
            },
            (Some(l), None) => {
                add_to_output(DiffItem::left(l))?;
                ldir = ldirs.next();
                rdir = None;
            }
            (None, Some(r)) => {
                add_to_output(DiffItem::right(r))?;
                ldir = None;
                rdir = rdirs.next();
            }
            (None, None) => break,
        }
    }

    Ok(output)
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

    use manifest::{testutil::*, DiffType, FileMetadata, FileType, Manifest};
    use pathmatcher::{AlwaysMatcher, TreeMatcher};
    use types::testutil::*;

    use crate::{link::DirLink, testutil::*, Link};

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
        let store = Arc::new(TestStore::new());
        let tree = make_tree_manifest(store, &[("a", "1"), ("b/f", "2"), ("c", "3"), ("d/f", "4")]);
        let dir = DirLink::from_root(&tree.root).unwrap();
        let (mut sender, receiver) = channel();
        let mut pending = 0;

        let matcher = AlwaysMatcher::new();
        let entries = diff_single(
            dir,
            &mut sender,
            Side::Left,
            &tree.store,
            &matcher,
            &mut pending,
        )
        .unwrap();

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
        let next = vec![receiver.recv().unwrap(), receiver.recv().unwrap()];
        let expected_next = vec![
            DiffItem::Single(
                DirLink::from_link(&dummy, repo_path_buf("b")).unwrap(),
                Side::Left,
            ),
            DiffItem::Single(
                DirLink::from_link(&dummy, repo_path_buf("d")).unwrap(),
                Side::Left,
            ),
        ];

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
        let entries = diff_files(lfiles, rfiles, &matcher).unwrap();
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
        let store = Arc::new(TestStore::new());
        let ltree = make_tree_manifest(
            store.clone(),
            &[
                ("changed", "1"),
                ("d1/changed", "1"),
                ("d1/leftonly", "1"),
                ("d1/same", "1"),
                ("d2/changed", "1"),
                ("d2/leftonly", "1"),
                ("d2/same", "1"),
                ("leftonly", "1"),
                ("same", "1"),
            ],
        );
        let rtree = make_tree_manifest(
            store,
            &[
                ("changed", "2"),
                ("d1/changed", "2"),
                ("d1/rightonly", "1"),
                ("d1/same", "1"),
                ("d2/changed", "2"),
                ("d2/rightonly", "1"),
                ("d2/same", "1"),
                ("rightonly", "1"),
                ("same", "1"),
            ],
        );

        let matcher = AlwaysMatcher::new();
        let diff = Diff::new(&ltree, &rtree, &matcher).unwrap();
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
        let store = Arc::new(TestStore::new());
        let ltree = make_tree_manifest(
            store.clone(),
            &[
                ("changed", "1"),
                ("d1/changed", "1"),
                ("d1/leftonly", "1"),
                ("d1/same", "1"),
                ("d2/changed", "1"),
                ("d2/leftonly", "1"),
                ("d2/same", "1"),
                ("leftonly", "1"),
                ("same", "1"),
            ],
        );
        let rtree = make_tree_manifest(
            store,
            &[
                ("changed", "2"),
                ("d1/changed", "2"),
                ("d1/rightonly", "1"),
                ("d1/same", "1"),
                ("d2/changed", "2"),
                ("d2/rightonly", "1"),
                ("d2/same", "1"),
                ("rightonly", "1"),
                ("same", "1"),
            ],
        );

        let matcher = TreeMatcher::from_rules(["d1/**"].iter()).unwrap();
        let diff = Diff::new(&ltree, &rtree, &matcher).unwrap();
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
        let store = Arc::new(TestStore::new());
        let mut left = make_tree_manifest(
            store.clone(),
            &[("a1/b1/c1/d1", "10"), ("a1/b2", "20"), ("a3/b1", "40")],
        );
        let mut right = make_tree_manifest(
            store,
            &[("a1/b2", "40"), ("a2/b2/c2", "30"), ("a3/b1", "40")],
        );

        assert_eq!(
            Diff::new(&left, &right, &AlwaysMatcher::new())
                .unwrap()
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
                .unwrap()
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

        assert!(
            Diff::new(&left, &right, &AlwaysMatcher::new())
                .unwrap()
                .next()
                .is_none()
        );
    }

    #[test]
    fn test_diff_does_not_evaluate_durable_on_hgid_equality() {
        // Leaving the store empty intentionaly so that we get a panic if anything is read from it.
        let left = TreeManifest::durable(Arc::new(TestStore::new()), hgid("10"));
        let right = TreeManifest::durable(Arc::new(TestStore::new()), hgid("10"));
        assert!(
            Diff::new(&left, &right, &AlwaysMatcher::new())
                .unwrap()
                .next()
                .is_none()
        );

        let right = TreeManifest::durable(Arc::new(TestStore::new()), hgid("20"));
        assert!(
            Diff::new(&left, &right, &AlwaysMatcher::new())
                .unwrap()
                .next()
                .unwrap()
                .is_err()
        );
    }

    #[test]
    fn test_diff_one_file_one_directory() {
        let mut left = TreeManifest::ephemeral(Arc::new(TestStore::new()));
        left.insert(repo_path_buf("a1/b1"), make_meta("10"))
            .unwrap();
        left.insert(repo_path_buf("a2"), make_meta("20")).unwrap();

        let mut right = TreeManifest::ephemeral(Arc::new(TestStore::new()));
        right.insert(repo_path_buf("a1"), make_meta("30")).unwrap();
        right
            .insert(repo_path_buf("a2/b2"), make_meta("40"))
            .unwrap();

        assert_eq!(
            Diff::new(&left, &right, &AlwaysMatcher::new())
                .unwrap()
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
        let store = Arc::new(TestStore::new());
        let mut left = TreeManifest::ephemeral(store.clone());
        let mut right = make_tree_manifest(
            store,
            &[("a1/b1/c1/d1", "10"), ("a1/b2", "20"), ("a2/b2/c2", "30")],
        );

        assert_eq!(
            Diff::new(&left, &right, &AlwaysMatcher::new())
                .unwrap()
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
                .unwrap()
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
        let store = Arc::new(TestStore::new());
        let left = make_tree_manifest(
            store.clone(),
            &[("a1/b1/c1/d1", "10"), ("a1/b2", "20"), ("a3/b1", "40")],
        );
        let right = make_tree_manifest(
            store,
            &[("a1/b2", "40"), ("a2/b2/c2", "30"), ("a3/b1", "40")],
        );

        assert_eq!(
            Diff::new(
                &left,
                &right,
                &TreeMatcher::from_rules(["a1/b1/**"].iter()).unwrap()
            )
            .unwrap()
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
            .unwrap()
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
            .unwrap()
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
            .unwrap()
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
        assert!(
            Diff::new(
                &left,
                &right,
                &TreeMatcher::from_rules(["a3/**"].iter()).unwrap()
            )
            .unwrap()
            .next()
            .is_none()
        );
    }

    #[test]
    fn test_diff_on_sort_order_edge() {
        let store = Arc::new(TestStore::new());
        let left = make_tree_manifest(
            store,
            &[("foo/bar-test/a.txt", "10"), ("foo/bartest/b.txt", "20")],
        );
        let mut right = left.clone();
        right
            .insert(repo_path_buf("foo/bar/c.txt"), make_meta("30"))
            .unwrap();

        assert_eq!(
            Diff::new(&left, &right, &AlwaysMatcher::new())
                .unwrap()
                .collect::<Result<Vec<_>>>()
                .unwrap(),
            vec![DiffEntry::new(
                repo_path_buf("foo/bar/c.txt"),
                DiffType::RightOnly(make_meta("30"))
            ),],
        );
    }
}
