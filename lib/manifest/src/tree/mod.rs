// Copyright 2019 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

mod cursor;
mod link;
mod store;

use std::{
    cmp::Ordering,
    collections::{btree_map::Entry, BTreeMap},
    fmt,
    sync::Arc,
};

use crypto::{digest::Digest, sha1::Sha1};
use failure::{bail, format_err, Fallible};
use once_cell::sync::OnceCell;

use pathmatcher::{DirectoryMatch, Matcher};
use types::{Node, PathComponent, PathComponentBuf, RepoPath, RepoPathBuf};

use self::cursor::{Cursor, Step};
use self::link::{Durable, DurableEntry, Ephemeral, Leaf, Link};
use self::store::InnerStore;
pub use self::store::TreeStore;
use crate::{FileMetadata, Manifest};

/// The Tree implementation of a Manifest dedicates an inner node for each directory in the
/// repository and a leaf for each file.
#[derive(Clone)]
pub struct Tree {
    store: InnerStore,
    // TODO: root can't be a Leaf
    root: Link,
}

impl Tree {
    /// Instantiates a tree manifest that was stored with the specificed `Node`
    pub fn durable(store: Arc<dyn TreeStore + Send + Sync>, node: Node) -> Self {
        Tree {
            store: InnerStore::new(store),
            root: Link::durable(node),
        }
    }

    /// Instantiates a new tree manifest with no history
    pub fn ephemeral(store: Arc<dyn TreeStore + Send + Sync>) -> Self {
        Tree {
            store: InnerStore::new(store),
            root: Link::Ephemeral(BTreeMap::new()),
        }
    }

    /// Returns an iterator over all the files that are present in the tree.
    pub fn files<'a, M>(&'a self, matcher: &'a M) -> Files<'a, M>
    where
        M: Matcher,
    {
        Files {
            cursor: self.root_cursor(),
            matcher,
        }
    }

    fn root_cursor<'a>(&'a self) -> Cursor<'a> {
        Cursor::new(&self.store, RepoPathBuf::new(), &self.root)
    }
}

impl Manifest for Tree {
    fn get(&self, path: &RepoPath) -> Fallible<Option<FileMetadata>> {
        match self.get_link(path)? {
            None => Ok(None),
            Some(link) => {
                if let &Leaf(file_metadata) = link {
                    Ok(Some(file_metadata))
                } else {
                    Err(format_err!("Encountered directory where file was expected"))
                }
            }
        }
    }

    fn insert(&mut self, path: RepoPathBuf, file_metadata: FileMetadata) -> Fallible<()> {
        let (parent, last_component) = match path.split_last_component() {
            Some(v) => v,
            None => bail!("Cannot insert file metadata for repository root"),
        };
        let mut cursor = &mut self.root;
        for (cursor_parent, component) in parent.parents().zip(parent.components()) {
            // TODO: only convert to ephemeral when a mutation takes place.
            cursor = cursor
                .mut_ephemeral_links(&self.store, cursor_parent)?
                .entry(component.to_owned())
                .or_insert_with(|| Ephemeral(BTreeMap::new()));
        }
        match cursor
            .mut_ephemeral_links(&self.store, parent)?
            .entry(last_component.to_owned())
        {
            Entry::Vacant(entry) => {
                entry.insert(Link::Leaf(file_metadata));
            }
            Entry::Occupied(mut entry) => {
                if let Leaf(ref mut store_ref) = entry.get_mut() {
                    *store_ref = file_metadata;
                } else {
                    bail!("Encountered directory where file was expected");
                }
            }
        }
        Ok(())
    }

    // TODO: return Fallible<Option<FileMetadata>>
    fn remove(&mut self, path: &RepoPath) -> Fallible<()> {
        // The return value lets us know if there are no more files in the subtree and we should be
        // removing it.
        fn do_remove<'a, I>(store: &InnerStore, cursor: &mut Link, iter: &mut I) -> Fallible<bool>
        where
            I: Iterator<Item = (&'a RepoPath, &'a PathComponent)>,
        {
            match iter.next() {
                None => {
                    if let Leaf(_) = cursor {
                        // We reached the file that we want to remove.
                        Ok(true)
                    } else {
                        // TODO: add directory to message.
                        // It turns out that the path we were asked to remove is a directory.
                        Err(format_err!("Asked to remove a directory."))
                    }
                }
                Some((parent, component)) => {
                    // TODO: only convert to ephemeral if a removal took place
                    // We are navigating the tree down following parent directories
                    let ephemeral_links = cursor.mut_ephemeral_links(&store, parent)?;
                    // When there is no `component` subtree we behave like the file was removed.
                    if let Some(link) = ephemeral_links.get_mut(component) {
                        if do_remove(store, link, iter)? {
                            // There are no files in the component subtree so we remove it.
                            ephemeral_links.remove(component);
                        }
                    }
                    Ok(ephemeral_links.is_empty())
                }
            }
        }
        do_remove(
            &self.store,
            &mut self.root,
            &mut path.parents().zip(path.components()),
        )?;
        Ok(())
    }

    fn flush(&mut self) -> Fallible<Node> {
        fn compute_node<C: AsRef<[u8]>>(content: C) -> Node {
            let mut hasher = Sha1::new();
            hasher.input(content.as_ref());
            let mut buf = [0u8; Node::len()];
            hasher.result(&mut buf);
            (&buf).into()
        }
        fn do_flush<'a, 'b, 'c>(
            store: &'a InnerStore,
            pathbuf: &'b mut RepoPathBuf,
            cursor: &'c mut Link,
        ) -> Fallible<(&'c Node, store::Flag)> {
            loop {
                match cursor {
                    Leaf(file_metadata) => {
                        return Ok((
                            &file_metadata.node,
                            store::Flag::File(file_metadata.file_type.clone()),
                        ));
                    }
                    Durable(entry) => return Ok((&entry.node, store::Flag::Directory)),
                    Ephemeral(links) => {
                        let iter = links.iter_mut().map(|(component, link)| {
                            pathbuf.push(component.as_path_component());
                            let (node, flag) = do_flush(store, pathbuf, link)?;
                            pathbuf.pop();
                            Ok(store::Element::new(
                                component.to_owned(),
                                node.clone(),
                                flag,
                            ))
                        });
                        let entry = store::Entry::from_elements(iter)?;
                        let node = compute_node(&entry);
                        store.insert_entry(&pathbuf, node, entry)?;

                        let cell = OnceCell::new();
                        // TODO: remove clone
                        cell.set(Ok(links.clone())).unwrap();

                        let durable_entry = DurableEntry { node, links: cell };
                        *cursor = Durable(Arc::new(durable_entry));
                    }
                }
            }
        }
        let mut path = RepoPathBuf::new();
        let (node, _) = do_flush(&self.store, &mut path, &mut self.root)?;
        Ok(node.clone())
    }
}

