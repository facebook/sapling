// Copyright 2019 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::{cmp::Ordering, collections::VecDeque, mem};

use failure::Fallible;

use pathmatcher::{DirectoryMatch, Matcher};
use types::{Key, Node, RepoPath, RepoPathBuf};

use crate::{
    tree::{link::Link, store::InnerStore, DiffEntry, DiffType, Tree},
    FileMetadata,
};

/// A file (leaf node) encountered during a tree traversal.
///
/// Consists of the full path to the file along with the associated file metadata.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
struct File {
    path: RepoPathBuf,
    meta: FileMetadata,
}

impl File {
    fn new(path: RepoPathBuf, meta: FileMetadata) -> Self {
        Self { path, meta }
    }

    /// Create a file record for a `Link`, failing if the link
    /// refers to a directory rather than a file.
    fn from_link(link: &Link, path: RepoPathBuf) -> Option<Self> {
        match link {
            Link::Leaf(meta) => Some(Self::new(path, *meta)),
            _ => None,
        }
    }

    fn into_left(self) -> DiffEntry {
        DiffEntry::new(self.path, DiffType::LeftOnly(self.meta))
    }

    fn into_right(self) -> DiffEntry {
        DiffEntry::new(self.path, DiffType::RightOnly(self.meta))
    }

    fn into_changed(self, other: File) -> DiffEntry {
        DiffEntry::new(self.path, DiffType::Changed(self.meta, other.meta))
    }
}

/// A directory (inner node) encountered during a tree traversal.
///
/// The directory may have a manifest node hash if it is unmodified from its
/// state on disk. If the directory has in-memory modifications that have not
/// been persisted to disk, it will not have a node hash.
#[derive(Clone, Debug)]
struct Directory<'a> {
    path: RepoPathBuf,
    node: Option<Node>,
    link: &'a Link,
}

impl<'a> Directory<'a> {
    /// Create a directory record for a `Link`, failing if the link
    /// refers to a file rather than a directory.
    fn from_link(link: &'a Link, path: RepoPathBuf) -> Option<Self> {
        let node = match link {
            Link::Leaf(_) => return None,
            Link::Ephemeral(_) => None,
            Link::Durable(entry) => Some(entry.node),
        };
        Some(Self { path, node, link })
    }

    /// Same as `from_link`, but set the directory's path to the empty
    /// path, making this method only useful for the root of the tree.
    fn from_root(link: &'a Link) -> Option<Self> {
        Self::from_link(link, RepoPathBuf::new())
    }

    /// List the contents of this directory.
    ///
    /// Returns two sorted vectors of files and directories contained
    /// in this directory.
    ///
    /// This operation may perform I/O to load the tree entry from the store
    /// if it is not already in memory. Depending on the store implementation,
    /// this may involve an expensive network request if the required data is
    /// not available locally. As such, algorithms that require fast access to
    /// this data should take care to ensure that this content is present
    /// locally before calling this method.
    fn list(&self, store: &InnerStore) -> Fallible<(Vec<File>, Vec<Directory<'a>>)> {
        let mut files = Vec::new();
        let mut dirs = Vec::new();

        let links = match &self.link {
            &Link::Leaf(_) => panic!("programming error: directory cannot be a leaf node"),
            &Link::Ephemeral(ref links) => links,
            &Link::Durable(entry) => entry.get_links(store, &self.path)?,
        };

        for (name, link) in links {
            let mut path = self.path.clone();
            path.push(name.as_ref());
            match link {
                Link::Leaf(_) => {
                    files.push(File::from_link(link, path).expect("leaf node must be a valid file"))
                }
                Link::Ephemeral(_) | Link::Durable(_) => dirs.push(
                    Directory::from_link(link, path).expect("inner node must be a valid directory"),
                ),
            }
        }

        Ok((files, dirs))
    }

    /// Create a `Key` (path/node pair) corresponding to this directory. Keys are used
    /// by the Eden API to fetch data from the server, making this representation useful
    /// for interacting with Mercurial's data fetching code.
    fn key(&self) -> Option<Key> {
        Some(Key::new(self.path.clone(), self.node.clone()?))
    }
}

impl Eq for Directory<'_> {}

impl PartialEq for Directory<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.path == other.path && self.node == other.node
    }
}

impl Ord for Directory<'_> {
    fn cmp(&self, other: &Self) -> Ordering {
        match self.path.cmp(&other.path) {
            Ordering::Equal => self.node.cmp(&other.node),
            ord => ord,
        }
    }
}

impl PartialOrd for Directory<'_> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

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
    Single(Directory<'a>, Side),
    Changed(Directory<'a>, Directory<'a>),
}

