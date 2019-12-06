/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod cursor;
mod diff;
mod files;
mod link;
mod store;
#[cfg(test)]
mod testutil;

use std::{
    collections::{btree_map::Entry, BTreeMap},
    fmt,
    sync::Arc,
};

use anyhow::Result;
use bytes::Bytes;
use crypto::{digest::Digest, sha1::Sha1};
use once_cell::sync::OnceCell;
use thiserror::Error;

use pathmatcher::Matcher;
use types::{HgId, Key, PathComponent, PathComponentBuf, RepoPath, RepoPathBuf};

pub(crate) use self::link::Link;
pub use self::{diff::Diff, store::TreeStore};
use crate::{
    tree::{
        cursor::{Cursor, Step},
        files::Items,
        link::{Directory, Durable, DurableEntry, Ephemeral, Leaf},
        store::InnerStore,
    },
    DiffEntry, File, FileMetadata, FsNode, Manifest,
};

/// The Tree implementation of a Manifest dedicates an inner node for each directory in the
/// repository and a leaf for each file.
#[derive(Clone)]
pub struct Tree {
    store: InnerStore,
    // TODO: root can't be a Leaf
    root: Link,
}

#[derive(Error, Debug)]
#[error("failure inserting '{path}' in manifest")]
pub struct InsertError {
    pub path: RepoPathBuf,
    pub file_metadata: FileMetadata,
    pub source: InsertErrorCause,
}

impl InsertError {
    pub fn new(path: RepoPathBuf, file_metadata: FileMetadata, source: InsertErrorCause) -> Self {
        Self {
            path,
            file_metadata,
            source,
        }
    }
}

#[derive(thiserror::Error, Debug)]
pub enum InsertErrorCause {
    #[error("'{0}' is already a file")]
    ParentFileExists(RepoPathBuf),
    #[error("file path is already a directory")]
    DirectoryExistsForPath,
}

impl Tree {
    /// Instantiates a tree manifest that was stored with the specificed `HgId`
    pub fn durable(store: Arc<dyn TreeStore + Send + Sync>, hgid: HgId) -> Self {
        Tree {
            store: InnerStore::new(store),
            root: Link::durable(hgid),
        }
    }

    /// Instantiates a new tree manifest with no history
    pub fn ephemeral(store: Arc<dyn TreeStore + Send + Sync>) -> Self {
        Tree {
            store: InnerStore::new(store),
            root: Link::Ephemeral(BTreeMap::new()),
        }
    }

    fn root_cursor<'a>(&'a self) -> Cursor<'a> {
        Cursor::new(&self.store, RepoPathBuf::new(), &self.root)
    }
}

impl Manifest for Tree {
    fn get(&self, path: &RepoPath) -> Result<Option<FsNode>> {
        let result = self.get_link(path)?.map(|link| link.to_fs_node());
        Ok(result)
    }

    fn insert(&mut self, path: RepoPathBuf, file_metadata: FileMetadata) -> Result<()> {
        let mut cursor = &self.root;
        let mut must_insert = false;
        for (parent, component) in path.parents().zip(path.components()) {
            let child = match cursor {
                Leaf(_) => Err(InsertError::new(
                    path.clone(), // TODO: get rid of clone (it is borrowed)
                    file_metadata,
                    InsertErrorCause::ParentFileExists(parent.to_owned()),
                ))?,
                Ephemeral(links) => links.get(component),
                Durable(ref entry) => {
                    let links = entry.materialize_links(&self.store, parent)?;
                    links.get(component)
                }
            };
            match child {
                None => {
                    must_insert = true;
                    break;
                }
                Some(link) => cursor = link,
            }
        }
        if must_insert == false {
            match cursor {
                Leaf(existing_metadata) => {
                    if *existing_metadata == file_metadata {
                        return Ok(()); // nothing to do
                    }
                }
                Ephemeral(_) | Durable(_) => Err(InsertError::new(
                    path.clone(), // TODO: get rid of clone (it is borrowed later)
                    file_metadata,
                    InsertErrorCause::DirectoryExistsForPath,
                ))?,
            }
        }
        let (path_parent, last_component) = path.split_last_component().unwrap();
        let mut cursor = &mut self.root;
        // unwrap is fine because root would have been a directory
        for (parent, component) in path_parent.parents().zip(path_parent.components()) {
            cursor = cursor
                .mut_ephemeral_links(&self.store, parent)?
                .entry(component.to_owned())
                .or_insert_with(|| Ephemeral(BTreeMap::new()));
        }
        match cursor
            .mut_ephemeral_links(&self.store, path_parent)?
            .entry(last_component.to_owned())
        {
            Entry::Vacant(entry) => {
                entry.insert(Link::Leaf(file_metadata));
            }
            Entry::Occupied(mut entry) => {
                if let Leaf(ref mut store_ref) = entry.get_mut() {
                    *store_ref = file_metadata;
                } else {
                    unreachable!("Unexpected directory found while insert.");
                }
            }
        }
        Ok(())
    }