impl fmt::Debug for Tree {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fn write_indent(f: &mut fmt::Formatter<'_>, indent: usize) -> fmt::Result {
            write!(f, "{}", str::repeat("| ", indent))?;
            Ok(())
        }
        fn write_children(
            f: &mut fmt::Formatter<'_>,
            children: &BTreeMap<PathComponentBuf, Link>,
            indent: usize,
        ) -> fmt::Result {
            for (component, link) in children {
                write_indent(f, indent)?;
                write!(f, "{} ", component)?;
                write_links(f, link, indent + 1)?;
            }
            Ok(())
        }
        fn write_links(f: &mut fmt::Formatter<'_>, link: &Link, indent: usize) -> fmt::Result {
            match link {
                Link::Leaf(metadata) => {
                    write!(f, "(File, {}, {:?})\n", metadata.node, metadata.file_type)
                }
                Link::Ephemeral(children) => {
                    write!(f, "(Ephemeral)\n")?;
                    write_children(f, children, indent)
                }
                Link::Durable(entry) => {
                    write!(f, "(Durable, {})\n", entry.node)?;
                    match entry.links.get() {
                        None => Ok(()),
                        Some(Err(fallible)) => {
                            write_indent(f, indent)?;
                            write!(f, "failed to load: {:?}", fallible)
                        }
                        Some(Ok(children)) => write_children(f, children, indent),
                    }
                }
            }
        }
        write!(f, "Root ")?;
        write_links(f, &self.root, 1)
    }
}

impl Tree {
    // TODO: return bytes for current or ability to get those bytes
    pub fn finalize(
        &mut self,
        parent_trees: Vec<&Tree>,
    ) -> Fallible<impl Iterator<Item = (RepoPathBuf, Node, Vec<Node>)>> {
        fn compute_node<C: AsRef<[u8]>>(parent_tree_nodes: &[Node], content: C) -> Node {
            let mut hasher = Sha1::new();
            debug_assert!(parent_tree_nodes.len() <= 2);
            let p1 = parent_tree_nodes.get(0).unwrap_or(Node::null_id());
            let p2 = parent_tree_nodes.get(1).unwrap_or(Node::null_id());
            if p1 < p2 {
                hasher.input(p1.as_ref());
                hasher.input(p2.as_ref());
            } else {
                hasher.input(p2.as_ref());
                hasher.input(p1.as_ref());
            }
            hasher.input(content.as_ref());
            let mut buf = [0u8; Node::len()];
            hasher.result(&mut buf);
            (&buf).into()
        }
        struct Executor<'a> {
            path: RepoPathBuf,
            converted_nodes: Vec<(RepoPathBuf, Node, Vec<Node>)>,
            parent_trees: Vec<Cursor<'a>>,
        };
        impl<'a> Executor<'a> {
            fn new(parent_trees: &[&'a Tree]) -> Fallible<Executor<'a>> {
                let mut executor = Executor {
                    path: RepoPathBuf::new(),
                    converted_nodes: Vec::new(),
                    parent_trees: parent_trees.iter().map(|v| v.root_cursor()).collect(),
                };
                // The first node after step is the root directory. `work()` expects cursors to
                // be pointing to the underlying link.
                for cursor in executor.parent_trees.iter_mut() {
                    match cursor.step() {
                        Step::Success | Step::End => (),
                        Step::Err(err) => return Err(err),
                    }
                }
                Ok(executor)
            }
            fn active_parent_tree_nodes(
                &mut self,
                active_parents: &[usize],
            ) -> Fallible<Vec<Node>> {
                let mut parent_nodes = Vec::with_capacity(active_parents.len());
                for id in active_parents {
                    let cursor = &mut self.parent_trees[*id];
                    let node = match cursor.link() {
                        Leaf(_) | Ephemeral(_) => unreachable!(),
                        Durable(entry) => entry.node,
                    };
                    parent_nodes.push(node);
                    match cursor.step() {
                        Step::Success | Step::End => (),
                        Step::Err(err) => return Err(err),
                    }
                }
                Ok(parent_nodes)
            }
            fn parent_trees_for_subdirectory(
                &mut self,
                active_parents: &[usize],
            ) -> Fallible<Vec<usize>> {
                let mut result = Vec::new();
                for id in active_parents.iter() {
                    let cursor = &mut self.parent_trees[*id];
                    while !cursor.finished() && cursor.path() < self.path.as_repo_path() {
                        cursor.skip_subtree();
                        match cursor.step() {
                            Step::Success | Step::End => (),
                            Step::Err(err) => return Err(err),
                        }
                    }
                    if !cursor.finished() && cursor.path() == self.path.as_repo_path() {
                        match cursor.link() {
                            Leaf(_) => (), // files and directories don't share history
                            Durable(_) => result.push(*id),
                            Ephemeral(_) => {
                                panic!("Found ephemeral parent when finalizing manifest.")
                            }
                        }
                    }
                }
                Ok(result)
            }
            fn work(
                &mut self,
                link: &mut Link,
                active_parents: Vec<usize>,
            ) -> Fallible<(Node, store::Flag)> {
                let parent_tree_nodes = self.active_parent_tree_nodes(&active_parents)?;
                match link {
                    Leaf(file_metadata) => Ok((
                        file_metadata.node,
                        store::Flag::File(file_metadata.file_type.clone()),
                    )),
                    Durable(entry) => Ok((entry.node, store::Flag::Directory)),
                    Ephemeral(links) => {
                        let mut entry = store::EntryMut::new();
                        for (component, link) in links.iter_mut() {
                            self.path.push(component.as_path_component());
                            let child_parents =
                                self.parent_trees_for_subdirectory(&active_parents)?;
                            let (node, flag) = self.work(link, child_parents)?;
                            self.path.pop();
                            let element = store::Element::new(component.clone(), node, flag);
                            entry.add_element(element);
                        }
                        let entry = entry.freeze();
                        let node = compute_node(&parent_tree_nodes, &entry);

                        let cell = OnceCell::new();
                        // TODO: remove clone
                        cell.set(Ok(links.clone())).unwrap();

                        let durable_entry = DurableEntry { node, links: cell };
                        let inner = Arc::new(durable_entry);
                        *link = Durable(inner);
                        self.converted_nodes
                            .push((self.path.clone(), node, parent_tree_nodes));
                        Ok((node, store::Flag::Directory))
                    }
                }
            }
        }

        let mut executor = Executor::new(&parent_trees)?;
        executor.work(&mut self.root, (0..parent_trees.len()).collect())?;
        Ok(executor.converted_nodes.into_iter())
    }

