/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod diff;
mod iter;
mod link;
mod namecmp;
mod store;
#[cfg(any(test, feature = "for-tests"))]
pub mod testutil;

use std::collections::btree_map::Entry;
use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

use anyhow::Result;
use manifest::DiffEntry;
use manifest::DirDiffEntry;
use manifest::Directory;
use manifest::File;
use manifest::FileMetadata;
use manifest::FsNodeMetadata;
use manifest::List;
pub use manifest::Manifest;
use minibytes::Bytes;
use once_cell::sync::OnceCell;
use pathmatcher::Matcher;
use sha1::Digest;
use sha1::Sha1;
pub use store::Flag;
use storemodel::TreeFormat;
use thiserror::Error;
use types::HgId;
use types::Key;
use types::PathComponent;
use types::PathComponentBuf;
use types::RepoPath;
use types::RepoPathBuf;

pub use self::diff::Diff;
pub(crate) use self::link::Link;
pub use self::store::Entry as TreeEntry;
pub use self::store::TreeStore;
use crate::iter::BfsIter;
use crate::iter::DfsCursor;
use crate::iter::Step;
use crate::link::DirLink;
use crate::link::Durable;
use crate::link::DurableEntry;
use crate::link::Ephemeral;
use crate::link::Leaf;
use crate::store::InnerStore;

/// The Tree implementation of a Manifest dedicates an inner node for each directory in the
/// repository and a leaf for each file.
#[derive(Clone)]
pub struct TreeManifest {
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

impl TreeManifest {
    /// Instantiates a tree manifest that was stored with the specificed `HgId`
    pub fn durable(store: Arc<dyn TreeStore + Send + Sync>, hgid: HgId) -> Self {
        TreeManifest {
            store: InnerStore::new(store),
            root: Link::durable(hgid),
        }
    }

    /// Instantiates a new tree manifest with no history
    pub fn ephemeral(store: Arc<dyn TreeStore + Send + Sync>) -> Self {
        TreeManifest {
            store: InnerStore::new(store),
            root: Link::ephemeral(),
        }
    }

    fn root_cursor<'a>(&'a self) -> DfsCursor<'a> {
        DfsCursor::new(&self.store, RepoPathBuf::new(), &self.root)
    }
}

impl Manifest for TreeManifest {
    fn get(&self, path: &RepoPath) -> Result<Option<FsNodeMetadata>> {
        let result = self.get_link(path)?.map(|link| link.to_fs_node());
        Ok(result)
    }

    fn list(&self, path: &RepoPath) -> Result<List> {
        let directory = match self.get_link(path)? {
            None => return Ok(List::NotFound),
            Some(l) => match l.as_ref() {
                Leaf(_) => return Ok(List::File),
                Ephemeral(content) => content,
                Durable(entry) => entry.materialize_links(&self.store, path)?,
            },
        };

        let directory = directory
            .into_iter()
            .map(|(key, value)| (key.to_owned(), value.to_fs_node()))
            .collect();

        Ok(List::Directory(directory))
    }