    fn remove(&mut self, path: &RepoPath) -> Result<Option<FileMetadata>> {
        // The return value lets us know if there are no more files in the subtree and we should be
        // removing it.
        fn do_remove<'a, I>(store: &InnerStore, cursor: &mut Link, iter: &mut I) -> Result<bool>
        where
            I: Iterator<Item = (&'a RepoPath, &'a PathComponent)>,
        {
            match iter.next() {
                None => {
                    if let Leaf(_) = cursor {
                        // We reached the file that we want to remove.
                        Ok(true)
                    } else {
                        unreachable!("Unexpected directory found while remove.");
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
        if let Some(file_metadata) = self.get_file(path)? {
            do_remove(
                &self.store,
                &mut self.root,
                &mut path.parents().zip(path.components()),
            )?;
            Ok(Some(file_metadata))
        } else {
            Ok(None)
        }
    }

    fn flush(&mut self) -> Result<HgId> {
        fn compute_hgid<C: AsRef<[u8]>>(content: C) -> HgId {
            let mut hasher = Sha1::new();
            hasher.input(content.as_ref());
            let mut buf = [0u8; HgId::len()];
            hasher.result(&mut buf);
            (&buf).into()
        }
        fn do_flush<'a, 'b, 'c>(
            store: &'a InnerStore,
            pathbuf: &'b mut RepoPathBuf,
            cursor: &'c mut Link,
        ) -> Result<(&'c HgId, store::Flag)> {
            loop {
                match cursor {
                    Leaf(file_metadata) => {
                        return Ok((
                            &file_metadata.hgid,
                            store::Flag::File(file_metadata.file_type.clone()),
                        ));
                    }
                    Durable(entry) => return Ok((&entry.hgid, store::Flag::Directory)),
                    Ephemeral(links) => {
                        let iter = links.iter_mut().map(|(component, link)| {
                            pathbuf.push(component.as_path_component());
                            let (hgid, flag) = do_flush(store, pathbuf, link)?;
                            pathbuf.pop();
                            Ok(store::Element::new(
                                component.to_owned(),
                                hgid.clone(),
                                flag,
                            ))
                        });
                        let entry = store::Entry::from_elements(iter)?;
                        let hgid = compute_hgid(&entry);
                        store.insert_entry(&pathbuf, hgid, entry)?;

                        let cell = OnceCell::new();
                        // TODO: remove clone
                        cell.set(Ok(links.clone())).unwrap();

                        let durable_entry = DurableEntry { hgid, links: cell };
                        *cursor = Durable(Arc::new(durable_entry));
                    }
                }
            }
        }
        let mut path = RepoPathBuf::new();
        let (hgid, _) = do_flush(&self.store, &mut path, &mut self.root)?;
        Ok(hgid.clone())
    }

    fn files<'a, M: Matcher>(
        &'a self,
        matcher: &'a M,
    ) -> Box<dyn Iterator<Item = Result<File>> + 'a> {
        let files = Items::new(&self, matcher).filter_map(|result| match result {
            Ok((path, FsNode::File(metadata))) => Some(Ok(File::new(path, metadata))),
            Ok(_) => None,
            Err(e) => Some(Err(e)),
        });
        Box::new(files)
    }

    /// Returns an iterator over all the directories that are present in the
    /// tree.
    ///
    /// Note: the matcher should be a prefix matcher, other kinds of matchers
    /// could be less effective than expected.
    fn dirs<'a, M: Matcher>(
        &'a self,
        matcher: &'a M,
    ) -> Box<dyn Iterator<Item = Result<crate::Directory>> + 'a> {
        let dirs = Items::new(&self, matcher).filter_map(|result| match result {
            Ok((path, FsNode::Directory(metadata))) => {
                Some(Ok(crate::Directory::new(path, metadata)))
            }
            Ok(_) => None,
            Err(e) => Some(Err(e)),
        });
        Box::new(dirs)
    }

    fn diff<'a, M: Matcher>(
        &'a self,
        other: &'a Self,
        matcher: &'a M,
    ) -> Box<dyn Iterator<Item = Result<DiffEntry>> + 'a> {
        Box::new(Diff::new(self, other, matcher))
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
                    write!(f, "(File, {}, {:?})\n", metadata.hgid, metadata.file_type)
                }
                Link::Ephemeral(children) => {
                    write!(f, "(Ephemeral)\n")?;
                    write_children(f, children, indent)
                }
                Link::Durable(entry) => {
                    write!(f, "(Durable, {})\n", entry.hgid)?;
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
    pub fn finalize(
        &mut self,
        parent_trees: Vec<&Tree>,
    ) -> Result<impl Iterator<Item = (RepoPathBuf, HgId, Bytes, HgId, HgId)>> {
        fn compute_hgid<C: AsRef<[u8]>>(parent_tree_nodes: &[HgId], content: C) -> HgId {
            let mut hasher = Sha1::new();
            debug_assert!(parent_tree_nodes.len() <= 2);
            let p1 = parent_tree_nodes.get(0).unwrap_or(HgId::null_id());
            let p2 = parent_tree_nodes.get(1).unwrap_or(HgId::null_id());
            // Even if parents are sorted two hashes go into hash computation but surprise
            // the NULL_ID is not a special case in this case and gets sorted.
            if p1 < p2 {
                hasher.input(p1.as_ref());
                hasher.input(p2.as_ref());
            } else {
                hasher.input(p2.as_ref());
                hasher.input(p1.as_ref());
            }
            hasher.input(content.as_ref());
            let mut buf = [0u8; HgId::len()];
            hasher.result(&mut buf);
            (&buf).into()
        }
        struct Executor<'a> {
            store: &'a InnerStore,
            path: RepoPathBuf,
            converted_nodes: Vec<(RepoPathBuf, HgId, Bytes, HgId, HgId)>,
            parent_trees: Vec<Cursor<'a>>,
        };
        impl<'a> Executor<'a> {
            fn new(store: &'a InnerStore, parent_trees: &[&'a Tree]) -> Result<Executor<'a>> {
                let mut executor = Executor {
                    store,
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
            fn active_parent_tree_nodes(&self, active_parents: &[usize]) -> Result<Vec<HgId>> {
                let mut parent_nodes = Vec::with_capacity(active_parents.len());
                for id in active_parents {
                    let cursor = &self.parent_trees[*id];
                    let hgid = match cursor.link() {
                        Leaf(_) | Ephemeral(_) => unreachable!(),
                        Durable(entry) => entry.hgid,
                    };
                    parent_nodes.push(hgid);
                }
                Ok(parent_nodes)
            }
            fn advance_parents(&mut self, active_parents: &[usize]) -> Result<()> {
                for id in active_parents {
                    let cursor = &mut self.parent_trees[*id];
                    match cursor.step() {
                        Step::Success | Step::End => (),
                        Step::Err(err) => return Err(err),
                    }
                }
                Ok(())
            }
            fn parent_trees_for_subdirectory(
                &mut self,
                active_parents: &[usize],
            ) -> Result<Vec<usize>> {
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
            ) -> Result<(HgId, store::Flag)> {
                let parent_tree_nodes = self.active_parent_tree_nodes(&active_parents)?;
                if let Durable(entry) = link {
                    if parent_tree_nodes.contains(&entry.hgid) {
                        return Ok((entry.hgid, store::Flag::Directory));
                    }
                }
                self.advance_parents(&active_parents)?;
                if let Leaf(file_metadata) = link {
                    return Ok((
                        file_metadata.hgid,
                        store::Flag::File(file_metadata.file_type.clone()),
                    ));
                }
                // TODO: This code is also used on durable nodes for the purpose of generating
                // a list of entries to insert in the local store. For those cases we don't
                // need to convert to Ephemeral instead only verify the hash.
                let links = link.mut_ephemeral_links(self.store, &self.path)?;
                let mut entry = store::EntryMut::new();
                for (component, link) in links.iter_mut() {
                    self.path.push(component.as_path_component());
                    let child_parents = self.parent_trees_for_subdirectory(&active_parents)?;
                    let (hgid, flag) = self.work(link, child_parents)?;
                    self.path.pop();
                    let element = store::Element::new(component.clone(), hgid, flag);
                    entry.add_element(element);
                }
                let entry = entry.freeze();
                let hgid = compute_hgid(&parent_tree_nodes, &entry);

                let cell = OnceCell::new();
                // TODO: remove clone
                cell.set(Ok(links.clone())).unwrap();

                let durable_entry = DurableEntry { hgid, links: cell };
                let inner = Arc::new(durable_entry);
                *link = Durable(inner);
                let parent_hgid = |id| *parent_tree_nodes.get(id).unwrap_or(HgId::null_id());
                self.converted_nodes.push((
                    self.path.clone(),
                    hgid,
                    entry.to_bytes(),
                    parent_hgid(0),
                    parent_hgid(1),
                ));
                Ok((hgid, store::Flag::Directory))
            }
        }

        let mut executor = Executor::new(&self.store, &parent_trees)?;
        executor.work(&mut self.root, (0..parent_trees.len()).collect())?;
        Ok(executor.converted_nodes.into_iter())
    }

    pub fn list(&self, path: &RepoPath) -> Result<List> {
        let directory = match self.get_link(path)? {
            None => return Ok(List::NotFound),
            Some(Leaf(_)) => return Ok(List::File),
            Some(Ephemeral(content)) => content,
            Some(Durable(entry)) => entry.materialize_links(&self.store, path)?,
        };

        let directory = directory
            .into_iter()
            .map(|(key, value)| (key.to_owned(), value.to_fs_node()))
            .collect();

        Ok(List::Directory(directory))
    }

    fn get_link(&self, path: &RepoPath) -> Result<Option<&Link>> {
        let mut cursor = &self.root;
        for (parent, component) in path.parents().zip(path.components()) {
            let child = match cursor {
                Leaf(_) => return Ok(None),
                Ephemeral(links) => links.get(component),
                Durable(ref entry) => {
                    let links = entry.materialize_links(&self.store, parent)?;
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

#[derive(Clone, Debug, Ord, PartialOrd, Eq, PartialEq, Hash)]
pub enum List {
    NotFound,
    File,
    Directory(Vec<(PathComponentBuf, FsNode)>),
}

/// The purpose of this function is to provide compatible behavior with the C++ implementation
/// of the treemanifest. This function is problematic because it goes through abstraction
/// boundaries and is built with the assumption that the storage format is the same as the
/// in memory format that is the same as the wire format.
///
/// This function returns the nodes that need to be sent over the wire for a subtree of the
/// manifest to be fully hydrated. The subtree is represented by `path` and `hgid`. The data
/// that is present locally by the client is represented by `other_nodes`.
///
/// It is undefined what this function will do when called with a path that points to a file
/// or with nodes that don't make sense.
// NOTE: The implementation is currently custom. Consider converting the code to use Cursor.
// The suggestion received in code review was also to consider making the return type more
// simple (RepoPath, HgId) and letting the call sites deal with the Bytes.
pub fn compat_subtree_diff(
    store: Arc<dyn TreeStore + Send + Sync>,
    path: &RepoPath,
    hgid: HgId,
    other_nodes: Vec<HgId>,
    depth: i32,
) -> Result<Vec<(RepoPathBuf, HgId, Bytes)>> {
    struct State {
        store: InnerStore,
        path: RepoPathBuf,
        result: Vec<(RepoPathBuf, HgId, Bytes)>,
        depth_remaining: i32,
    }
    impl State {
        fn work(&mut self, hgid: HgId, other_nodes: Vec<HgId>) -> Result<()> {
            let entry = self.store.get_entry(&self.path, hgid)?;

            if self.depth_remaining > 0 {
                // TODO: optimize "other_nodes" construction
                // We use BTreeMap for convenience only, it is more efficient to use an array since
                // the entries are already sorted.
                let mut others_map = BTreeMap::new();
                for other_hgid in other_nodes {
                    let other_entry = self.store.get_entry(&self.path, other_hgid)?;
                    for other_element_result in other_entry.elements() {
                        let other_element = other_element_result?;
                        if other_element.flag == store::Flag::Directory {
                            others_map
                                .entry(other_element.component)
                                .or_insert(vec![])
                                .push(other_element.hgid);
                        }
                    }
                }
                for element_result in entry.elements() {
                    let element = element_result?;
                    if element.flag != store::Flag::Directory {
                        continue;
                    }
                    let mut others = others_map
                        .remove(&element.component)
                        .unwrap_or_else(|| vec![]);
                    if others.contains(&element.hgid) {
                        continue;
                    }
                    others.dedup();
                    self.path.push(element.component.as_ref());
                    self.depth_remaining -= 1;
                    self.work(element.hgid, others)?;
                    self.depth_remaining += 1;
                    self.path.pop();
                }
            }
            // NOTE: order in the result set matters for a lot of the integration tests
            self.result
                .push((self.path.clone(), hgid, entry.to_bytes()));
            Ok(())
        }
    }

    if other_nodes.contains(&hgid) {
        return Ok(vec![]);
    }

    let mut state = State {
        store: InnerStore::new(store),
        path: path.to_owned(),
        result: vec![],
        depth_remaining: depth - 1,
    };
    state.work(hgid, other_nodes)?;
    Ok(state.result)
}

/// Recursively prefetch the entire subtree under the given Key up to the given depth.
////
/// This serves as a client-driven alternative to the `gettreepack` wire protocol
/// command (wherein the server determines which missing tree nodes to send).
///
/// Determining which missing nodes to fetch on the client side, as this function does,
/// may be faster in some cases since any nodes that are already present on the client
/// will be by definition fast to access, whereas the server would effectively be forced
/// to fetch the desired tree and the base tree from its underlying datastore. This comes
/// at the expense of an increased number of network roundtrips to the server (specifically,
/// O(depth) requests will be sent serially), which may be problematic if there is high
/// network latency between the server and client. As such, this function's performance
/// relative to `gettreepack` is highly dependent on the situation in question.
pub fn prefetch(
    store: Arc<dyn TreeStore + Send + Sync>,
    key: Key,
    mut depth: Option<usize>,
) -> Result<()> {
    let tree = Tree::durable(store, key.hgid);
    let mut dirs = vec![Directory::from_link(&tree.root, key.path).unwrap()];

    while !dirs.is_empty() {
        let keys = dirs.iter().filter_map(|d| d.key()).collect::<Vec<_>>();
        if !keys.is_empty() {
            // Note that the prefetch() function is expected to filter out
            // keys that are already present in the client's cache.
            tree.store.prefetch(keys)?;
        }

        dirs = dirs
            .into_iter()
            .map(|d| Ok(d.list(&tree.store)?.1))
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .flatten()
            .collect();

        depth = match depth {
            Some(0) => break,
            Some(d) => Some(d - 1),
            None => None,
        };
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    use types::{hgid::NULL_ID, testutil::*};

    use self::{store::TestStore, testutil::*};
    use crate::FileType;

    #[test]
    fn test_insert() {
        let mut tree = Tree::ephemeral(Arc::new(TestStore::new()));
        tree.insert(repo_path_buf("foo/bar"), make_meta("10"))
            .unwrap();
        assert_eq!(
            tree.get_file(repo_path("foo/bar")).unwrap(),
            Some(make_meta("10"))
        );
        assert_eq!(tree.get_file(repo_path("baz")).unwrap(), None);

        tree.insert(repo_path_buf("baz"), make_meta("20")).unwrap();
        assert_eq!(
            tree.get_file(repo_path("foo/bar")).unwrap(),
            Some(make_meta("10"))
        );
        assert_eq!(
            tree.get_file(repo_path("baz")).unwrap(),
            Some(make_meta("20"))
        );

        tree.insert(repo_path_buf("foo/bat"), make_meta("30"))
            .unwrap();
        assert_eq!(
            tree.get_file(repo_path("foo/bat")).unwrap(),
            Some(make_meta("30"))
        );
        assert_eq!(
            tree.get_file(repo_path("foo/bar")).unwrap(),
            Some(make_meta("10"))
        );
        assert_eq!(
            tree.get_file(repo_path("baz")).unwrap(),
            Some(make_meta("20"))
        );

        assert_eq!(
            tree.insert(repo_path_buf("foo/bar/error"), make_meta("40"))
                .unwrap_err()
                .chain()
                .map(|e| format!("{}", e))
                .collect::<Vec<_>>(),
            vec![
                "failure inserting 'foo/bar/error' in manifest",
                "\'foo/bar\' is already a file",
            ],
        );
        assert_eq!(
            tree.insert(repo_path_buf("foo"), make_meta("50"))
                .unwrap_err()
                .chain()
                .map(|e| format!("{}", e))
                .collect::<Vec<_>>(),
            vec![
                "failure inserting 'foo' in manifest",
                "file path is already a directory",
            ],
        );
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
            .insert(RepoPath::empty(), hgid("1"), root_entry.to_bytes())
            .unwrap();
        let foo_entry = store::Entry::from_elements(vec![store_element(
            "bar",
            "11",
            store::Flag::File(FileType::Regular),
        )])
        .unwrap();
        store
            .insert(repo_path("foo"), hgid("10"), foo_entry.to_bytes())
            .unwrap();
        let mut tree = Tree::durable(Arc::new(store), hgid("1"));

        assert_eq!(
            tree.get_file(repo_path("foo/bar")).unwrap(),
            Some(make_meta("11"))
        );
        assert_eq!(
            tree.get_file(repo_path("baz")).unwrap(),
            Some(make_meta("20"))
        );

        tree.insert(repo_path_buf("foo/bat"), make_meta("12"))
            .unwrap();
        assert_eq!(
            tree.get_file(repo_path("foo/bat")).unwrap(),
            Some(make_meta("12"))
        );
        assert_eq!(
            tree.get_file(repo_path("foo/bar")).unwrap(),
            Some(make_meta("11"))
        );
        assert_eq!(
            tree.get_file(repo_path("baz")).unwrap(),
            Some(make_meta("20"))
        );
    }

    #[test]
    fn test_insert_into_directory() {
        let mut tree = Tree::ephemeral(Arc::new(TestStore::new()));
        tree.insert(repo_path_buf("foo/bar/baz"), make_meta("10"))
            .unwrap();
        assert!(tree
            .insert(repo_path_buf("foo/bar"), make_meta("20"))
            .is_err());
        assert!(tree.insert(repo_path_buf("foo"), make_meta("30")).is_err());
    }

    #[test]
    fn test_insert_with_file_parent() {
        let mut tree = Tree::ephemeral(Arc::new(TestStore::new()));
        tree.insert(repo_path_buf("foo"), make_meta("10")).unwrap();
        assert!(tree
            .insert(repo_path_buf("foo/bar"), make_meta("20"))
            .is_err());
        assert!(tree
            .insert(repo_path_buf("foo/bar/baz"), make_meta("30"))
            .is_err());
    }

    #[test]
    fn test_get_from_directory() {
        let mut tree = Tree::ephemeral(Arc::new(TestStore::new()));
        tree.insert(repo_path_buf("foo/bar/baz"), make_meta("10"))
            .unwrap();
        assert_eq!(
            tree.get(repo_path("foo/bar")).unwrap(),
            Some(FsNode::Directory(None))
        );
        assert_eq!(
            tree.get(repo_path("foo")).unwrap(),
            Some(FsNode::Directory(None))
        );
    }

    #[test]
    fn test_get_with_file_parent() {
        let mut tree = Tree::ephemeral(Arc::new(TestStore::new()));
        tree.insert(repo_path_buf("foo"), make_meta("10")).unwrap();
        assert_eq!(tree.get(repo_path("foo/bar")).unwrap(), None);
        assert_eq!(tree.get(repo_path("foo/bar/baz")).unwrap(), None);
    }

    #[test]
    fn test_remove_from_ephemeral() {
        let mut tree = Tree::ephemeral(Arc::new(TestStore::new()));
        tree.insert(repo_path_buf("a1/b1/c1/d1"), make_meta("10"))
            .unwrap();
        tree.insert(repo_path_buf("a1/b2"), make_meta("20"))
            .unwrap();
        tree.insert(repo_path_buf("a2/b2/c2"), make_meta("30"))
            .unwrap();

        assert_eq!(tree.remove(repo_path("a1")).unwrap(), None);
        assert_eq!(tree.remove(repo_path("a1/b1")).unwrap(), None);
        assert_eq!(tree.remove(repo_path("a1/b1/c1/d1/e1")).unwrap(), None);
        assert_eq!(
            tree.remove(repo_path("a1/b1/c1/d1")).unwrap(),
            Some(make_meta("10"))
        );
        assert_eq!(tree.remove(repo_path("a3")).unwrap(), None);
        assert_eq!(tree.remove(repo_path("a1/b3")).unwrap(), None);
        assert_eq!(tree.remove(repo_path("a1/b1/c1/d2")).unwrap(), None);
        assert_eq!(tree.remove(repo_path("a1/b1/c1/d1/e1")).unwrap(), None);
        assert_eq!(tree.remove(RepoPath::empty()).unwrap(), None);
        assert_eq!(tree.get(repo_path("a1/b1/c1/d1")).unwrap(), None);
        assert_eq!(tree.get(repo_path("a1/b1/c1")).unwrap(), None);
        assert_eq!(
            tree.get(repo_path("a1/b2")).unwrap(),
            Some(FsNode::File(make_meta("20")))
        );
        assert_eq!(
            tree.remove(repo_path("a1/b2")).unwrap(),
            Some(make_meta("20"))
        );
        assert_eq!(tree.get(repo_path("a1")).unwrap(), None);

        assert_eq!(
            tree.get(repo_path("a2/b2/c2")).unwrap(),
            Some(FsNode::File(make_meta("30")))
        );
        assert_eq!(
            tree.remove(repo_path("a2/b2/c2")).unwrap(),
            Some(make_meta("30"))
        );
        assert_eq!(tree.get(repo_path("a2")).unwrap(), None);

        assert_eq!(
            tree.get(RepoPath::empty()).unwrap(),
            Some(FsNode::Directory(None))
        );
    }

    #[test]
    fn test_remove_from_durable() {
        let store = TestStore::new();
        let root_entry = store::Entry::from_elements(vec![
            store_element("a1", "10", store::Flag::Directory),
            store_element("a2", "20", store::Flag::File(FileType::Regular)),
        ])
        .unwrap();
        let tree_hgid = hgid("1");
        store
            .insert(RepoPath::empty(), tree_hgid, root_entry.to_bytes())
            .unwrap();
        let a1_entry = store::Entry::from_elements(vec![
            store_element("b1", "11", store::Flag::File(FileType::Regular)),
            store_element("b2", "12", store::Flag::File(FileType::Regular)),
        ])
        .unwrap();
        store
            .insert(repo_path("a1"), hgid("10"), a1_entry.to_bytes())
            .unwrap();
        let mut tree = Tree::durable(Arc::new(store), tree_hgid);

        assert_eq!(
            tree.get(RepoPath::empty()).unwrap(),
            Some(FsNode::Directory(Some(tree_hgid)))
        );
        assert_eq!(tree.remove(repo_path("a1")).unwrap(), None);
        assert_eq!(
            tree.remove(repo_path("a1/b1")).unwrap(),
            Some(make_meta("11"))
        );
        assert_eq!(tree.get(repo_path("a1/b1")).unwrap(), None);
        assert_eq!(
            tree.get(repo_path("a1/b2")).unwrap(),
            Some(FsNode::File(make_meta("12")))
        );
        assert_eq!(
            tree.remove(repo_path("a1/b2")).unwrap(),
            Some(make_meta("12"))
        );
        assert_eq!(tree.get(repo_path("a1/b2")).unwrap(), None);
        assert_eq!(tree.get(repo_path("a1")).unwrap(), None);
        assert_eq!(tree.get_link(repo_path("a1")).unwrap(), None);

        assert_eq!(
            tree.get(repo_path("a2")).unwrap(),
            Some(FsNode::File(make_meta("20")))
        );
        assert_eq!(tree.remove(repo_path("a2")).unwrap(), Some(make_meta("20")));
        assert_eq!(tree.get(repo_path("a2")).unwrap(), None);
        assert_eq!(
            tree.get(RepoPath::empty()).unwrap(),
            Some(FsNode::Directory(None))
        );
    }

    #[test]
    fn test_flush() {
        let store = Arc::new(TestStore::new());
        let mut tree = Tree::ephemeral(store.clone());
        tree.insert(repo_path_buf("a1/b1/c1/d1"), make_meta("10"))
            .unwrap();
        tree.insert(repo_path_buf("a1/b2"), make_meta("20"))
            .unwrap();
        tree.insert(repo_path_buf("a2/b2/c2"), make_meta("30"))
            .unwrap();

        let hgid = tree.flush().unwrap();

        let tree = Tree::durable(store.clone(), hgid);
        assert_eq!(
            tree.get_file(repo_path("a1/b1/c1/d1")).unwrap(),
            Some(make_meta("10"))
        );
        assert_eq!(
            tree.get_file(repo_path("a1/b2")).unwrap(),
            Some(make_meta("20"))
        );
        assert_eq!(
            tree.get_file(repo_path("a2/b2/c2")).unwrap(),
            Some(make_meta("30"))
        );
        assert_eq!(tree.get(repo_path("a2/b1")).unwrap(), None);
    }

    #[test]
    fn test_finalize_with_zero_and_one_parents() {
        let store = Arc::new(TestStore::new());
        let mut tree = Tree::ephemeral(store.clone());
        tree.insert(repo_path_buf("a1/b1/c1/d1"), make_meta("10"))
            .unwrap();
        tree.insert(repo_path_buf("a1/b2"), make_meta("20"))
            .unwrap();
        tree.insert(repo_path_buf("a2/b2/c2"), make_meta("30"))
            .unwrap();
        let tree_changed: Vec<_> = tree.finalize(vec![]).unwrap().collect();

        assert_eq!(tree_changed.len(), 6);
        assert_eq!(tree_changed[0].0, repo_path_buf("a1/b1/c1"));
        assert_eq!(tree_changed[1].0, repo_path_buf("a1/b1"));
        assert_eq!(tree_changed[2].0, repo_path_buf("a1"));
        assert_eq!(tree_changed[3].0, repo_path_buf("a2/b2"));
        assert_eq!(tree_changed[4].0, repo_path_buf("a2"));
        assert_eq!(tree_changed[5].0, RepoPathBuf::new());

        // we should write before we can update
        // depends on the implementation but it is valid for finalize to query the store
        // for the values returned in the previous finalize call

        use bytes::Bytes;
        for (path, hgid, raw, _, _) in tree_changed.iter() {
            store.insert(&path, *hgid, Bytes::from(&raw[..])).unwrap();
        }

        let mut update = tree.clone();
        update
            .insert(repo_path_buf("a1/b2"), make_meta("40"))
            .unwrap();
        update.remove(repo_path("a2/b2/c2")).unwrap();
        update
            .insert(repo_path_buf("a3/b1"), make_meta("50"))
            .unwrap();
        let update_changed: Vec<_> = update.finalize(vec![&tree]).unwrap().collect();
        assert_eq!(update_changed[0].0, repo_path_buf("a1"));
        assert_eq!(update_changed[0].3, tree_changed[2].1);
        assert_eq!(update_changed[0].4, NULL_ID);
        assert_eq!(update_changed[1].0, repo_path_buf("a3"));
        assert_eq!(update_changed[1].3, NULL_ID);
        assert_eq!(update_changed[1].4, NULL_ID);
        assert_eq!(update_changed[2].0, RepoPathBuf::new());
        assert_eq!(update_changed[2].3, tree_changed[5].1);
        assert_eq!(update_changed[2].4, NULL_ID);
    }

    #[test]
    fn test_finalize_merge() {
        let store = Arc::new(TestStore::new());
        let mut p1 = Tree::ephemeral(store.clone());
        p1.insert(repo_path_buf("a1/b1/c1/d1"), make_meta("10"))
            .unwrap();
        p1.insert(repo_path_buf("a1/b2"), make_meta("20")).unwrap();
        p1.insert(repo_path_buf("a2/b2/c2"), make_meta("30"))
            .unwrap();
        let _p1_changed = p1.finalize(vec![]).unwrap();

        let mut p2 = Tree::ephemeral(store.clone());
        p2.insert(repo_path_buf("a1/b2"), make_meta("40")).unwrap();
        p2.insert(repo_path_buf("a3/b1"), make_meta("50")).unwrap();
        let _p2_changed = p2.finalize(vec![]).unwrap();

        let mut tree = p1.clone();
        tree.insert(repo_path_buf("a1/b2"), make_meta("40"))
            .unwrap();
        tree.insert(repo_path_buf("a2/b2/c2"), make_meta("60"))
            .unwrap();
        tree.insert(repo_path_buf("a3/b1"), make_meta("50"))
            .unwrap();
        let tree_changed: Vec<_> = tree.finalize(vec![&p1, &p2]).unwrap().collect();
        assert_eq!(tree_changed[0].0, repo_path_buf("a1"));
        assert_eq!(tree_changed[0].3, get_hgid(&p1, repo_path("a1")));
        assert_eq!(tree_changed[0].4, get_hgid(&p2, repo_path("a1")));

        assert_eq!(tree_changed[1].0, repo_path_buf("a2/b2"));
        assert_eq!(tree_changed[1].3, get_hgid(&p1, repo_path("a2/b2")));
        assert_eq!(tree_changed[1].4, NULL_ID);
        assert_eq!(tree_changed[2].0, repo_path_buf("a2"));
        assert_eq!(tree_changed[3].0, repo_path_buf("a3"));
        assert_eq!(tree_changed[3].3, get_hgid(&p2, repo_path("a3")));
        assert_eq!(tree_changed[3].4, NULL_ID);
        assert_eq!(tree_changed[4].0, RepoPathBuf::new());

        assert_eq!(
            vec![tree_changed[4].3, tree_changed[4].4],
            vec![
                get_hgid(&p1, RepoPath::empty()),
                get_hgid(&p2, RepoPath::empty()),
            ]
        );
    }

    #[test]
    fn test_finalize_file_to_directory() {
        let store = Arc::new(TestStore::new());
        let mut tree1 = Tree::ephemeral(store.clone());
        tree1.insert(repo_path_buf("a1"), make_meta("10")).unwrap();
        let tree1_changed: Vec<_> = tree1.finalize(vec![]).unwrap().collect();
        assert_eq!(tree1_changed[0].0, RepoPathBuf::new());
        assert_eq!(tree1_changed[0].3, NULL_ID);

        let mut tree2 = Tree::ephemeral(store.clone());
        tree2
            .insert(repo_path_buf("a1/b1"), make_meta("20"))
            .unwrap();
        let tree2_changed: Vec<_> = tree2.finalize(vec![&tree1]).unwrap().collect();
        assert_eq!(tree2_changed[0].0, repo_path_buf("a1"));
        assert_eq!(tree2_changed[0].3, NULL_ID);
        assert_eq!(tree2_changed[1].0, RepoPathBuf::new());
        assert_eq!(tree2_changed[1].3, tree1_changed[0].1);
        assert_eq!(tree2_changed[1].4, NULL_ID);

        let mut tree3 = Tree::ephemeral(store.clone());
        tree3.insert(repo_path_buf("a1"), make_meta("30")).unwrap();
        let tree3_changed: Vec<_> = tree3.finalize(vec![&tree2]).unwrap().collect();
        assert_eq!(tree3_changed[0].0, RepoPathBuf::new());
        assert_eq!(tree3_changed[0].3, tree2_changed[1].1);
        assert_eq!(tree3_changed[0].4, NULL_ID);
    }

    #[test]
    fn test_finalize_on_durable() {
        let store = Arc::new(TestStore::new());
        let mut tree1 = Tree::ephemeral(store.clone());
        tree1
            .insert(repo_path_buf("a1/b1/c1/d1"), make_meta("10"))
            .unwrap();
        tree1
            .insert(repo_path_buf("a1/b2"), make_meta("20"))
            .unwrap();
        tree1
            .insert(repo_path_buf("a2/b2/c2"), make_meta("30"))
            .unwrap();
        let _tree1_changed = tree1.finalize(vec![]).unwrap();

        let mut tree2 = tree1.clone();
        tree2
            .insert(repo_path_buf("a1/b2"), make_meta("40"))
            .unwrap();
        tree2
            .insert(repo_path_buf("a2/b2/c2"), make_meta("60"))
            .unwrap();
        tree2
            .insert(repo_path_buf("a3/b1"), make_meta("50"))
            .unwrap();
        let tree_changed: Vec<_> = tree2.finalize(vec![&tree1]).unwrap().collect();
        assert_eq!(
            tree2.finalize(vec![&tree1]).unwrap().collect::<Vec<_>>(),
            tree_changed,
        );
    }

    #[test]
    fn test_finalize_materialization() {
        let store = Arc::new(TestStore::new());
        let entry_1 = store::Entry::from_elements(vec![
            store_element("foo", "10", store::Flag::Directory),
            store_element("baz", "20", store::Flag::File(FileType::Regular)),
        ])
        .unwrap();
        store
            .insert(RepoPath::empty(), hgid("1"), entry_1.to_bytes())
            .unwrap();
        let parent = Tree::durable(store.clone(), hgid("1"));

        let entry_2 = store::Entry::from_elements(vec![
            store_element("foo", "10", store::Flag::Directory),
            store_element("baz", "21", store::Flag::File(FileType::Regular)),
        ])
        .unwrap();
        store
            .insert(RepoPath::empty(), hgid("2"), entry_2.to_bytes())
            .unwrap();

        let mut tree = Tree::durable(store.clone(), hgid("2"));

        let _changes: Vec<_> = tree.finalize(vec![&parent]).unwrap().collect();
        // expecting the code to not panic
        // the panic would be caused by materializing link (foo, 10) which
        // doesn't have a store entry
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
        tree.insert(repo_path_buf("a1"), make_meta("10")).unwrap();
        tree.insert(repo_path_buf("a2/b2"), make_meta("20"))
            .unwrap();
        tree.insert(repo_path_buf("a3"), make_meta("30")).unwrap();

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
    fn test_debug() {
        use std::fmt::Write;

        let store = Arc::new(TestStore::new());
        let mut tree = Tree::ephemeral(store.clone());
        tree.insert(repo_path_buf("a1/b1/c1/d1"), make_meta("10"))
            .unwrap();
        let _hgid = tree.flush().unwrap();

        tree.insert(repo_path_buf("a1/b2"), make_meta("20"))
            .unwrap();
        tree.insert(repo_path_buf("a2/b2/c2"), make_meta("30"))
            .unwrap();

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

    #[test]
    fn test_compat_subtree_diff() {
        let store = Arc::new(TestStore::new());
        // add ("", 1), ("foo", 11), ("baz", 21), ("foo/bar", 111)
        let root_1_entry = store::Entry::from_elements(vec![
            store_element("foo", "11", store::Flag::Directory),
            store_element("baz", "21", store::Flag::File(FileType::Regular)),
        ])
        .unwrap();
        store
            .insert(
                RepoPath::empty(),
                hgid("1"),
                root_1_entry.clone().to_bytes(),
            )
            .unwrap();
        let foo_11_entry = store::Entry::from_elements(vec![store_element(
            "bar",
            "111",
            store::Flag::File(FileType::Regular),
        )])
        .unwrap();
        store
            .insert(
                repo_path("foo"),
                hgid("11"),
                foo_11_entry.clone().to_bytes(),
            )
            .unwrap();

        // add ("", 2), ("foo", 12), ("baz", 21), ("foo/bar", 112)
        let root_2_entry = store::Entry::from_elements(vec![
            store_element("foo", "12", store::Flag::Directory),
            store_element("baz", "21", store::Flag::File(FileType::Regular)),
        ])
        .unwrap();
        store
            .insert(RepoPath::empty(), hgid("2"), root_2_entry.to_bytes())
            .unwrap();
        let foo_12_entry = store::Entry::from_elements(vec![store_element(
            "bar",
            "112",
            store::Flag::File(FileType::Regular),
        )])
        .unwrap();
        store
            .insert(repo_path("foo"), hgid("12"), foo_12_entry.to_bytes())
            .unwrap();

        assert_eq!(
            compat_subtree_diff(
                store.clone(),
                RepoPath::empty(),
                hgid("1"),
                vec![hgid("2")],
                3
            )
            .unwrap(),
            vec![
                (
                    repo_path_buf("foo"),
                    hgid("11"),
                    foo_11_entry.clone().to_bytes()
                ),
                (
                    RepoPathBuf::new(),
                    hgid("1"),
                    root_1_entry.clone().to_bytes()
                ),
            ]
        );
        assert_eq!(
            compat_subtree_diff(
                store.clone(),
                RepoPath::empty(),
                hgid("1"),
                vec![hgid("2")],
                1
            )
            .unwrap(),
            vec![(
                RepoPathBuf::new(),
                hgid("1"),
                root_1_entry.clone().to_bytes()
            ),]
        );
        assert_eq!(
            compat_subtree_diff(
                store.clone(),
                repo_path("foo"),
                hgid("11"),
                vec![hgid("12")],
                3
            )
            .unwrap(),
            vec![(
                repo_path_buf("foo"),
                hgid("11"),
                foo_11_entry.clone().to_bytes()
            ),]
        );
        assert_eq!(
            compat_subtree_diff(
                store.clone(),
                RepoPath::empty(),
                hgid("1"),
                vec![hgid("1")],
                3
            )
            .unwrap(),
            vec![]
        );
        assert_eq!(
            compat_subtree_diff(
                store.clone(),
                repo_path("foo"),
                hgid("11"),
                vec![hgid("11")],
                3
            )
            .unwrap(),
            vec![]
        );
        // it is illegal to call compat_subtree_diff with "baz" but we can't validate for it
    }

    #[test]
    fn test_compat_subtree_diff_file_to_directory() {
        let store = Arc::new(TestStore::new());
        // add ("", 1), ("foo", 11)
        let root_1_entry = store::Entry::from_elements(vec![store_element(
            "foo",
            "11",
            store::Flag::File(FileType::Regular),
        )])
        .unwrap();
        store
            .insert(
                RepoPath::empty(),
                hgid("1"),
                root_1_entry.clone().to_bytes(),
            )
            .unwrap();

        // add ("", 2), ("foo", 12), ("foo/bar", 121)
        let root_2_entry =
            store::Entry::from_elements(vec![store_element("foo", "12", store::Flag::Directory)])
                .unwrap();
        store
            .insert(
                RepoPath::empty(),
                hgid("2"),
                root_2_entry.clone().to_bytes(),
            )
            .unwrap();
        let foo_12_entry = store::Entry::from_elements(vec![store_element(
            "bar",
            "121",
            store::Flag::File(FileType::Regular),
        )])
        .unwrap();
        store
            .insert(
                repo_path("foo"),
                hgid("12"),
                foo_12_entry.clone().to_bytes(),
            )
            .unwrap();

        assert_eq!(
            compat_subtree_diff(
                store.clone(),
                RepoPath::empty(),
                hgid("2"),
                vec![hgid("1")],
                3
            )
            .unwrap(),
            vec![
                (
                    repo_path_buf("foo"),
                    hgid("12"),
                    foo_12_entry.clone().to_bytes()
                ),
                (
                    RepoPathBuf::new(),
                    hgid("2"),
                    root_2_entry.clone().to_bytes()
                ),
            ]
        );
    }

    #[test]
    fn test_list() {
        let mut tree = Tree::ephemeral(Arc::new(TestStore::new()));
        let c1_meta = make_meta("10");
        tree.insert(repo_path_buf("a1/b1/c1"), c1_meta).unwrap();
        let b2_meta = make_meta("20");
        tree.insert(repo_path_buf("a1/b2"), b2_meta).unwrap();
        let _hgid = tree.flush().unwrap();
        let c2_meta = make_meta("30");
        tree.insert(repo_path_buf("a2/b3/c2"), c2_meta).unwrap();
        let b4_meta = make_meta("40");
        tree.insert(repo_path_buf("a2/b4"), b4_meta).unwrap();

        assert_eq!(tree.list(repo_path("not_found")).unwrap(), List::NotFound);
        assert_eq!(tree.list(repo_path("a1/b1/c1")).unwrap(), List::File);
        assert_eq!(
            tree.list(repo_path("a1/b1")).unwrap(),
            List::Directory(vec![(path_component_buf("c1"), FsNode::File(c1_meta))]),
        );
        assert_eq!(
            tree.list(repo_path("a1")).unwrap(),
            List::Directory(vec![
                (
                    path_component_buf("b1"),
                    tree.get(repo_path("a1/b1")).unwrap().unwrap()
                ),
                (path_component_buf("b2"), FsNode::File(b2_meta)),
            ]),
        );
        assert_eq!(tree.list(repo_path("a2/b3/c2")).unwrap(), List::File);
        assert_eq!(
            tree.list(repo_path("a2/b3")).unwrap(),
            List::Directory(vec![(path_component_buf("c2"), FsNode::File(c2_meta))]),
        );
        assert_eq!(
            tree.list(repo_path("a2")).unwrap(),
            List::Directory(vec![
                (path_component_buf("b3"), FsNode::Directory(None)),
                (path_component_buf("b4"), FsNode::File(b4_meta)),
            ]),
        );
        assert_eq!(
            tree.list(RepoPath::empty()).unwrap(),
            List::Directory(vec![
                (
                    path_component_buf("a1"),
                    tree.get(repo_path("a1")).unwrap().unwrap()
                ),
                (path_component_buf("a2"), FsNode::Directory(None)),
            ]),
        );
    }
}