    fn get_link(&self, path: &RepoPath) -> Fallible<Option<&Link>> {
        let mut cursor = &self.root;
        for (parent, component) in path.parents().zip(path.components()) {
            let child = match cursor {
                Leaf(_) => bail!("Encountered file where a directory was expected."),
                Ephemeral(links) => links.get(component),
                Durable(ref entry) => {
                    let links = entry.get_links(&self.store, parent)?;
                    links.get(component)
                }
            };
            match child {
                None => return Ok(None),
                Some(link) => cursor = link,
            }
        }
        Ok(Some(cursor))
    }
}

pub struct Files<'a, M> {
    cursor: Cursor<'a>,
    matcher: &'a M,
}

impl<'a, M> Iterator for Files<'a, M>
where
    M: Matcher,
{
    type Item = Fallible<(RepoPathBuf, FileMetadata)>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            match self.cursor.step() {
                Step::Success => {
                    if let Leaf(file_metadata) = self.cursor.link() {
                        if self.matcher.matches_file(self.cursor.path()) {
                            return Some(Ok((self.cursor.path().to_owned(), *file_metadata)));
                        }
                    } else {
                        if self.matcher.matches_directory(self.cursor.path())
                            == DirectoryMatch::Nothing
                        {
                            self.cursor.skip_subtree();
                        }
                    }
                }
                Step::Err(error) => return Some(Err(error)),
                Step::End => return None,
            }
        }
    }
}

/// Returns an iterator over all the differences between two [`Tree`]s. Keeping in mind that
/// manifests operate over files, the difference space is limited to three cases described by
/// [`DiffType`]:
///  * a file may be present only in the left tree manifest
///  * a file may be present only in the right tree manifest
///  * a file may have different file_metadata between the two tree manifests
///
/// For the case where we have the the file "foo" in the `left` tree manifest and we have the "foo"
/// directory in the `right` tree manifest, the differences returned will be:
///  1. DiffEntry("foo", LeftOnly(_))
///  2. DiffEntry(file, RightOnly(_)) for all `file`s under the "foo" directory
pub fn diff<'a, M>(left: &'a Tree, right: &'a Tree, matcher: &'a M) -> Diff<'a, M> {
    Diff {
        left: left.root_cursor(),
        step_left: false,
        right: right.root_cursor(),
        step_right: false,
        matcher,
    }
}

/// An iterator over the differences between two tree manifests.
/// See [`diff()`].
pub struct Diff<'a, M> {
    left: Cursor<'a>,
    step_left: bool,
    right: Cursor<'a>,
    step_right: bool,
    matcher: &'a M,
}

/// Represents a file that is different between two tree manifests.
#[derive(Clone, Debug, Ord, PartialOrd, Eq, PartialEq, Hash)]
pub struct DiffEntry {
    pub path: RepoPathBuf,
    pub diff_type: DiffType,
}

#[derive(Clone, Copy, Debug, Ord, PartialOrd, Eq, PartialEq, Hash)]
pub enum DiffType {
    LeftOnly(FileMetadata),
    RightOnly(FileMetadata),
    Changed(FileMetadata, FileMetadata),
}

impl DiffEntry {
    fn new(path: RepoPathBuf, diff_type: DiffType) -> Self {
        DiffEntry { path, diff_type }
    }
}

impl DiffType {
    /// Returns the metadata of the file in the left manifest when it exists.
    pub fn left(&self) -> Option<FileMetadata> {
        match self {
            DiffType::LeftOnly(left_metadata) => Some(*left_metadata),
            DiffType::RightOnly(_) => None,
            DiffType::Changed(left_metadata, _) => Some(*left_metadata),
        }
    }

    /// Returns the metadata of the file in the right manifest when it exists.
    pub fn right(&self) -> Option<FileMetadata> {
        match self {
            DiffType::LeftOnly(_) => None,
            DiffType::RightOnly(right_metadata) => Some(*right_metadata),
            DiffType::Changed(_, right_metadata) => Some(*right_metadata),
        }
    }
}