    fn insert(&mut self, path: RepoPathBuf, file_metadata: FileMetadata) -> Result<()> {
        let mut cursor = &self.root;
        let mut must_insert = false;
        for (parent, component) in path.parents().zip(path.components()) {
            let child = match cursor.as_ref() {
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
            match cursor.as_ref() {
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
                .or_insert_with(|| Link::ephemeral());
        }
        match cursor
            .mut_ephemeral_links(&self.store, path_parent)?
            .entry(last_component.to_owned())
        {
            Entry::Vacant(entry) => {
                entry.insert(Link::leaf(file_metadata));
            }
            Entry::Occupied(mut entry) => {
                if let Leaf(ref mut store_ref) = entry.get_mut().as_mut_ref()? {
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
                    if let Leaf(_) = cursor.as_ref() {
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

    /// Write dirty trees using specified format to disk. Return the root tree id.
    fn flush(&mut self) -> Result<HgId> {
        fn compute_sha1(content: &[u8], format: TreeFormat) -> HgId {
            let mut hasher = Sha1::new();
            match format {
                TreeFormat::Git => hasher.update(format!("tree {}\0", content.len())),
                TreeFormat::Hg => {
                    // XXX: No p1, p2 to produce a genuine SHA1.
                    // This code path is only meaningful for tests.
                    assert!(
                        cfg!(test),
                        "flush() cannot be used with hg store, consider finalize() instead"
                    );
                }
            }
            hasher.update(content);
            let buf: [u8; HgId::len()] = hasher.finalize().into();
            (&buf).into()
        }
        fn do_flush<'a, 'b, 'c>(
            store: &'a InnerStore,
            pathbuf: &'b mut RepoPathBuf,
            cursor: &'c mut Link,
            format: TreeFormat,
        ) -> Result<(HgId, store::Flag)> {
            loop {
                let new_cursor = match cursor.as_mut_ref()? {
                    Leaf(file_metadata) => {
                        return Ok((
                            file_metadata.hgid.clone(),
                            store::Flag::File(file_metadata.file_type.clone()),
                        ));
                    }
                    Durable(entry) => return Ok((entry.hgid.clone(), store::Flag::Directory)),
                    Ephemeral(links) => {
                        let iter = links.iter_mut().map(|(component, link)| {
                            pathbuf.push(component.as_path_component());
                            let (hgid, flag) = do_flush(store, pathbuf, link, format)?;
                            pathbuf.pop();
                            Ok(store::Element::new(
                                component.to_owned(),
                                hgid.clone(),
                                flag,
                            ))
                        });
                        let elements: Vec<_> = iter.collect::<Result<Vec<_>>>()?;
                        let entry = store::Entry::from_elements(elements, format);
                        let hgid = compute_sha1(entry.as_ref(), format);
                        store.insert_entry(&pathbuf, hgid, entry)?;

                        let cell = OnceCell::new();
                        // TODO: remove clone
                        cell.set(links.clone()).unwrap();

                        let durable_entry = DurableEntry { hgid, links: cell };
                        Link::new(Durable(Arc::new(durable_entry)))
                    }
                };
                *cursor = new_cursor;
            }
        }
        let mut path = RepoPathBuf::new();
        let format = self.store.format();
        let (hgid, _) = do_flush(&self.store, &mut path, &mut self.root, format)?;
        Ok(hgid)
    }

    fn files<'a, M: 'static + Matcher + Sync + Send>(
        &'a self,
        matcher: M,
    ) -> Box<dyn Iterator<Item = Result<File>> + 'a> {
        let files = BfsIter::new(&self, matcher).filter_map(|result| match result {
            Ok((path, FsNodeMetadata::File(metadata))) => Some(Ok(File::new(path, metadata))),
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
    fn dirs<'a, M: 'static + Matcher + Sync + Send>(
        &'a self,
        matcher: M,
    ) -> Box<dyn Iterator<Item = Result<Directory>> + 'a> {
        let dirs = BfsIter::new(&self, matcher).filter_map(|result| match result {
            Ok((path, FsNodeMetadata::Directory(metadata))) => {
                Some(Ok(Directory::new(path, metadata)))
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
    ) -> Result<Box<dyn Iterator<Item = Result<DiffEntry>> + 'a>> {
        Ok(Box::new(Diff::new(self, other, matcher)?))
    }

    fn modified_dirs<'a, M: Matcher>(
        &'a self,
        other: &'a Self,
        matcher: &'a M,
    ) -> Result<Box<dyn Iterator<Item = Result<DirDiffEntry>> + 'a>> {
        Ok(Box::new(Diff::new(self, other, matcher)?.modified_dirs()))
    }
}

impl fmt::Debug for TreeManifest {
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
            match link.as_ref() {
                Leaf(metadata) => {
                    write!(f, "(File, {}, {:?})\n", metadata.hgid, metadata.file_type)
                }
                Ephemeral(children) => {
                    write!(f, "(Ephemeral)\n")?;
                    write_children(f, children, indent)
                }
                Durable(entry) => {
                    write!(f, "(Durable, {})\n", entry.hgid)?;
                    match entry.links.get() {
                        None => Ok(()),
                        Some(children) => write_children(f, children, indent),
                    }
                }
            }
        }
        write!(f, "Root ")?;
        write_links(f, &self.root, 1)
    }
}

impl TreeManifest {
    /// Produces new trees to write in hg format (path, id, text, p1, p2).
    /// Does not write to the tree store directly.
    pub fn finalize(
        &mut self,
        parent_trees: Vec<&TreeManifest>,
    ) -> Result<impl Iterator<Item = (RepoPathBuf, HgId, Bytes, HgId, HgId)>> {
        fn compute_hgid<C: AsRef<[u8]>>(parent_tree_nodes: &[HgId], content: C) -> HgId {
            let mut hasher = Sha1::new();
            debug_assert!(parent_tree_nodes.len() <= 2);
            let p1 = parent_tree_nodes.get(0).unwrap_or(HgId::null_id());
            let p2 = parent_tree_nodes.get(1).unwrap_or(HgId::null_id());
            // Even if parents are sorted two hashes go into hash computation but surprise
            // the NULL_ID is not a special case in this case and gets sorted.
            if p1 < p2 {
                hasher.update(p1.as_ref());
                hasher.update(p2.as_ref());
            } else {
                hasher.update(p2.as_ref());
                hasher.update(p1.as_ref());
            }
            hasher.update(content.as_ref());
            let buf: [u8; HgId::len()] = hasher.finalize().into();
            (&buf).into()
        }
        struct Executor<'a> {
            store: &'a InnerStore,
            path: RepoPathBuf,
            converted_nodes: Vec<(RepoPathBuf, HgId, Bytes, HgId, HgId)>,
            parent_trees: Vec<DfsCursor<'a>>,
        }
        impl<'a> Executor<'a> {
            fn new(
                store: &'a InnerStore,
                parent_trees: &[&'a TreeManifest],
            ) -> Result<Executor<'a>> {
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
                        Step::Success | Step::End => {}
                        Step::Err(err) => return Err(err),
                    }
                }
                Ok(executor)
            }
            fn active_parent_tree_nodes(&self, active_parents: &[usize]) -> Result<Vec<HgId>> {
                let mut parent_nodes = Vec::with_capacity(active_parents.len());
                for id in active_parents {
                    let cursor = &self.parent_trees[*id];
                    let hgid = match cursor.link().as_ref() {
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
                        Step::Success | Step::End => {}
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
                            Step::Success | Step::End => {}
                            Step::Err(err) => return Err(err),
                        }
                    }
                    if !cursor.finished() && cursor.path() == self.path.as_repo_path() {
                        match cursor.link().as_ref() {
                            Leaf(_) => {} // files and directories don't share history
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
                if let Durable(entry) = link.as_ref() {
                    if parent_tree_nodes.contains(&entry.hgid) {
                        return Ok((entry.hgid, store::Flag::Directory));
                    }
                }
                self.advance_parents(&active_parents)?;
                if let Leaf(file_metadata) = link.as_ref() {
                    return Ok((
                        file_metadata.hgid,
                        store::Flag::File(file_metadata.file_type.clone()),
                    ));
                }
                // TODO: This code is also used on durable nodes for the purpose of generating
                // a list of entries to insert in the local store. For those cases we don't
                // need to convert to Ephemeral instead only verify the hash.
                let links = link.mut_ephemeral_links(self.store, &self.path)?;
                // finalize() is only used for hg format.
                let format = TreeFormat::Hg;
                let mut entry = store::EntryMut::new(format);
                for (component, link) in links.iter_mut() {
                    self.path.push(component.as_path_component());
                    let child_parents = self.parent_trees_for_subdirectory(&active_parents)?;
                    let (hgid, flag) = self.work(link, child_parents)?;
                    self.path.pop();
                    let element = store::Element::new(component.clone(), hgid, flag);
                    entry.add_element_hg(element);
                }
                let entry = entry.freeze();
                let hgid = compute_hgid(&parent_tree_nodes, &entry);

                let cell = OnceCell::new();
                // TODO: remove clone
                cell.set(links.clone()).unwrap();

                let durable_entry = DurableEntry { hgid, links: cell };
                let inner = Arc::new(durable_entry);
                *link = Link::new(Durable(inner));
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

        assert_eq!(
            self.store.format(),
            TreeFormat::Hg,
            "finalize() can only be used for hg store, use flush() instead"
        );
        let mut executor = Executor::new(&self.store, &parent_trees)?;
        executor.work(&mut self.root, (0..parent_trees.len()).collect())?;
        Ok(executor.converted_nodes.into_iter())
    }

    fn get_link(&self, path: &RepoPath) -> Result<Option<&Link>> {
        let mut cursor = &self.root;
        for (parent, component) in path.parents().zip(path.components()) {
            let child = match cursor.as_ref() {
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
) -> Result<Vec<(RepoPathBuf, HgId, Vec<HgId>, Bytes)>> {
    struct State {
        store: InnerStore,
        path: RepoPathBuf,
        result: Vec<(RepoPathBuf, HgId, Vec<HgId>, Bytes)>,
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
                for other_hgid in other_nodes.clone() {
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
                .push((self.path.clone(), hgid, other_nodes, entry.to_bytes()));
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
    let tree = TreeManifest::durable(store, key.hgid);
    let mut dirs = vec![DirLink::from_link(&tree.root, key.path).unwrap()];

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
    use manifest::testutil::*;
    use manifest::FileType;
    use store::Element;
    use types::hgid::NULL_ID;
    use types::testutil::*;

    use self::testutil::*;
    use super::*;

    impl store::Entry {
        fn from_elements_hg(elements: Vec<Element>) -> Self {
            Self::from_elements(elements, TreeFormat::Hg)
        }
    }
    fn store_element(path: &str, hex: &str, flag: store::Flag) -> store::Element {
        store::Element::new(path_component_buf(path), hgid(hex), flag)
    }

    fn get_hgid(tree: &TreeManifest, path: &RepoPath) -> HgId {
        match tree.get_link(path).unwrap().unwrap().as_ref() {
            Leaf(file_metadata) => file_metadata.hgid,
            Durable(ref entry) => entry.hgid,
            Ephemeral(_) => {
                panic!("Asked for hgid on path {} but found ephemeral hgid.", path)
            }
        }
    }

    #[test]
    fn test_insert() {
        let mut tree = TreeManifest::ephemeral(Arc::new(TestStore::new()));
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
        let root_entry = store::Entry::from_elements_hg(vec![
            store_element("foo", "10", store::Flag::Directory),
            store_element("baz", "20", store::Flag::File(FileType::Regular)),
        ]);
        store
            .insert(RepoPath::empty(), hgid("1"), root_entry.to_bytes())
            .unwrap();
        let foo_entry = store::Entry::from_elements_hg(vec![store_element(
            "bar",
            "11",
            store::Flag::File(FileType::Regular),
        )]);
        store
            .insert(repo_path("foo"), hgid("10"), foo_entry.to_bytes())
            .unwrap();
        let mut tree = TreeManifest::durable(Arc::new(store), hgid("1"));

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
        let mut tree = TreeManifest::ephemeral(Arc::new(TestStore::new()));
        tree.insert(repo_path_buf("foo/bar/baz"), make_meta("10"))
            .unwrap();
        assert!(
            tree.insert(repo_path_buf("foo/bar"), make_meta("20"))
                .is_err()
        );
        assert!(tree.insert(repo_path_buf("foo"), make_meta("30")).is_err());
    }

    #[test]
    fn test_insert_with_file_parent() {
        let mut tree = TreeManifest::ephemeral(Arc::new(TestStore::new()));
        tree.insert(repo_path_buf("foo"), make_meta("10")).unwrap();
        assert!(
            tree.insert(repo_path_buf("foo/bar"), make_meta("20"))
                .is_err()
        );
        assert!(
            tree.insert(repo_path_buf("foo/bar/baz"), make_meta("30"))
                .is_err()
        );
    }

    #[test]
    fn test_get_from_directory() {
        let mut tree = TreeManifest::ephemeral(Arc::new(TestStore::new()));
        tree.insert(repo_path_buf("foo/bar/baz"), make_meta("10"))
            .unwrap();
        assert_eq!(
            tree.get(repo_path("foo/bar")).unwrap(),
            Some(FsNodeMetadata::Directory(None))
        );
        assert_eq!(
            tree.get(repo_path("foo")).unwrap(),
            Some(FsNodeMetadata::Directory(None))
        );
    }

    #[test]
    fn test_get_with_file_parent() {
        let mut tree = TreeManifest::ephemeral(Arc::new(TestStore::new()));
        tree.insert(repo_path_buf("foo"), make_meta("10")).unwrap();
        assert_eq!(tree.get(repo_path("foo/bar")).unwrap(), None);
        assert_eq!(tree.get(repo_path("foo/bar/baz")).unwrap(), None);
    }

    #[test]
    fn test_remove_from_ephemeral() {
        let mut tree = TreeManifest::ephemeral(Arc::new(TestStore::new()));
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
            Some(FsNodeMetadata::File(make_meta("20")))
        );
        assert_eq!(
            tree.remove(repo_path("a1/b2")).unwrap(),
            Some(make_meta("20"))
        );
        assert_eq!(tree.get(repo_path("a1")).unwrap(), None);

        assert_eq!(
            tree.get(repo_path("a2/b2/c2")).unwrap(),
            Some(FsNodeMetadata::File(make_meta("30")))
        );
        assert_eq!(
            tree.remove(repo_path("a2/b2/c2")).unwrap(),
            Some(make_meta("30"))
        );
        assert_eq!(tree.get(repo_path("a2")).unwrap(), None);

        assert_eq!(
            tree.get(RepoPath::empty()).unwrap(),
            Some(FsNodeMetadata::Directory(None))
        );
    }

    #[test]
    fn test_remove_from_durable() {
        let store = TestStore::new();
        let root_entry = store::Entry::from_elements_hg(vec![
            store_element("a1", "10", store::Flag::Directory),
            store_element("a2", "20", store::Flag::File(FileType::Regular)),
        ]);
        let tree_hgid = hgid("1");
        store
            .insert(RepoPath::empty(), tree_hgid, root_entry.to_bytes())
            .unwrap();
        let a1_entry = store::Entry::from_elements_hg(vec![
            store_element("b1", "11", store::Flag::File(FileType::Regular)),
            store_element("b2", "12", store::Flag::File(FileType::Regular)),
        ]);
        store
            .insert(repo_path("a1"), hgid("10"), a1_entry.to_bytes())
            .unwrap();
        let mut tree = TreeManifest::durable(Arc::new(store), tree_hgid);

        assert_eq!(
            tree.get(RepoPath::empty()).unwrap(),
            Some(FsNodeMetadata::Directory(Some(tree_hgid)))
        );
        assert_eq!(tree.remove(repo_path("a1")).unwrap(), None);
        assert_eq!(
            tree.remove(repo_path("a1/b1")).unwrap(),
            Some(make_meta("11"))
        );
        assert_eq!(tree.get(repo_path("a1/b1")).unwrap(), None);
        assert_eq!(
            tree.get(repo_path("a1/b2")).unwrap(),
            Some(FsNodeMetadata::File(make_meta("12")))
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
            Some(FsNodeMetadata::File(make_meta("20")))
        );
        assert_eq!(tree.remove(repo_path("a2")).unwrap(), Some(make_meta("20")));
        assert_eq!(tree.get(repo_path("a2")).unwrap(), None);
        assert_eq!(
            tree.get(RepoPath::empty()).unwrap(),
            Some(FsNodeMetadata::Directory(None))
        );
    }

    #[test]
    fn test_flush() {
        let store = Arc::new(TestStore::new());
        let mut tree = TreeManifest::ephemeral(store.clone());
        tree.insert(repo_path_buf("a1/b1/c1/d1"), make_meta("10"))
            .unwrap();
        tree.insert(repo_path_buf("a1/b2"), make_meta("20"))
            .unwrap();
        tree.insert(repo_path_buf("a2/b2/c2"), make_meta("30"))
            .unwrap();

        let hgid = tree.flush().unwrap();

        let tree = TreeManifest::durable(store.clone(), hgid);
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
        let mut tree = TreeManifest::ephemeral(store.clone());
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

        use minibytes::Bytes;
        for (path, hgid, raw, _, _) in tree_changed.iter() {
            store
                .insert(&path, *hgid, Bytes::copy_from_slice(&raw[..]))
                .unwrap();
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
        let mut p1 = TreeManifest::ephemeral(store.clone());
        p1.insert(repo_path_buf("a1/b1/c1/d1"), make_meta("10"))
            .unwrap();
        p1.insert(repo_path_buf("a1/b2"), make_meta("20")).unwrap();
        p1.insert(repo_path_buf("a2/b2/c2"), make_meta("30"))
            .unwrap();
        let _p1_changed = p1.finalize(vec![]).unwrap();

        let mut p2 = TreeManifest::ephemeral(store.clone());
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
        let mut tree1 = TreeManifest::ephemeral(store.clone());
        tree1.insert(repo_path_buf("a1"), make_meta("10")).unwrap();
        let tree1_changed: Vec<_> = tree1.finalize(vec![]).unwrap().collect();
        assert_eq!(tree1_changed[0].0, RepoPathBuf::new());
        assert_eq!(tree1_changed[0].3, NULL_ID);

        let mut tree2 = TreeManifest::ephemeral(store.clone());
        tree2
            .insert(repo_path_buf("a1/b1"), make_meta("20"))
            .unwrap();
        let tree2_changed: Vec<_> = tree2.finalize(vec![&tree1]).unwrap().collect();
        assert_eq!(tree2_changed[0].0, repo_path_buf("a1"));
        assert_eq!(tree2_changed[0].3, NULL_ID);
        assert_eq!(tree2_changed[1].0, RepoPathBuf::new());
        assert_eq!(tree2_changed[1].3, tree1_changed[0].1);
        assert_eq!(tree2_changed[1].4, NULL_ID);

        let mut tree3 = TreeManifest::ephemeral(store.clone());
        tree3.insert(repo_path_buf("a1"), make_meta("30")).unwrap();
        let tree3_changed: Vec<_> = tree3.finalize(vec![&tree2]).unwrap().collect();
        assert_eq!(tree3_changed[0].0, RepoPathBuf::new());
        assert_eq!(tree3_changed[0].3, tree2_changed[1].1);
        assert_eq!(tree3_changed[0].4, NULL_ID);
    }

    #[test]
    fn test_finalize_on_durable() {
        let store = Arc::new(TestStore::new());
        let mut tree1 = TreeManifest::ephemeral(store.clone());
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
        let entry_1 = store::Entry::from_elements_hg(vec![
            store_element("foo", "10", store::Flag::Directory),
            store_element("baz", "20", store::Flag::File(FileType::Regular)),
        ]);
        store
            .insert(RepoPath::empty(), hgid("1"), entry_1.to_bytes())
            .unwrap();
        let parent = TreeManifest::durable(store.clone(), hgid("1"));

        let entry_2 = store::Entry::from_elements_hg(vec![
            store_element("foo", "10", store::Flag::Directory),
            store_element("baz", "21", store::Flag::File(FileType::Regular)),
        ]);
        store
            .insert(RepoPath::empty(), hgid("2"), entry_2.to_bytes())
            .unwrap();

        let mut tree = TreeManifest::durable(store.clone(), hgid("2"));

        let _changes: Vec<_> = tree.finalize(vec![&parent]).unwrap().collect();
        // expecting the code to not panic
        // the panic would be caused by materializing link (foo, 10) which
        // doesn't have a store entry
    }

    #[test]
    fn test_cursor_skip_on_root() {
        let tree = TreeManifest::ephemeral(Arc::new(TestStore::new()));
        let mut cursor = tree.root_cursor();
        cursor.skip_subtree();
        match cursor.step() {
            Step::Success => panic!("should have reached the end of the tree"),
            Step::End => {} // success
            Step::Err(error) => panic!("{}", error),
        }
    }

    #[test]
    fn test_cursor_skip() {
        fn step<'a>(cursor: &mut DfsCursor<'a>) {
            match cursor.step() {
                Step::Success => {}
                Step::End => panic!("reached the end too soon"),
                Step::Err(error) => panic!("{}", error),
            }
        }
        let mut tree = TreeManifest::ephemeral(Arc::new(TestStore::new()));
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
            Step::End => {} // success
            Step::Err(error) => panic!("{}", error),
        }
    }

    #[test]
    fn test_debug() {
        use std::fmt::Write;

        let store = Arc::new(TestStore::new());
        let mut tree = TreeManifest::ephemeral(store.clone());
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
        let root_1_entry = store::Entry::from_elements_hg(vec![
            store_element("foo", "11", store::Flag::Directory),
            store_element("baz", "21", store::Flag::File(FileType::Regular)),
        ]);
        store
            .insert(
                RepoPath::empty(),
                hgid("1"),
                root_1_entry.clone().to_bytes(),
            )
            .unwrap();
        let foo_11_entry = store::Entry::from_elements_hg(vec![store_element(
            "bar",
            "111",
            store::Flag::File(FileType::Regular),
        )]);
        store
            .insert(
                repo_path("foo"),
                hgid("11"),
                foo_11_entry.clone().to_bytes(),
            )
            .unwrap();

        // add ("", 2), ("foo", 12), ("baz", 21), ("foo/bar", 112)
        let root_2_entry = store::Entry::from_elements_hg(vec![
            store_element("foo", "12", store::Flag::Directory),
            store_element("baz", "21", store::Flag::File(FileType::Regular)),
        ]);
        store
            .insert(RepoPath::empty(), hgid("2"), root_2_entry.to_bytes())
            .unwrap();
        let foo_12_entry = store::Entry::from_elements_hg(vec![store_element(
            "bar",
            "112",
            store::Flag::File(FileType::Regular),
        )]);
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
                    vec![hgid("12")],
                    foo_11_entry.clone().to_bytes()
                ),
                (
                    RepoPathBuf::new(),
                    hgid("1"),
                    vec![hgid("2")],
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
                vec![hgid("2")],
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
                vec![hgid("12")],
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
        let root_1_entry = store::Entry::from_elements_hg(vec![store_element(
            "foo",
            "11",
            store::Flag::File(FileType::Regular),
        )]);
        store
            .insert(
                RepoPath::empty(),
                hgid("1"),
                root_1_entry.clone().to_bytes(),
            )
            .unwrap();

        // add ("", 2), ("foo", 12), ("foo/bar", 121)
        let root_2_entry = store::Entry::from_elements_hg(vec![store_element(
            "foo",
            "12",
            store::Flag::Directory,
        )]);
        store
            .insert(
                RepoPath::empty(),
                hgid("2"),
                root_2_entry.clone().to_bytes(),
            )
            .unwrap();
        let foo_12_entry = store::Entry::from_elements_hg(vec![store_element(
            "bar",
            "121",
            store::Flag::File(FileType::Regular),
        )]);
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
                    vec![],
                    foo_12_entry.clone().to_bytes()
                ),
                (
                    RepoPathBuf::new(),
                    hgid("2"),
                    vec![hgid("1")],
                    root_2_entry.clone().to_bytes()
                ),
            ]
        );
    }

    #[test]
    fn test_list() {
        test_list_format(TreeFormat::Git);
        test_list_format(TreeFormat::Hg);
    }

    fn test_list_format(format: TreeFormat) {
        let mut tree = TreeManifest::ephemeral(Arc::new(TestStore::new().with_format(format)));
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
            List::Directory(vec![(
                path_component_buf("c1"),
                FsNodeMetadata::File(c1_meta)
            )]),
        );
        assert_eq!(
            tree.list(repo_path("a1")).unwrap(),
            List::Directory(vec![
                (
                    path_component_buf("b1"),
                    tree.get(repo_path("a1/b1")).unwrap().unwrap()
                ),
                (path_component_buf("b2"), FsNodeMetadata::File(b2_meta)),
            ]),
        );
        assert_eq!(tree.list(repo_path("a2/b3/c2")).unwrap(), List::File);
        assert_eq!(
            tree.list(repo_path("a2/b3")).unwrap(),
            List::Directory(vec![(
                path_component_buf("c2"),
                FsNodeMetadata::File(c2_meta)
            )]),
        );
        assert_eq!(
            tree.list(repo_path("a2")).unwrap(),
            List::Directory(vec![
                (path_component_buf("b3"), FsNodeMetadata::Directory(None)),
                (path_component_buf("b4"), FsNodeMetadata::File(b4_meta)),
            ]),
        );
        assert_eq!(
            tree.list(RepoPath::empty()).unwrap(),
            List::Directory(vec![
                (
                    path_component_buf("a1"),
                    tree.get(repo_path("a1")).unwrap().unwrap()
                ),
                (path_component_buf("a2"), FsNodeMetadata::Directory(None)),
            ]),
        );
    }
}