impl<'a> DiffItem<'a> {
    fn process(
        self,
        next: &mut VecDeque<DiffItem<'a>>,
        lstore: &'a InnerStore,
        rstore: &'a InnerStore,
        matcher: &'a dyn Matcher,
    ) -> Fallible<Vec<DiffEntry>> {
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

    fn left(dir: Directory<'a>) -> Self {
        DiffItem::Single(dir, Side::Left)
    }

    fn right(dir: Directory<'a>) -> Self {
        DiffItem::Single(dir, Side::Right)
    }
}

/// Process a directory that is only present on one side of the diff.
///
/// Returns diff entries of all of the files in this directory, and
/// adds any subdirectories to the next layer to be processed.
fn diff_single<'a>(
    dir: Directory<'a>,
    next: &mut VecDeque<DiffItem<'a>>,
    side: Side,
    store: &'a InnerStore,
    matcher: &'a dyn Matcher,
) -> Fallible<Vec<DiffEntry>> {
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
            Side::Left => f.into_left(),
            Side::Right => f.into_right(),
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
    left: Directory<'a>,
    right: Directory<'a>,
    next: &mut VecDeque<DiffItem<'a>>,
    lstore: &'a InnerStore,
    rstore: &'a InnerStore,
    matcher: &'a dyn Matcher,
) -> Fallible<Vec<DiffEntry>> {
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
                    add_to_output(l.into_left());
                    lfile = lfiles.next();
                    rfile = Some(r);
                }
                Ordering::Greater => {
                    add_to_output(r.into_right());
                    lfile = Some(l);
                    rfile = rfiles.next();
                }
                Ordering::Equal => {
                    if l.meta != r.meta {
                        add_to_output(l.into_changed(r));
                    }
                    lfile = lfiles.next();
                    rfile = rfiles.next();
                }
            },
            (Some(l), None) => {
                add_to_output(l.into_left());
                lfile = lfiles.next();
                rfile = None;
            }
            (None, Some(r)) => {
                add_to_output(r.into_right());
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
    ldirs: Vec<Directory<'a>>,
    rdirs: Vec<Directory<'a>>,
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
                    if l.node != r.node || l.node.is_none() {
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

/// A breadth-first diff iterator over two trees.
///
/// This struct is an iterator that, given two trees, will iterate
/// over the directories layer-by-layer, outputting diff entries
/// for each mismatched file encountered. At the start of each layer
/// of the traversal, all of the modified directories in that layer
/// will be prefetched from the store, thereby reducing the total
/// number of tree fetches required to perform a full-tree diff while
/// only fetching tree nodes that have actually changed.
pub struct BfsDiff<'a> {
    output: VecDeque<DiffEntry>,
    current: VecDeque<DiffItem<'a>>,
    next: VecDeque<DiffItem<'a>>,
    lstore: &'a InnerStore,
    rstore: &'a InnerStore,
    matcher: &'a dyn Matcher,
}

impl<'a> BfsDiff<'a> {
    pub fn new(left: &'a Tree, right: &'a Tree, matcher: &'a dyn Matcher) -> Self {
        let lroot = Directory::from_root(&left.root).expect("tree root is not a directory");
        let rroot = Directory::from_root(&right.root).expect("tree root is not a directory");
        let mut current = VecDeque::new();

        // Don't even attempt to perform a diff if these trees are the same.
        if lroot.node != rroot.node || lroot.node.is_none() {
            current.push_back(DiffItem::Changed(lroot, rroot));
        }

        BfsDiff {
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
    fn prefetch(&self) -> Fallible<()> {
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
    fn process_next_item(&mut self) -> Fallible<bool> {
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

impl<'a> Iterator for BfsDiff<'a> {
    type Item = Fallible<DiffEntry>;

    fn next(&mut self) -> Option<Self::Item> {
        while self.output.is_empty() {
            match self.process_next_item() {
                Ok(true) => continue,
                Ok(false) => break,
                Err(e) => return Some(Err(e)),
            }
        }
        self.output.pop_front().map(Ok)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::Arc;

    use pathmatcher::{AlwaysMatcher, TreeMatcher};
    use types::testutil::*;

    use crate::{tree::store::TestStore, FileType, Manifest};

    fn make_meta(hex: &str) -> FileMetadata {
        FileMetadata::regular(node(hex))
    }

    fn make_file(path: &str, hex: &str) -> File {
        File {
            path: repo_path_buf(path),
            meta: make_meta(hex),
        }
    }

    fn make_dir<'a>(path: &str, hex: Option<&str>, link: &'a Link) -> Directory<'a> {
        Directory {
            path: repo_path_buf(path),
            node: hex.map(node),
            link,
        }
    }

    fn make_tree<'a>(paths: impl IntoIterator<Item = &'a (&'a str, &'a str)>) -> Tree {
        let mut tree = Tree::ephemeral(Arc::new(TestStore::new()));
        for (path, filenode) in paths {
            tree.insert(repo_path_buf(path), make_meta(filenode))
                .unwrap();
        }
        tree
    }

    #[test]
    fn test_file_from_link() {
        // Leaf link should result in a file.
        let meta = make_meta("a");
        let path = repo_path_buf("test/leaf");

        let leaf = Link::Leaf(meta.clone());
        let file = File::from_link(&leaf, path.clone()).unwrap();

        let expected = File {
            path: path.clone(),
            meta,
        };
        assert_eq!(file, expected);

        // Attempting to use a directory link should fail.
        let ephemeral = Link::ephemeral();
        let _file = File::from_link(&ephemeral, path.clone());;

        // Durable link should result in a directory.
        let durable = Link::durable(node("a"));
        let file = File::from_link(&durable, path.clone());;
        assert!(file.is_none());
    }

    #[test]
    fn test_diff_entry_from_file() {
        let path = repo_path_buf("foo/bar");
        let meta = make_meta("a");
        let file = File {
            path: path.clone(),
            meta: meta.clone(),
        };

        let left = file.clone().into_left();
        let expected = DiffEntry::new(path.clone(), DiffType::LeftOnly(meta.clone()));
        assert_eq!(left, expected);

        let right = file.clone().into_right();
        let expected = DiffEntry::new(path.clone(), DiffType::RightOnly(meta.clone()));
        assert_eq!(right, expected);

        let meta2 = make_meta("b");
        let file2 = File {
            path: path.clone(),
            meta: meta2.clone(),
        };

        let changed = file.into_changed(file2);
        let expected = DiffEntry::new(path, DiffType::Changed(meta, meta2));
        assert_eq!(changed, expected);
    }

    #[test]
    fn test_directory_from_link() {
        let meta = make_meta("a");
        let path = repo_path_buf("test/leaf");

        let ephemeral = Link::ephemeral();
        let dir = Directory::from_link(&ephemeral, path.clone()).unwrap();
        let expected = Directory {
            path: path.clone(),
            node: None,
            link: &ephemeral,
        };
        assert_eq!(dir, expected);

        let hash = node("b");
        let durable = Link::durable(hash);
        let dir = Directory::from_link(&durable, path.clone()).unwrap();
        let expected = Directory {
            path: path.clone(),
            node: Some(hash),
            link: &ephemeral,
        };
        assert_eq!(dir, expected);

        // If the Link is actually a file, we should get None.
        let leaf = Link::Leaf(meta.clone());
        let dir = Directory::from_link(&leaf, path.clone());
        assert!(dir.is_none());
    }

    #[test]
    fn test_list_directory() -> Fallible<()> {
        let tree = make_tree(&[("a", "1"), ("b/f", "2"), ("c", "3"), ("d/f", "4")]);
        let dir = Directory::from_root(&tree.root).unwrap();
        let (files, dirs) = dir.list(&tree.store)?;

        let file_names = files.into_iter().map(|f| f.path).collect::<Vec<_>>();
        let dir_names = dirs.into_iter().map(|d| d.path).collect::<Vec<_>>();

        assert_eq!(file_names, vec![repo_path_buf("a"), repo_path_buf("c")]);
        assert_eq!(dir_names, vec![repo_path_buf("b"), repo_path_buf("d")]);

        Ok(())
    }

    #[test]
    fn test_diff_single() {
        let tree = make_tree(&[("a", "1"), ("b/f", "2"), ("c", "3"), ("d/f", "4")]);
        let dir = Directory::from_root(&tree.root).unwrap();
        let mut next = VecDeque::new();

        let matcher = AlwaysMatcher::new();
        let entries = diff_single(dir, &mut next, Side::Left, &tree.store, &matcher).unwrap();

        let expected_entries = vec![
            DiffEntry::new(
                repo_path_buf("a"),
                DiffType::LeftOnly(FileMetadata {
                    node: node("1"),
                    file_type: FileType::Regular,
                }),
            ),
            DiffEntry::new(
                repo_path_buf("c"),
                DiffType::LeftOnly(FileMetadata {
                    node: node("3"),
                    file_type: FileType::Regular,
                }),
            ),
        ];
        assert_eq!(entries, expected_entries);

        let dummy = Link::ephemeral();
        let expected_next = VecDeque::from(vec![
            DiffItem::Single(make_dir("b", None, &dummy), Side::Left),
            DiffItem::Single(make_dir("d", None, &dummy), Side::Left),
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
                    node: node("2"),
                    file_type: FileType::Regular,
                }),
            ),
            DiffEntry::new(
                repo_path_buf("d"),
                DiffType::RightOnly(FileMetadata {
                    node: node("5"),
                    file_type: FileType::Regular,
                }),
            ),
            DiffEntry::new(
                repo_path_buf("e"),
                DiffType::Changed(
                    FileMetadata {
                        node: node("4"),
                        file_type: FileType::Regular,
                    },
                    FileMetadata {
                        node: node("6"),
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
        let diff = BfsDiff::new(&ltree, &rtree, &matcher);
        let entries = diff
            .collect::<Fallible<Vec<_>>>()
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

        let matcher = TreeMatcher::from_rules(["d1"].iter());
        let diff = BfsDiff::new(&ltree, &rtree, &matcher);
        let entries = diff
            .collect::<Fallible<Vec<_>>>()
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
}