impl<'a, M> Iterator for Diff<'a, M>
where
    M: Matcher,
{
    type Item = Fallible<DiffEntry>;

    fn next(&mut self) -> Option<Self::Item> {
        // This is the standard algorithm for returning the differences in two lists but adjusted
        // to have the iterator interface and to evaluate the tree lazily.

        fn diff_entry(path: &RepoPath, diff_type: DiffType) -> Option<Fallible<DiffEntry>> {
            Some(Ok(DiffEntry::new(path.to_owned(), diff_type)))
        }
        fn compare<'a>(left: &Cursor<'a>, right: &Cursor<'a>) -> Option<Ordering> {
            // TODO: cache ordering state so we compare last components at most
            match (left.finished(), right.finished()) {
                (true, true) => None,
                (false, true) => Some(Ordering::Less),
                (true, false) => Some(Ordering::Greater),
                (false, false) => Some(left.path().cmp(right.path())),
            }
        }
        fn evaluate_cursor<M: Matcher>(cursor: &mut Cursor, matcher: &M) -> Option<FileMetadata> {
            if let Leaf(file_metadata) = cursor.link() {
                if matcher.matches_file(cursor.path()) {
                    return Some(*file_metadata);
                }
            }
            try_skipping(cursor, matcher);
            None
        }
        fn try_skipping<M: Matcher>(cursor: &mut Cursor, matcher: &M) {
            if matcher.matches_directory(cursor.path()) == DirectoryMatch::Nothing {
                cursor.skip_subtree();
            }
        }
        loop {
            if self.step_left {
                if let Step::Err(error) = self.left.step() {
                    return Some(Err(error));
                }
                self.step_left = false;
            }
            if self.step_right {
                if let Step::Err(error) = self.right.step() {
                    return Some(Err(error));
                }
                self.step_right = false;
            }
            match compare(&self.left, &self.right) {
                None => return None,
                Some(Ordering::Less) => {
                    self.step_left = true;
                    if let Some(file_metadata) = evaluate_cursor(&mut self.left, &self.matcher) {
                        return diff_entry(self.left.path(), DiffType::LeftOnly(file_metadata));
                    }
                }
                Some(Ordering::Greater) => {
                    self.step_right = true;
                    if let Some(file_metadata) = evaluate_cursor(&mut self.right, &self.matcher) {
                        return diff_entry(self.right.path(), DiffType::RightOnly(file_metadata));
                    }
                }
                Some(Ordering::Equal) => {
                    self.step_left = true;
                    self.step_right = true;
                    match (self.left.link(), self.right.link()) {
                        (Leaf(left_metadata), Leaf(right_metadata)) => {
                            if left_metadata != right_metadata
                                && self.matcher.matches_file(self.left.path())
                            {
                                return diff_entry(
                                    self.left.path(),
                                    DiffType::Changed(*left_metadata, *right_metadata),
                                );
                            }
                        }
                        (Leaf(file_metadata), _) => {
                            try_skipping(&mut self.right, &self.matcher);
                            if self.matcher.matches_file(self.left.path()) {
                                return diff_entry(
                                    self.left.path(),
                                    DiffType::LeftOnly(*file_metadata),
                                );
                            }
                        }
                        (_, Leaf(file_metadata)) => {
                            try_skipping(&mut self.left, &self.matcher);
                            if self.matcher.matches_file(self.right.path()) {
                                return diff_entry(
                                    self.right.path(),
                                    DiffType::RightOnly(*file_metadata),
                                );
                            }
                        }
                        (Durable(left_entry), Durable(right_entry)) => {
                            if left_entry.node == right_entry.node
                                || self.matcher.matches_directory(self.left.path())
                                    == DirectoryMatch::Nothing
                            {
                                self.left.skip_subtree();
                                self.right.skip_subtree();
                            }
                        }
                        _ => {
                            // All other cases are two directories that we would iterate if not
                            // for the matcher
                            if self.matcher.matches_directory(self.left.path())
                                == DirectoryMatch::Nothing
                            {
                                self.left.skip_subtree();
                                self.right.skip_subtree();
                            }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use pathmatcher::{AlwaysMatcher, TreeMatcher};
    use types::testutil::*;

    use self::store::TestStore;
    use crate::FileType;

    fn meta(hex: &str) -> FileMetadata {
        FileMetadata::regular(node(hex))
    }
    fn store_element(path: &str, hex: &str, flag: store::Flag) -> Fallible<store::Element> {
        Ok(store::Element::new(
            path_component_buf(path),
            node(hex),
            flag,
        ))
    }
    fn get_node(tree: &Tree, path: &RepoPath) -> Node {
        match tree.get_link(path).unwrap().unwrap() {
            Leaf(file_metadata) => file_metadata.node,
            Durable(ref entry) => entry.node,
            Ephemeral(_) => panic!("Asked for node on path {} but found ephemeral node.", path),
        }
    }

    #[test]
    fn test_insert() {
        let mut tree = Tree::ephemeral(Arc::new(TestStore::new()));
        tree.insert(repo_path_buf("foo/bar"), meta("10")).unwrap();
        assert_eq!(tree.get(repo_path("foo/bar")).unwrap(), Some(meta("10")));
        assert_eq!(tree.get(repo_path("baz")).unwrap(), None);

        tree.insert(repo_path_buf("baz"), meta("20")).unwrap();
        assert_eq!(tree.get(repo_path("foo/bar")).unwrap(), Some(meta("10")));
        assert_eq!(tree.get(repo_path("baz")).unwrap(), Some(meta("20")));

        tree.insert(repo_path_buf("foo/bat"), meta("30")).unwrap();
        assert_eq!(tree.get(repo_path("foo/bat")).unwrap(), Some(meta("30")));
        assert_eq!(tree.get(repo_path("foo/bar")).unwrap(), Some(meta("10")));
        assert_eq!(tree.get(repo_path("baz")).unwrap(), Some(meta("20")));
    }

    #[test]
    fn test_durable_link() {
        let store = TestStore::new();
        let root_entry = store::Entry::from_elements(vec![
            store_element("foo", "10", store::Flag::Directory),
            store_element("baz", "20", store::Flag::File(FileType::Regular)),
        ])
        .unwrap();
        store
            .insert(RepoPath::empty(), node("1"), root_entry.to_bytes())
            .unwrap();
        let foo_entry = store::Entry::from_elements(vec![store_element(
            "bar",
            "11",
            store::Flag::File(FileType::Regular),
        )])
        .unwrap();
        store
            .insert(repo_path("foo"), node("10"), foo_entry.to_bytes())
            .unwrap();
        let mut tree = Tree::durable(Arc::new(store), node("1"));

        assert_eq!(tree.get(repo_path("foo/bar")).unwrap(), Some(meta("11")));
        assert_eq!(tree.get(repo_path("baz")).unwrap(), Some(meta("20")));

        tree.insert(repo_path_buf("foo/bat"), meta("12")).unwrap();
        assert_eq!(tree.get(repo_path("foo/bat")).unwrap(), Some(meta("12")));
        assert_eq!(tree.get(repo_path("foo/bar")).unwrap(), Some(meta("11")));
        assert_eq!(tree.get(repo_path("baz")).unwrap(), Some(meta("20")));
    }

    #[test]
    fn test_insert_into_directory() {
        let mut tree = Tree::ephemeral(Arc::new(TestStore::new()));
        tree.insert(repo_path_buf("foo/bar/baz"), meta("10"))
            .unwrap();
        assert!(tree.insert(repo_path_buf("foo/bar"), meta("20")).is_err());
        assert!(tree.insert(repo_path_buf("foo"), meta("30")).is_err());
    }

    #[test]
    fn test_insert_with_file_parent() {
        let mut tree = Tree::ephemeral(Arc::new(TestStore::new()));
        tree.insert(repo_path_buf("foo"), meta("10")).unwrap();
        assert!(tree.insert(repo_path_buf("foo/bar"), meta("20")).is_err());
        assert!(tree
            .insert(repo_path_buf("foo/bar/baz"), meta("30"))
            .is_err());
    }

    #[test]
    fn test_get_from_directory() {
        let mut tree = Tree::ephemeral(Arc::new(TestStore::new()));
        tree.insert(repo_path_buf("foo/bar/baz"), meta("10"))
            .unwrap();
        assert!(tree.get(repo_path("foo/bar")).is_err());
        assert!(tree.get(repo_path("foo")).is_err());
    }

    #[test]
    fn test_get_with_file_parent() {
        let mut tree = Tree::ephemeral(Arc::new(TestStore::new()));
        tree.insert(repo_path_buf("foo"), meta("10")).unwrap();
        assert!(tree.get(repo_path("foo/bar")).is_err());
        assert!(tree.get(repo_path("foo/bar/baz")).is_err());
    }

    #[test]
    fn test_remove_from_ephemeral() {
        let mut tree = Tree::ephemeral(Arc::new(TestStore::new()));
        tree.insert(repo_path_buf("a1/b1/c1/d1"), meta("10"))
            .unwrap();
        tree.insert(repo_path_buf("a1/b2"), meta("20")).unwrap();
        tree.insert(repo_path_buf("a2/b2/c2"), meta("30")).unwrap();

        assert!(tree.remove(repo_path("a1")).is_err());
        assert!(tree.remove(repo_path("a1/b1")).is_err());
        assert!(tree.remove(repo_path("a1/b1/c1/d1/e1")).is_err());
        tree.remove(repo_path("a1/b1/c1/d1")).unwrap();
        tree.remove(repo_path("a3")).unwrap(); // does nothing
        tree.remove(repo_path("a1/b3")).unwrap(); // does nothing
        tree.remove(repo_path("a1/b1/c1/d2")).unwrap(); // does nothing
        tree.remove(repo_path("a1/b1/c1/d1/e1")).unwrap(); // does nothing
        assert!(tree.remove(RepoPath::empty()).is_err());
        assert_eq!(tree.get(repo_path("a1/b1/c1/d1")).unwrap(), None);
        assert_eq!(tree.get(repo_path("a1/b1/c1")).unwrap(), None);
        assert_eq!(tree.get(repo_path("a1/b2")).unwrap(), Some(meta("20")));
        tree.remove(repo_path("a1/b2")).unwrap();
        assert_eq!(tree.get_link(repo_path("a1")).unwrap(), None);

        assert_eq!(tree.get(repo_path("a2/b2/c2")).unwrap(), Some(meta("30")));
        tree.remove(repo_path("a2/b2/c2")).unwrap();
        assert_eq!(tree.get(repo_path("a2")).unwrap(), None);

        assert!(tree.get_link(RepoPath::empty()).unwrap().is_some());
    }

    #[test]
    fn test_remove_from_durable() {
        let store = TestStore::new();
        let root_entry = store::Entry::from_elements(vec![
            store_element("a1", "10", store::Flag::Directory),
            store_element("a2", "20", store::Flag::File(FileType::Regular)),
        ])
        .unwrap();
        store
            .insert(RepoPath::empty(), node("1"), root_entry.to_bytes())
            .unwrap();
        let a1_entry = store::Entry::from_elements(vec![
            store_element("b1", "11", store::Flag::File(FileType::Regular)),
            store_element("b2", "12", store::Flag::File(FileType::Regular)),
        ])
        .unwrap();
        store
            .insert(repo_path("a1"), node("10"), a1_entry.to_bytes())
            .unwrap();
        let mut tree = Tree::durable(Arc::new(store), node("1"));

        assert!(tree.remove(repo_path("a1")).is_err());
        tree.remove(repo_path("a1/b1")).unwrap();
        assert_eq!(tree.get(repo_path("a1/b1")).unwrap(), None);
        assert_eq!(tree.get(repo_path("a1/b2")).unwrap(), Some(meta("12")));
        tree.remove(repo_path("a1/b2")).unwrap();
        assert_eq!(tree.get(repo_path("a1/b2")).unwrap(), None);
        assert_eq!(tree.get(repo_path("a1")).unwrap(), None);
        assert_eq!(tree.get_link(repo_path("a1")).unwrap(), None);

        assert_eq!(tree.get(repo_path("a2")).unwrap(), Some(meta("20")));
        tree.remove(repo_path("a2")).unwrap();
        assert_eq!(tree.get(repo_path("a2")).unwrap(), None);

        assert!(tree.get_link(RepoPath::empty()).unwrap().is_some());
    }

    #[test]
    fn test_flush() {
        let store = Arc::new(TestStore::new());
        let mut tree = Tree::ephemeral(store.clone());
        tree.insert(repo_path_buf("a1/b1/c1/d1"), meta("10"))
            .unwrap();
        tree.insert(repo_path_buf("a1/b2"), meta("20")).unwrap();
        tree.insert(repo_path_buf("a2/b2/c2"), meta("30")).unwrap();

        let node = tree.flush().unwrap();

        let tree = Tree::durable(store.clone(), node);
        assert_eq!(
            tree.get(repo_path("a1/b1/c1/d1")).unwrap(),
            Some(meta("10"))
        );
        assert_eq!(tree.get(repo_path("a1/b2")).unwrap(), Some(meta("20")));
        assert_eq!(tree.get(repo_path("a2/b2/c2")).unwrap(), Some(meta("30")));
        assert_eq!(tree.get(repo_path("a2/b1")).unwrap(), None);
    }

    #[test]
    fn test_finalize_with_zero_and_one_parents() {
        let mut tree = Tree::ephemeral(Arc::new(TestStore::new()));
        tree.insert(repo_path_buf("a1/b1/c1/d1"), meta("10"))
            .unwrap();
        tree.insert(repo_path_buf("a1/b2"), meta("20")).unwrap();
        tree.insert(repo_path_buf("a2/b2/c2"), meta("30")).unwrap();
        let tree_changed: Vec<_> = tree.finalize(vec![]).unwrap().collect();

        assert_eq!(tree_changed.len(), 6);
        assert_eq!(tree_changed[0].0, repo_path_buf("a1/b1/c1"));
        assert_eq!(tree_changed[1].0, repo_path_buf("a1/b1"));
        assert_eq!(tree_changed[2].0, repo_path_buf("a1"));
        assert_eq!(tree_changed[3].0, repo_path_buf("a2/b2"));
        assert_eq!(tree_changed[4].0, repo_path_buf("a2"));
        assert_eq!(tree_changed[5].0, RepoPathBuf::new());

        let mut update = tree.clone();
        update.insert(repo_path_buf("a1/b2"), meta("40")).unwrap();
        update.remove(repo_path("a2/b2/c2")).unwrap();
        update.insert(repo_path_buf("a3/b1"), meta("50")).unwrap();
        let update_changed: Vec<_> = update.finalize(vec![&tree]).unwrap().collect();
        assert_eq!(update_changed[0].0, repo_path_buf("a1"));
        assert_eq!(update_changed[0].2, vec![tree_changed[2].1]);
        assert_eq!(update_changed[1].0, repo_path_buf("a3"));
        assert_eq!(update_changed[1].2, vec![]);
        assert_eq!(update_changed[2].0, RepoPathBuf::new());
        assert_eq!(update_changed[2].2, vec![tree_changed[5].1]);
    }

    #[test]
    fn test_finalize_merge() {
        let store = Arc::new(TestStore::new());
        let mut p1 = Tree::ephemeral(store.clone());
        p1.insert(repo_path_buf("a1/b1/c1/d1"), meta("10")).unwrap();
        p1.insert(repo_path_buf("a1/b2"), meta("20")).unwrap();
        p1.insert(repo_path_buf("a2/b2/c2"), meta("30")).unwrap();
        let _p1_changed = p1.finalize(vec![]).unwrap();

        let mut p2 = Tree::ephemeral(store.clone());
        p2.insert(repo_path_buf("a1/b2"), meta("40")).unwrap();
        p2.insert(repo_path_buf("a3/b1"), meta("50")).unwrap();
        let _p2_changed = p2.finalize(vec![]).unwrap();

        let mut tree = p1.clone();
        tree.insert(repo_path_buf("a1/b2"), meta("40")).unwrap();
        tree.insert(repo_path_buf("a2/b2/c2"), meta("60")).unwrap();
        tree.insert(repo_path_buf("a3/b1"), meta("50")).unwrap();
        let tree_changed: Vec<_> = tree.finalize(vec![&p1, &p2]).unwrap().collect();
        assert_eq!(tree_changed[0].0, repo_path_buf("a1"));
        assert_eq!(
            tree_changed[0].2,
            vec![
                get_node(&p1, repo_path("a1")),
                get_node(&p2, repo_path("a1"))
            ]
        );
        assert_eq!(tree_changed[1].0, repo_path_buf("a2/b2"));
        assert_eq!(tree_changed[1].2, vec![get_node(&p1, repo_path("a2/b2"))]);
        assert_eq!(tree_changed[2].0, repo_path_buf("a2"));
        assert_eq!(tree_changed[3].0, repo_path_buf("a3"));
        assert_eq!(tree_changed[3].2, vec![get_node(&p2, repo_path("a3"))]);
        assert_eq!(tree_changed[4].0, RepoPathBuf::new());
        assert_eq!(
            tree_changed[4].2,
            vec![
                get_node(&p1, RepoPath::empty()),
                get_node(&p2, RepoPath::empty())
            ]
        );
    }

    #[test]
    fn test_finalize_file_to_directory() {
        let store = Arc::new(TestStore::new());
        let mut tree1 = Tree::ephemeral(store.clone());
        tree1.insert(repo_path_buf("a1"), meta("10")).unwrap();
        let tree1_changed: Vec<_> = tree1.finalize(vec![]).unwrap().collect();
        assert_eq!(tree1_changed[0].0, RepoPathBuf::new());
        assert_eq!(tree1_changed[0].2, vec![]);

        let mut tree2 = Tree::ephemeral(store.clone());
        tree2.insert(repo_path_buf("a1/b1"), meta("20")).unwrap();
        let tree2_changed: Vec<_> = tree2.finalize(vec![&tree1]).unwrap().collect();
        assert_eq!(tree2_changed[0].0, repo_path_buf("a1"));
        assert_eq!(tree2_changed[0].2, vec![]);
        assert_eq!(tree2_changed[1].0, RepoPathBuf::new());
        assert_eq!(tree2_changed[1].2, vec![tree1_changed[0].1]);

        let mut tree3 = Tree::ephemeral(store.clone());
        tree3.insert(repo_path_buf("a1"), meta("30")).unwrap();
        let tree3_changed: Vec<_> = tree3.finalize(vec![&tree2]).unwrap().collect();
        assert_eq!(tree3_changed[0].0, RepoPathBuf::new());
        assert_eq!(tree3_changed[0].2, vec![tree2_changed[1].1]);
    }

    #[test]
    fn test_cursor_skip_on_root() {
        let tree = Tree::ephemeral(Arc::new(TestStore::new()));
        let mut cursor = tree.root_cursor();
        cursor.skip_subtree();
        match cursor.step() {
            Step::Success => panic!("should have reached the end of the tree"),
            Step::End => (), // success
            Step::Err(error) => panic!(error),
        }
    }

    #[test]
    fn test_cursor_skip() {
        fn step<'a>(cursor: &mut Cursor<'a>) {
            match cursor.step() {
                Step::Success => (),
                Step::End => panic!("reached the end too soon"),
                Step::Err(error) => panic!(error),
            }
        }
        let mut tree = Tree::ephemeral(Arc::new(TestStore::new()));
        tree.insert(repo_path_buf("a1"), meta("10")).unwrap();
        tree.insert(repo_path_buf("a2/b2"), meta("20")).unwrap();
        tree.insert(repo_path_buf("a3"), meta("30")).unwrap();

        let mut cursor = tree.root_cursor();
        step(&mut cursor);
        assert_eq!(cursor.path(), RepoPath::empty());
        step(&mut cursor);
        assert_eq!(cursor.path(), RepoPath::from_str("a1").unwrap());
        // Skip leaf
        cursor.skip_subtree();
        step(&mut cursor);
        assert_eq!(cursor.path(), RepoPath::from_str("a2").unwrap());
        // Skip directory
        cursor.skip_subtree();
        step(&mut cursor);
        assert_eq!(cursor.path(), RepoPath::from_str("a3").unwrap());
        // Skip on the element before State::End
        cursor.skip_subtree();
        match cursor.step() {
            Step::Success => panic!("should have reached the end of the tree"),
            Step::End => (), // success
            Step::Err(error) => panic!(error),
        }
    }

    #[test]
    fn test_files_empty() {
        let tree = Tree::ephemeral(Arc::new(TestStore::new()));
        assert!(tree.files(&AlwaysMatcher::new()).next().is_none());
    }

    #[test]
    fn test_files_ephemeral() {
        let mut tree = Tree::ephemeral(Arc::new(TestStore::new()));
        tree.insert(repo_path_buf("a1/b1/c1/d1"), meta("10"))
            .unwrap();
        tree.insert(repo_path_buf("a1/b2"), meta("20")).unwrap();
        tree.insert(repo_path_buf("a2/b2/c2"), meta("30")).unwrap();

        assert_eq!(
            tree.files(&AlwaysMatcher::new())
                .collect::<Fallible<Vec<_>>>()
                .unwrap(),
            vec!(
                (repo_path_buf("a1/b1/c1/d1"), meta("10")),
                (repo_path_buf("a1/b2"), meta("20")),
                (repo_path_buf("a2/b2/c2"), meta("30")),
            )
        );
    }

    #[test]
    fn test_files_durable() {
        let store = Arc::new(TestStore::new());
        let mut tree = Tree::ephemeral(store.clone());
        tree.insert(repo_path_buf("a1/b1/c1/d1"), meta("10"))
            .unwrap();
        tree.insert(repo_path_buf("a1/b2"), meta("20")).unwrap();
        tree.insert(repo_path_buf("a2/b2/c2"), meta("30")).unwrap();
        let node = tree.flush().unwrap();
        let tree = Tree::durable(store.clone(), node);

        assert_eq!(
            tree.files(&AlwaysMatcher::new())
                .collect::<Fallible<Vec<_>>>()
                .unwrap(),
            vec!(
                (repo_path_buf("a1/b1/c1/d1"), meta("10")),
                (repo_path_buf("a1/b2"), meta("20")),
                (repo_path_buf("a2/b2/c2"), meta("30")),
            )
        );
    }

    #[test]
    fn test_files_matcher() {
        let mut tree = Tree::ephemeral(Arc::new(TestStore::new()));
        tree.insert(repo_path_buf("a1/b1/c1/d1"), meta("10"))
            .unwrap();
        tree.insert(repo_path_buf("a1/b2"), meta("20")).unwrap();
        tree.insert(repo_path_buf("a2/b2/c2"), meta("30")).unwrap();
        tree.insert(repo_path_buf("a2/b2/c3"), meta("40")).unwrap();
        tree.insert(repo_path_buf("a3/b2/c3"), meta("50")).unwrap();

        assert_eq!(
            tree.files(&TreeMatcher::from_rules(["a2/b2"].iter()))
                .collect::<Fallible<Vec<_>>>()
                .unwrap(),
            vec!(
                (repo_path_buf("a2/b2/c2"), meta("30")),
                (repo_path_buf("a2/b2/c3"), meta("40"))
            )
        );
        assert_eq!(
            tree.files(&TreeMatcher::from_rules(["a1/*/c1"].iter()))
                .collect::<Fallible<Vec<_>>>()
                .unwrap(),
            vec!((repo_path_buf("a1/b1/c1/d1"), meta("10")),)
        );
        assert_eq!(
            tree.files(&TreeMatcher::from_rules(["**/c3"].iter()))
                .collect::<Fallible<Vec<_>>>()
                .unwrap(),
            vec!(
                (repo_path_buf("a2/b2/c3"), meta("40")),
                (repo_path_buf("a3/b2/c3"), meta("50"))
            )
        );
    }

    #[test]
    fn test_files_finish_on_error_when_collecting_to_vec() {
        let tree = Tree::durable(Arc::new(TestStore::new()), node("1"));
        let file_results = tree.files(&AlwaysMatcher::new()).collect::<Vec<_>>();
        assert_eq!(file_results.len(), 1);
        assert!(file_results[0].is_err());

        let files_result = tree
            .files(&AlwaysMatcher::new())
            .collect::<Result<Vec<_>, _>>();
        assert!(files_result.is_err());
    }

    #[test]
    fn test_diff_generic() {
        let mut left = Tree::ephemeral(Arc::new(TestStore::new()));
        left.insert(repo_path_buf("a1/b1/c1/d1"), meta("10"))
            .unwrap();
        left.insert(repo_path_buf("a1/b2"), meta("20")).unwrap();
        left.insert(repo_path_buf("a3/b1"), meta("40")).unwrap();

        let mut right = Tree::ephemeral(Arc::new(TestStore::new()));
        right.insert(repo_path_buf("a1/b2"), meta("40")).unwrap();
        right.insert(repo_path_buf("a2/b2/c2"), meta("30")).unwrap();
        right.insert(repo_path_buf("a3/b1"), meta("40")).unwrap();

        assert_eq!(
            diff(&left, &right, &AlwaysMatcher::new())
                .collect::<Fallible<Vec<_>>>()
                .unwrap(),
            vec!(
                DiffEntry::new(repo_path_buf("a1/b1/c1/d1"), DiffType::LeftOnly(meta("10"))),
                DiffEntry::new(
                    repo_path_buf("a1/b2"),
                    DiffType::Changed(meta("20"), meta("40"))
                ),
                DiffEntry::new(repo_path_buf("a2/b2/c2"), DiffType::RightOnly(meta("30"))),
            )
        );

        left.flush().unwrap();
        right.flush().unwrap();

        assert_eq!(
            diff(&left, &right, &AlwaysMatcher::new())
                .collect::<Fallible<Vec<_>>>()
                .unwrap(),
            vec!(
                DiffEntry::new(repo_path_buf("a1/b1/c1/d1"), DiffType::LeftOnly(meta("10"))),
                DiffEntry::new(
                    repo_path_buf("a1/b2"),
                    DiffType::Changed(meta("20"), meta("40"))
                ),
                DiffEntry::new(repo_path_buf("a2/b2/c2"), DiffType::RightOnly(meta("30"))),
            )
        );
        right
            .insert(repo_path_buf("a1/b1/c1/d1"), meta("10"))
            .unwrap();
        left.insert(repo_path_buf("a1/b2"), meta("40")).unwrap();
        left.insert(repo_path_buf("a2/b2/c2"), meta("30")).unwrap();

        assert!(diff(&left, &right, &AlwaysMatcher::new()).next().is_none());
    }

    #[test]
    fn test_diff_does_not_evaluate_durable_on_node_equality() {
        // Leaving the store empty intentionaly so that we get a panic if anything is read from it.
        let left = Tree::durable(Arc::new(TestStore::new()), node("10"));
        let right = Tree::durable(Arc::new(TestStore::new()), node("10"));
        assert!(diff(&left, &right, &AlwaysMatcher::new()).next().is_none());

        let right = Tree::durable(Arc::new(TestStore::new()), node("20"));
        assert!(diff(&left, &right, &AlwaysMatcher::new())
            .next()
            .unwrap()
            .is_err());
    }

    #[test]
    fn test_diff_one_file_one_directory() {
        let mut left = Tree::ephemeral(Arc::new(TestStore::new()));
        left.insert(repo_path_buf("a1/b1"), meta("10")).unwrap();
        left.insert(repo_path_buf("a2"), meta("20")).unwrap();

        let mut right = Tree::ephemeral(Arc::new(TestStore::new()));
        right.insert(repo_path_buf("a1"), meta("30")).unwrap();
        right.insert(repo_path_buf("a2/b2"), meta("40")).unwrap();

        assert_eq!(
            diff(&left, &right, &AlwaysMatcher::new())
                .collect::<Fallible<Vec<_>>>()
                .unwrap(),
            vec!(
                DiffEntry::new(repo_path_buf("a1"), DiffType::RightOnly(meta("30"))),
                DiffEntry::new(repo_path_buf("a1/b1"), DiffType::LeftOnly(meta("10"))),
                DiffEntry::new(repo_path_buf("a2"), DiffType::LeftOnly(meta("20"))),
                DiffEntry::new(repo_path_buf("a2/b2"), DiffType::RightOnly(meta("40"))),
            )
        );
    }

    #[test]
    fn test_diff_left_empty() {
        let mut left = Tree::ephemeral(Arc::new(TestStore::new()));

        let mut right = Tree::ephemeral(Arc::new(TestStore::new()));
        right
            .insert(repo_path_buf("a1/b1/c1/d1"), meta("10"))
            .unwrap();
        right.insert(repo_path_buf("a1/b2"), meta("20")).unwrap();
        right.insert(repo_path_buf("a2/b2/c2"), meta("30")).unwrap();

        assert_eq!(
            diff(&left, &right, &AlwaysMatcher::new())
                .collect::<Fallible<Vec<_>>>()
                .unwrap(),
            vec!(
                DiffEntry::new(
                    repo_path_buf("a1/b1/c1/d1"),
                    DiffType::RightOnly(meta("10"))
                ),
                DiffEntry::new(repo_path_buf("a1/b2"), DiffType::RightOnly(meta("20"))),
                DiffEntry::new(repo_path_buf("a2/b2/c2"), DiffType::RightOnly(meta("30"))),
            )
        );

        left.flush().unwrap();
        right.flush().unwrap();

        assert_eq!(
            diff(&left, &right, &AlwaysMatcher::new())
                .collect::<Fallible<Vec<_>>>()
                .unwrap(),
            vec!(
                DiffEntry::new(
                    repo_path_buf("a1/b1/c1/d1"),
                    DiffType::RightOnly(meta("10"))
                ),
                DiffEntry::new(repo_path_buf("a1/b2"), DiffType::RightOnly(meta("20"))),
                DiffEntry::new(repo_path_buf("a2/b2/c2"), DiffType::RightOnly(meta("30"))),
            )
        );
    }

    #[test]
    fn test_diff_matcher() {
        let mut left = Tree::ephemeral(Arc::new(TestStore::new()));
        left.insert(repo_path_buf("a1/b1/c1/d1"), meta("10"))
            .unwrap();
        left.insert(repo_path_buf("a1/b2"), meta("20")).unwrap();
        left.insert(repo_path_buf("a3/b1"), meta("40")).unwrap();

        let mut right = Tree::ephemeral(Arc::new(TestStore::new()));
        right.insert(repo_path_buf("a1/b2"), meta("40")).unwrap();
        right.insert(repo_path_buf("a2/b2/c2"), meta("30")).unwrap();
        right.insert(repo_path_buf("a3/b1"), meta("40")).unwrap();

        assert_eq!(
            diff(&left, &right, &TreeMatcher::from_rules(["a1/b1"].iter()))
                .collect::<Fallible<Vec<_>>>()
                .unwrap(),
            vec!(DiffEntry::new(
                repo_path_buf("a1/b1/c1/d1"),
                DiffType::LeftOnly(meta("10"))
            ),)
        );
        assert_eq!(
            diff(&left, &right, &TreeMatcher::from_rules(["a1/b2"].iter()))
                .collect::<Fallible<Vec<_>>>()
                .unwrap(),
            vec!(DiffEntry::new(
                repo_path_buf("a1/b2"),
                DiffType::Changed(meta("20"), meta("40"))
            ),)
        );
        assert_eq!(
            diff(&left, &right, &TreeMatcher::from_rules(["a2/b2"].iter()))
                .collect::<Fallible<Vec<_>>>()
                .unwrap(),
            vec!(DiffEntry::new(
                repo_path_buf("a2/b2/c2"),
                DiffType::RightOnly(meta("30"))
            ),)
        );
        assert_eq!(
            diff(&left, &right, &TreeMatcher::from_rules(["*/b2"].iter()))
                .collect::<Fallible<Vec<_>>>()
                .unwrap(),
            vec!(
                DiffEntry::new(
                    repo_path_buf("a1/b2"),
                    DiffType::Changed(meta("20"), meta("40"))
                ),
                DiffEntry::new(repo_path_buf("a2/b2/c2"), DiffType::RightOnly(meta("30"))),
            )
        );
        assert!(
            diff(&left, &right, &TreeMatcher::from_rules(["a3/**"].iter()))
                .next()
                .is_none()
        );
    }

    #[test]
    fn test_debug() {
        use std::fmt::Write;

        let store = Arc::new(TestStore::new());
        let mut tree = Tree::ephemeral(store.clone());
        tree.insert(repo_path_buf("a1/b1/c1/d1"), meta("10"))
            .unwrap();
        let _node = tree.flush().unwrap();

        tree.insert(repo_path_buf("a1/b2"), meta("20")).unwrap();
        tree.insert(repo_path_buf("a2/b2/c2"), meta("30")).unwrap();

        let mut output = String::new();
        write!(output, "{:?}", tree).unwrap();
        assert_eq!(
            output,
            "Root (Ephemeral)\n\
             | a1 (Ephemeral)\n\
             | | b1 (Durable, 4f75b40350c5a77ea27d3287b371016e2d940bab)\n\
             | | | c1 (Durable, 4495bc0cc4093ed880fe1eb1489635f3cddcf04d)\n\
             | | | | d1 (File, 0000000000000000000000000000000000000010, Regular)\n\
             | | b2 (File, 0000000000000000000000000000000000000020, Regular)\n\
             | a2 (Ephemeral)\n\
             | | b2 (Ephemeral)\n\
             | | | c2 (File, 0000000000000000000000000000000000000030, Regular)\n\
             "
        );
    }
}
