// Copyright Facebook, Inc. 2017
//! Directory State Tree.

use crate::filestate::{FileState, FileStateV2, StateFlags};
use crate::serialization::Serializable;
use crate::store::{BlockId, Store, StoreView};
use crate::vecmap::VecMap;
use crate::vecstack::VecStack;
use failure::Fallible;
use std::cell::Cell;
use std::collections::Bound;
use std::io::{Cursor, Read, Write};

/// A node entry is an entry in a directory, either a file or another directory.
#[derive(Debug)]
pub(crate) enum NodeEntry<T> {
    Directory(Node<T>),
    File(T),
}

/// Filenames are buffers of bytes.  They're not stored in Strings as they may not be UTF-8.
pub type Key = Box<[u8]>;
pub type KeyRef<'a> = &'a [u8];

/// Result of a "visitor" function.  Specify whether a file is changed or not. Used to mark
/// parent directory as "dirty" recursively.
#[derive(Debug, Eq, PartialEq, Clone, Copy)]
pub enum VisitorResult {
    NotChanged,
    Changed,
}

/// Store the node entries in an ordered map from name to node entry.
pub(crate) type NodeEntryMap<T> = VecMap<Key, NodeEntry<T>>;

/// The aggregated state. Useful for fast decision about whether to visit a directory recursively
/// or not.
#[derive(Debug, PartialEq, Clone, Copy)]
pub struct AggregatedState {
    pub union: StateFlags,
    pub intersection: StateFlags,
}

impl AggregatedState {
    pub fn merge(&self, rhs: AggregatedState) -> AggregatedState {
        AggregatedState {
            union: self.union | rhs.union,
            intersection: self.intersection & rhs.intersection,
        }
    }

    /// Adjust `intersection` so it does not have bits outside `union`.
    pub fn normalized(&self) -> AggregatedState {
        AggregatedState {
            union: self.union,
            intersection: self.intersection & self.union,
        }
    }
}

impl From<StateFlags> for AggregatedState {
    fn from(state: StateFlags) -> Self {
        AggregatedState {
            union: state,
            intersection: state,
        }
    }
}

impl Default for AggregatedState {
    fn default() -> Self {
        AggregatedState {
            union: StateFlags::empty(),
            intersection: StateFlags::all(),
        }
    }
}

/// The contents of a directory.
#[derive(Debug)]
pub struct Node<T> {
    /// The ID of the directory in the store.  If None, this directory has not yet been
    /// written to the back-end store in its current state.
    pub(crate) id: Option<BlockId>,

    /// The set of files and directories in this directory, indexed by their name.  If None,
    /// then the ID must not be None, and the entries are yet to be loaded from the back-end
    /// store.
    pub(crate) entries: Option<NodeEntryMap<T>>,

    /// Aggregated state flags. This is useful for quickly test whether there is a file matching
    /// given state or not in this tree (recursively). `None` means it is not calculated yet.
    aggregated_state: Cell<Option<AggregatedState>>,

    /// Optional cache about name filtering result. See `FilteredKeyCache` and `get_filtered_key`
    /// for details.
    filtered_keys: Option<FilteredKeyCache>,
}

/// A map from keys that have been filtered through a case-folding filter function to the
/// original key.  This is used for case-folded look-ups.  Filtered values are cached here.
///
/// The map is associated with an integer, the identity of the filter function used. So the
/// map could be invalidated correctly if filter function changes.
///
/// If a filtered key maps to multiple keys. All of them are stored, sorted by alphabet order.
#[derive(Debug)]
struct FilteredKeyCache {
    filter_id: u64,
    map: VecMap<Key, Vec<Key>>,
}

/// The root of the tree.  The count of files in the tree is maintained for fast size
/// determination.
pub struct Tree<T> {
    root: Node<T>,
    file_count: u32,
}

/// Utility enum for recursing through trees.
enum PathRecurse<'name, 'node, T: 'node> {
    Directory(KeyRef<'name>, KeyRef<'name>, &'node mut Node<T>),
    ExactDirectory(KeyRef<'name>, &'node mut Node<T>),
    MissingDirectory(KeyRef<'name>, KeyRef<'name>),
    File(KeyRef<'name>, &'node mut T),
    MissingFile(KeyRef<'name>),
    ConflictingFile(KeyRef<'name>, KeyRef<'name>, &'node mut T),
}

/// Splits a key into the first path element and the remaining path elements (if any).  Doesn't
/// split the key if it just contains an exact file or directory name.
fn split_key<'a>(key: KeyRef<'a>) -> (KeyRef<'a>, Option<KeyRef<'a>>) {
    if key.len() == 0 {
        return (key, None);
    }
    // Skip the last character.  Even if it's a '/' we don't want to split on it.
    for (index, value) in key.iter().take(key.len() - 1).enumerate() {
        if *value == b'/' {
            return (&key[..index + 1], Some(&key[index + 1..]));
        }
    }
    (key, None)
}

/// Splits a key into the first path element and the remaining path elements (if any).  Splits
/// the key even if it contains an exact file or directory name.
fn split_key_exact<'a>(key: KeyRef<'a>) -> (KeyRef<'a>, Option<KeyRef<'a>>) {
    for (index, value) in key.iter().enumerate() {
        if *value == b'/' {
            return (&key[..index + 1], Some(&key[index + 1..]));
        }
    }
    (key, None)
}

/// Compatiblity layer - difference between `FileState` and `FileStateV2`
pub trait CompatExt<T> {
    /// Load extra fields. Extends `load`.
    fn load_ext(&self, data: &mut dyn Read) -> Fallible<()>;

    /// Write extra fields. Extends `write_entries`.
    fn write_ext(&self, writer: &mut dyn Write) -> Fallible<()>;

    /// Calculate `aggregated_state` if it's not calculated yet.
    fn calculate_aggregated_state(&self) -> AggregatedState;

    /// Calculate `aggregated_state` if it's not calculated yet, recursively.
    fn calculate_aggregated_state_recursive(
        &mut self,
        _store: &dyn StoreView,
    ) -> Fallible<AggregatedState>;
}

impl CompatExt<FileState> for Node<FileState> {
    fn load_ext(&self, _: &mut dyn Read) -> Fallible<()> {
        Ok(())
    }

    fn write_ext(&self, _: &mut dyn Write) -> Fallible<()> {
        Ok(())
    }

    fn calculate_aggregated_state(&self) -> AggregatedState {
        AggregatedState::default().normalized()
    }

    fn calculate_aggregated_state_recursive(
        &mut self,
        _store: &dyn StoreView,
    ) -> Fallible<AggregatedState> {
        Ok(AggregatedState::default().normalized())
    }
}

impl CompatExt<FileStateV2> for Node<FileStateV2> {
    fn write_ext(&self, writer: &mut dyn Write) -> Fallible<()> {
        let state = self.calculate_aggregated_state();
        state.serialize(writer)?;
        Ok(())
    }

    fn calculate_aggregated_state(&self) -> AggregatedState {
        match self.aggregated_state.get() {
            None => {
                let state = self
                    .entries
                    .as_ref()
                    .expect("entries should exist")
                    .iter()
                    .fold(AggregatedState::default(), |acc, (_, x)| match x {
                        &NodeEntry::Directory(ref x) => {
                            acc.merge(x.aggregated_state.get().expect("should be ready now"))
                        }
                        &NodeEntry::File(ref x) => acc.merge(x.state.into()),
                    });
                self.aggregated_state.set(Some(state));
                state
            }
            Some(state) => state,
        }
    }

    fn calculate_aggregated_state_recursive(
        &mut self,
        store: &dyn StoreView,
    ) -> Fallible<AggregatedState> {
        self.load_aggregated_state(store)?;
        if self.aggregated_state.get().is_none() {
            for (_name, entry) in self.load_entries(store)?.iter_mut() {
                if let &mut NodeEntry::Directory(ref mut node) = entry {
                    node.calculate_aggregated_state_recursive(store)?;
                }
            }
        }
        Ok(self.calculate_aggregated_state())
    }

    fn load_ext(&self, data: &mut dyn Read) -> Fallible<()> {
        self.aggregated_state
            .set(Some(AggregatedState::deserialize(data)?));
        Ok(())
    }
}

impl<T: Serializable + Clone> Node<T> {
    /// Create a new empty Node.  This has no ID as it is not yet written to the store.
    fn new() -> Node<T> {
        Node {
            id: None,
            entries: Some(NodeEntryMap::new()),
            filtered_keys: None,
            aggregated_state: Cell::new(None),
        }
    }

    /// Create a new Node for an existing entry in the store.  The entries are not loaded until
    /// the load method is called.
    pub(crate) fn open(id: BlockId) -> Node<T> {
        Node {
            id: Some(id),
            entries: None,
            filtered_keys: None,
            aggregated_state: Cell::new(None),
        }
    }

    /// Return the aggregated file state that is the "bitwise-or" of all subentries,
    /// or `None` if it's not calculated.
    pub fn get_aggregated_state(&self) -> Option<AggregatedState> {
        self.aggregated_state.get()
    }

    /// Return if the node is changed in memory and is not written to disk yet.
    pub fn is_changed(&self) -> bool {
        self.id.is_none()
    }
}

impl<T: Serializable + Clone> Node<T>
where
    Self: CompatExt<T>,
{
    /// Attempt to load a node from a store.
    fn load(&mut self, store: &dyn StoreView) -> Fallible<()> {
        if self.entries.is_some() {
            // Already loaded.
            return Ok(());
        }
        let id = self.id.expect("Node must have a valid ID to be loaded");
        let data = store.read(id)?;
        let mut cur = Cursor::new(data);
        self.load_ext(&mut cur)?;
        self.entries = Some(NodeEntryMap::<T>::deserialize(&mut cur)?);
        Ok(())
    }

    /// Load only the aggregated_state without entries.
    fn load_aggregated_state(&mut self, store: &dyn StoreView) -> Fallible<()> {
        if self.entries.is_some() {
            // No need to load aggregated_state, since it was loaded before.
            return Ok(());
        } else if self.aggregated_state.get().is_some() {
            // Already loaded.
            return Ok(());
        }
        let id = self
            .id
            .expect("Node must have a valid ID to load aggregated_state");
        let data = store.read(id)?;
        let mut cur = Cursor::new(data);
        self.load_ext(&mut cur)?;
        Ok(())
    }

    /// Get access to the node entries, ensuring they are loaded first.
    #[inline]
    fn load_entries(&mut self, store: &dyn StoreView) -> Fallible<&mut NodeEntryMap<T>> {
        self.load(store)?;
        let entries = self
            .entries
            .as_mut()
            .expect("Entries should have been populated by loading");
        Ok(entries)
    }

    /// Writes all entries for this node to the store.  Any child directory entries must have
    /// had IDs assigned to them.
    fn write_entries(&mut self, store: &mut dyn Store) -> Fallible<()> {
        let mut data = Vec::new();
        self.write_ext(&mut data)?;
        {
            let entries = self
                .entries
                .as_ref()
                .expect("Node should have entries populated before writing out.");
            entries.serialize(&mut data)?;
        }
        self.id = Some(store.append(&data)?);
        Ok(())
    }

    /// Perform a full write of the node and its children to the store.  Old entries are
    /// loaded from the old_store before being written back to the new store.
    fn write_full(&mut self, store: &mut dyn Store, old_store: &dyn StoreView) -> Fallible<()> {
        // Write out all the child nodes.
        for (_name, entry) in self.load_entries(old_store)?.iter_mut() {
            if let &mut NodeEntry::Directory(ref mut node) = entry {
                node.write_full(store, old_store)?;
            }
        }
        // Write out this node.
        self.write_entries(store)
    }

    /// Perform a delta write of the node and its children to the store.  Entries that are
    /// already in the store will not be written again.
    fn write_delta<S: Store + StoreView>(&mut self, store: &mut S) -> Fallible<()> {
        if self.id.is_none() {
            // This node has been modified, write out a new copy of any children who have
            // also changed.  The entries list must already have been populated when the node
            // was modified, so no need to load it here.
            {
                let entries = self
                    .entries
                    .as_mut()
                    .expect("Node should have entries populated if it was modified.");
                for (_name, entry) in entries.iter_mut() {
                    if let &mut NodeEntry::Directory(ref mut node) = entry {
                        node.write_delta(store)?;
                    }
                }
            }

            // This is needed. Sometimes subentries have `id` set but not aggregated_state.
            // That happens with `Node::open`.
            {
                self.calculate_aggregated_state_recursive(store)?;
            }

            // Write out this node.
            self.write_entries(store)
        } else {
            // This node and its descendents have not been modified.
            Ok(())
        }
    }

    /// Visit all of the files in under this node, by calling the visitor function on each one.
    ///
    /// `visit_dir` will be called to test if a directory is worth visiting or not.
    /// `visit_file` will be called to test if a file is worth visiting or not.
    ///
    /// The visitor can change the file, in which case it must return `VisitorResult::Changed` so
    /// parent nodes can be marked as "changed" correctly.
    ///
    /// Return a `VisitorResult` indicating whether this node is changed or not so nodes can be
    /// marked "changed" recursively.
    fn visit<'a, F, VD, VF>(
        &'a mut self,
        store: &dyn StoreView,
        path: &mut VecStack<'a, [u8]>,
        visitor: &mut F,
        visit_dir: &VD,
        visit_file: &VF,
    ) -> Fallible<VisitorResult>
    where
        F: FnMut(&Vec<KeyRef>, &mut T) -> Fallible<VisitorResult>,
        VD: Fn(&Vec<KeyRef>, &Node<T>) -> bool,
        VF: Fn(&Vec<KeyRef>, &T) -> bool,
    {
        // visit_dir wants aggregated_state to be populated to do quick filtering.
        self.load_aggregated_state(store)?;
        if !visit_dir(path.as_ref(), self) {
            return Ok(VisitorResult::NotChanged);
        }

        let mut result = VisitorResult::NotChanged;

        let entries: &mut NodeEntryMap<T> = {
            self.load_entries(store)?;
            self.entries.as_mut().unwrap()
        };

        for (name, entry) in entries.iter_mut() {
            let mut path = path.push(name);
            let sub_result = match entry {
                &mut NodeEntry::Directory(ref mut node) => {
                    node.visit(store, &mut path, visitor, visit_dir, visit_file)?
                }
                &mut NodeEntry::File(ref mut file) => {
                    if visit_file(path.as_ref(), file) {
                        visitor(path.as_ref(), file)?
                    } else {
                        VisitorResult::NotChanged
                    }
                }
            };
            if sub_result == VisitorResult::Changed {
                result = VisitorResult::Changed;
            }
        }

        if result == VisitorResult::Changed {
            self.id = None;
            self.aggregated_state.set(None);
        }
        Ok(result)
    }

    /// Get the first file in the subtree under this node.  If the subtree is not empty, returns a
    /// pair containing the path to the file as a reversed vector of key references for each path
    /// element, and a reference to the file.
    fn get_first<'node>(
        &'node mut self,
        store: &dyn StoreView,
    ) -> Fallible<Option<(Vec<KeyRef<'node>>, &'node T)>> {
        for (name, entry) in self.load_entries(store)?.iter_mut() {
            match entry {
                &mut NodeEntry::Directory(ref mut node) => {
                    if let Some((mut next_name, next_file)) = node.get_first(store)? {
                        next_name.push(name);
                        return Ok(Some((next_name, next_file)));
                    }
                }
                &mut NodeEntry::File(ref file) => {
                    return Ok(Some((vec![name], file)));
                }
            }
        }
        Ok(None)
    }

    /// Get the next file after a particular file in the tree.  Returns a pair containing the path
    /// to the file as a reversed vector of key references for each path element, and a reference
    /// to the file, or None if there are no more files.
    fn get_next<'node>(
        &'node mut self,
        store: &dyn StoreView,
        name: KeyRef,
    ) -> Fallible<Option<(Vec<KeyRef<'node>>, &'node T)>> {
        // Find the entry within this list, and what the remainder of the path is.
        let (elem, mut path) = split_key(name);

        // Get the next entry after the current one.  We need to look inside directories as we go.
        // The subpath we obtained from split_key is only relevant if we are looking inside the
        // directory the path refers to.
        for (entry_name, entry) in self
            .load_entries(store)?
            .range_mut((Bound::Included(elem), Bound::Unbounded))
        {
            match entry {
                &mut NodeEntry::Directory(ref mut node) => {
                    // The entry is a directory, check inside it.
                    if elem != &entry_name[..] {
                        // This directory is not the one we were initially looking inside.  We
                        // have moved on past that directory, so the rest of the path is no
                        // longer relevant.
                        path = None
                    }
                    let next = if let Some(path) = path {
                        // Find the next file after the given subpath.
                        node.get_next(store, path)?
                    } else {
                        // Find the first file in this subtree.
                        node.get_first(store)?
                    };
                    if let Some((mut next_name, next_file)) = next {
                        next_name.push(entry_name);
                        return Ok(Some((next_name, next_file)));
                    }
                }
                &mut NodeEntry::File(ref file) => {
                    // This entry is a file.  Skip over it if it is the original file.
                    if elem != &entry_name[..] {
                        return Ok(Some((vec![entry_name], file)));
                    }
                }
            }
        }
        Ok(None)
    }

    /// Utility function for recursing through subdirectories.  Returns the appropriate
    /// PathRecurse variant for the current position in the file tree given by name.
    fn path_recurse<'name, 'node>(
        &'node mut self,
        store: &dyn StoreView,
        name: KeyRef<'name>,
    ) -> Fallible<PathRecurse<'name, 'node, T>> {
        let (elem, path) = split_key(name);
        let res = if let Some(path) = path {
            // The name is for a subdirectory.
            match self.load_entries(store)?.get_mut(elem) {
                Some(&mut NodeEntry::Directory(ref mut node)) => {
                    PathRecurse::Directory(elem, path, node)
                }
                Some(&mut NodeEntry::File(ref mut file)) => {
                    PathRecurse::ConflictingFile(elem, path, file)
                }
                None => PathRecurse::MissingDirectory(elem, path),
            }
        } else {
            // The name is for a file or directory in this directory.
            match self.load_entries(store)?.get_mut(elem) {
                Some(&mut NodeEntry::Directory(ref mut node)) => {
                    PathRecurse::ExactDirectory(elem, node)
                }
                Some(&mut NodeEntry::File(ref mut file)) => PathRecurse::File(elem, file),
                None => PathRecurse::MissingFile(elem),
            }
        };
        Ok(res)
    }

    /// Get a file's state.
    fn get<'node>(
        &'node mut self,
        store: &dyn StoreView,
        name: KeyRef,
    ) -> Fallible<Option<&'node T>> {
        match self.path_recurse(store, name)? {
            PathRecurse::Directory(_dir, path, node) => node.get(store, path),
            PathRecurse::ExactDirectory(_dir, _node) => Ok(None),
            PathRecurse::MissingDirectory(_dir, _path) => Ok(None),
            PathRecurse::File(_name, file) => Ok(Some(file)),
            PathRecurse::MissingFile(_name) => Ok(None),
            PathRecurse::ConflictingFile(_name, _path, _file) => Ok(None),
        }
    }

    /// Returns true if the given path is a directory.
    fn has_dir(&mut self, store: &dyn StoreView, name: KeyRef) -> Fallible<bool> {
        // This directory exists, without checking entries.
        if name == b"/" {
            return Ok(true);
        }
        match self.path_recurse(store, name)? {
            PathRecurse::Directory(_dir, path, node) => node.has_dir(store, path),
            PathRecurse::ExactDirectory(_dir, _node) => Ok(true),
            PathRecurse::MissingDirectory(_dir, _path) => Ok(false),
            PathRecurse::File(_name, _file) => Ok(false),
            PathRecurse::MissingFile(_name) => Ok(false),
            PathRecurse::ConflictingFile(_name, _path, _file) => Ok(false),
        }
    }

    /// Returns `Some(AggregatedState)` if the given path is a directory, or `None`.
    fn get_dir(
        &mut self,
        store: &dyn StoreView,
        name: KeyRef,
    ) -> Fallible<Option<AggregatedState>> {
        if name == b"/" {
            return Ok(Some(self.calculate_aggregated_state_recursive(store)?));
        }

        match self.path_recurse(store, name)? {
            PathRecurse::Directory(_dir, path, node) => node.get_dir(store, path),
            PathRecurse::ExactDirectory(_dir, node) => node.get_dir(store, b"/"),
            PathRecurse::MissingDirectory(_dir, _path) => Ok(None),
            PathRecurse::File(_name, _file) => Ok(None),
            PathRecurse::MissingFile(_name) => Ok(None),
            PathRecurse::ConflictingFile(_name, _path, _file) => Ok(None),
        }
    }

    /// Add a file to the node.  The name may contain a path, in which case sufficient
    /// subdirectories are updated to add or update the file.
    fn add(&mut self, store: &dyn StoreView, name: KeyRef, info: &T) -> Fallible<bool> {
        let (new_entry, file_added) = match self.path_recurse(store, name)? {
            PathRecurse::Directory(_dir, path, node) => {
                // The file is in a subdirectory.  Add it to the subdirectory.
                let file_added = node.add(store, path, info)?;
                (None, file_added)
            }
            PathRecurse::ExactDirectory(_dir, _node) => {
                panic!("Adding file which matches the name of a directory.");
            }
            PathRecurse::MissingDirectory(dir, path) => {
                // The file is in a new subdirectory.  Create the directory and add the file.
                let mut node = Node::new();
                let file_added = node.add(store, path, info)?;
                (
                    Some((dir.to_vec().into_boxed_slice(), NodeEntry::Directory(node))),
                    file_added,
                )
            }
            PathRecurse::File(_name, file) => {
                // The file is in this directory.  Update it.
                file.clone_from(info);
                (None, false)
            }
            PathRecurse::MissingFile(ref name) => {
                // The file should be in this directory.  Add it.
                if name.is_empty() || name[name.len() - 1] == b'/' {
                    panic!("Adding file with tailing slash");
                }
                (
                    Some((
                        name.to_vec().into_boxed_slice(),
                        NodeEntry::File(info.clone()),
                    )),
                    true,
                )
            }
            PathRecurse::ConflictingFile(_name, _path, _file) => {
                panic!("Adding file with path prefix that matches the name of a file.")
            }
        };
        if let Some((new_key, new_entry)) = new_entry {
            self.load_entries(store)?.insert(new_key, new_entry);
            self.filtered_keys = None;
        }
        // Reset aggregated_state so it needs recalculation.
        self.aggregated_state.set(None);
        self.id = None;
        Ok(file_added)
    }

    /// Remove a file from the node.  The name may contain a path, in which case sufficient
    /// subdirectories are updated to remove the file.
    ///
    /// Returns a pair of booleans (file_removed, now_empty) indicating whether the file
    /// was removed, and whether the diectory is now empty.
    fn remove(&mut self, store: &dyn StoreView, name: KeyRef) -> Fallible<(bool, bool)> {
        let (file_removed, remove_entry) = match self.path_recurse(store, name)? {
            PathRecurse::Directory(dir, path, node) => {
                let (file_removed, now_empty) = node.remove(store, path)?;
                (file_removed, if now_empty { Some(dir) } else { None })
            }
            PathRecurse::ExactDirectory(_dir, _node) => (false, None),
            PathRecurse::MissingDirectory(_dir, _path) => (false, None),
            PathRecurse::File(name, _file) => (true, Some(name)),
            PathRecurse::MissingFile(_name) => (false, None),
            PathRecurse::ConflictingFile(_name, _path, _file) => (false, None),
        };
        if let Some(entry) = remove_entry {
            self.load_entries(store)?.remove(entry);
            self.filtered_keys = None;
            self.id = None;
        }
        if file_removed {
            self.aggregated_state.set(None);
            self.id = None;
        }
        Ok((file_removed, self.load_entries(store)?.is_empty()))
    }

    /// Performs a key lookup using filtered keys.
    ///
    /// Applies the filter function to each key in the node, then returns the real key that
    /// matches the name provided.  The name may contain a path, in which case the subdirectories
    /// of this node are also queried.
    ///
    /// Returns a list of reversed vector of key references for each path element.
    ///
    /// `filter_id` should be different for logically different `filter` functions. It is used for
    /// cache invalidation.
    fn get_filtered_key<'a, F>(
        &'a mut self,
        store: &dyn StoreView,
        name: KeyRef,
        filter: &mut F,
        filter_id: u64,
    ) -> Fallible<Vec<Vec<Key>>>
    where
        F: FnMut(KeyRef) -> Fallible<Key>,
    {
        let (elem, path) = split_key(name);
        if self.filtered_keys.is_none()
            || self.filtered_keys.as_ref().unwrap().filter_id != filter_id
        {
            let new_map = {
                let entries = self.load_entries(store)?;
                let mut new_map: VecMap<Key, Vec<Key>> = VecMap::with_capacity(entries.len());
                for (k, _v) in entries.iter() {
                    let filtered = filter(k)?;
                    let inserted = match new_map.get_mut(&filtered) {
                        Some(keys) => {
                            keys.push(k.to_vec().into_boxed_slice());
                            true
                        }
                        None => false,
                    };
                    if !inserted {
                        new_map.insert(filtered, vec![k.to_vec().into_boxed_slice()]);
                    }
                }
                new_map
            };
            self.filtered_keys = Some(FilteredKeyCache {
                filter_id,
                map: new_map,
            });
        }
        if let Some(path) = path {
            let mut result = Vec::new();
            if let Some(mapped_elems) = self.filtered_keys.as_ref().unwrap().map.get(elem) {
                let mut entries = self.entries.as_mut().unwrap();
                let entries = &mut entries;
                for mapped_elem in mapped_elems {
                    if let Some(&mut NodeEntry::Directory(ref mut node)) =
                        entries.get_mut(mapped_elem)
                    {
                        for mut mapped_path in
                            node.get_filtered_key(store, path, filter, filter_id)?
                        {
                            mapped_path.push(mapped_elem.clone());
                            result.push(mapped_path);
                        }
                    }
                }
            }
            Ok(result)
        } else {
            Ok(self
                .filtered_keys
                .as_ref()
                .unwrap()
                .map
                .get(elem)
                .cloned()
                .unwrap_or_else(|| Vec::new())
                .iter()
                .map(|e| vec![e.to_vec().into_boxed_slice()])
                .collect())
        }
    }

    /// Checks if a path is suitable for completion, in that it contains a file that matches
    /// the acceptable conditions.
    fn path_complete_check<FA>(&mut self, store: &dyn StoreView, acceptable: &FA) -> Fallible<bool>
    where
        FA: Fn(&T) -> bool,
    {
        for (_name, entry) in self.load_entries(store)?.iter_mut() {
            match entry {
                &mut NodeEntry::Directory(ref mut node) => {
                    if node.path_complete_check(store, acceptable)? {
                        return Ok(true);
                    }
                }
                &mut NodeEntry::File(ref mut file) => {
                    if acceptable(file) {
                        return Ok(true);
                    }
                }
            }
        }
        Ok(false)
    }

    /// Attempt to complete a path prefix.
    ///
    /// If full_paths is true, calls the visitor for every file that matches the prefix where
    /// the file's state returns true when passed to acceptable.
    ///
    /// If full_paths is false, the first matching directory that matches the prefix is used as
    /// long as there is at least one file under the directory that is acceptable.
    fn path_complete<'a, FA, FV>(
        &'a mut self,
        store: &dyn StoreView,
        path: &mut VecStack<'a, [u8]>,
        prefix: KeyRef<'a>,
        full_paths: bool,
        acceptable: &FA,
        visitor: &mut FV,
    ) -> Fallible<()>
    where
        FA: Fn(&T) -> bool,
        FV: FnMut(&Vec<KeyRef>) -> Fallible<()>,
    {
        let (elem, subpath) = split_key_exact(prefix);
        if let Some(subpath) = subpath {
            // Prefix part is for a directory, so look for that directory.
            let entry = self.load_entries(store)?.get_mut(elem);
            if let Some(&mut NodeEntry::Directory(ref mut node)) = entry {
                let mut path = path.push(elem);
                node.path_complete(store, &mut path, subpath, full_paths, acceptable, visitor)?;
            }
        } else {
            // Prefix part is for a entry in this directory.  Iterate across all matching entries.
            for (entry_name, entry) in self
                .load_entries(store)?
                .range_mut((Bound::Included(elem), Bound::Unbounded))
            {
                if entry_name.len() < elem.len() || &entry_name[..elem.len()] != elem {
                    // This entry is no longer a prefix.
                    break;
                }
                match entry {
                    &mut NodeEntry::Directory(ref mut node) => {
                        if full_paths {
                            let mut path = path.push(entry_name);
                            // The entry is a directory, and the caller has asked for full paths.
                            // Visit every entry inside the directory.
                            let mut visit_adapter = |filepath: &Vec<KeyRef>, state: &mut T| {
                                if acceptable(state) {
                                    visitor(filepath)?;
                                }
                                Ok(VisitorResult::NotChanged)
                            };
                            node.visit(
                                store,
                                &mut path,
                                &mut visit_adapter,
                                &|_, _| true,
                                &|_, _| true,
                            )?;
                        } else {
                            // The entry is a directory, and the caller has asked for matching
                            // directories.  Check there is an acceptable file under the
                            // directory.
                            if node.path_complete_check(store, acceptable)? {
                                let path = path.push(entry_name);
                                visitor(path.as_ref())?;
                            }
                        }
                    }
                    &mut NodeEntry::File(ref mut file) => {
                        // This entry is a file.
                        if acceptable(file) {
                            let path = path.push(entry_name);
                            visitor(path.as_ref())?;
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

impl<T: Serializable + Clone> Tree<T>
where
    Node<T>: CompatExt<T>,
{
    /// Create a new empty tree.
    pub fn new() -> Tree<T> {
        Tree {
            root: Node::new(),
            file_count: 0,
        }
    }

    /// Create a tree that references an existing root node.
    pub fn open(root_id: BlockId, file_count: u32) -> Tree<T> {
        Tree {
            root: Node::open(root_id),
            file_count,
        }
    }

    /// Clear all entries in the tree.
    pub fn clear(&mut self) {
        self.root = Node::new();
        self.file_count = 0;
    }

    pub fn root_id(&self) -> Option<BlockId> {
        self.root.id
    }

    pub fn file_count(&self) -> u32 {
        self.file_count
    }

    pub fn write_full(
        &mut self,
        store: &mut dyn Store,
        old_store: &dyn StoreView,
    ) -> Fallible<BlockId> {
        self.root.write_full(store, old_store)?;
        Ok(self.root.id.unwrap())
    }

    pub fn write_delta<S: Store + StoreView>(&mut self, store: &mut S) -> Fallible<BlockId> {
        self.root.write_delta(store)?;
        Ok(self.root.id.unwrap())
    }

    pub fn get<'a>(&'a mut self, store: &dyn StoreView, name: KeyRef) -> Fallible<Option<&'a T>> {
        Ok(self.root.get(store, name)?)
    }

    pub fn visit_advanced<F, VD, VF>(
        &mut self,
        store: &dyn StoreView,
        visitor: &mut F,
        visit_dir: &VD,
        visit_file: &VF,
    ) -> Fallible<()>
    where
        F: FnMut(&Vec<KeyRef>, &mut T) -> Fallible<VisitorResult>,
        VD: Fn(&Vec<KeyRef>, &Node<T>) -> bool,
        VF: Fn(&Vec<KeyRef>, &T) -> bool,
    {
        let mut path = Vec::new();
        let mut path = VecStack::new(&mut path);
        self.root
            .visit(store, &mut path, visitor, visit_dir, visit_file)?;
        Ok(())
    }

    pub fn visit<F>(&mut self, store: &dyn StoreView, visitor: &mut F) -> Fallible<()>
    where
        F: FnMut(&Vec<KeyRef>, &mut T) -> Fallible<VisitorResult>,
    {
        self.visit_advanced(store, visitor, &|_, _| true, &|_, _| true)
    }

    pub fn visit_changed<F>(&mut self, store: &dyn StoreView, visitor: &mut F) -> Fallible<()>
    where
        F: FnMut(&Vec<KeyRef>, &mut T) -> Fallible<VisitorResult>,
    {
        self.visit_advanced(
            store,
            visitor,
            &|_, dir: &Node<T>| dir.is_changed(),
            &|_, _| true,
        )
    }

    pub fn get_first<'a>(&'a mut self, store: &dyn StoreView) -> Fallible<Option<(Key, &'a T)>> {
        Ok(self.root.get_first(store)?.map(|(mut path, file)| {
            path.reverse();
            (path.concat().into_boxed_slice(), file)
        }))
    }

    pub fn get_next<'a>(
        &'a mut self,
        store: &dyn StoreView,
        name: KeyRef,
    ) -> Fallible<Option<(Key, &'a T)>> {
        Ok(self.root.get_next(store, name)?.map(|(mut path, file)| {
            path.reverse();
            (path.concat().into_boxed_slice(), file)
        }))
    }

    pub fn has_dir(&mut self, store: &dyn StoreView, name: KeyRef) -> Fallible<bool> {
        Ok(self.root.has_dir(store, name)?)
    }

    pub fn get_dir(
        &mut self,
        store: &dyn StoreView,
        name: KeyRef,
    ) -> Fallible<Option<AggregatedState>> {
        Ok(self.root.get_dir(store, name)?)
    }

    pub fn add(&mut self, store: &dyn StoreView, name: KeyRef, file: &T) -> Fallible<()> {
        if self.root.add(store, name, file)? {
            self.file_count += 1;
        }
        Ok(())
    }

    pub fn remove(&mut self, store: &dyn StoreView, name: KeyRef) -> Fallible<bool> {
        let removed = self.root.remove(store, name)?.0;
        if removed {
            assert!(self.file_count > 0);
            self.file_count -= 1;
        }
        Ok(removed)
    }

    pub fn get_filtered_key<F>(
        &mut self,
        store: &dyn StoreView,
        name: KeyRef,
        filter: &mut F,
        filter_id: u64,
    ) -> Fallible<Vec<Key>>
    where
        F: FnMut(KeyRef) -> Fallible<Key>,
    {
        Ok(self
            .root
            .get_filtered_key(store, name, filter, filter_id)?
            .iter_mut()
            .map(|path| {
                path.reverse();
                path.concat().into_boxed_slice()
            })
            .collect())
    }

    pub fn path_complete<FA, FV>(
        &mut self,
        store: &dyn StoreView,
        prefix: KeyRef,
        full_paths: bool,
        acceptable: &FA,
        visitor: &mut FV,
    ) -> Fallible<()>
    where
        FA: Fn(&T) -> bool,
        FV: FnMut(&Vec<KeyRef>) -> Fallible<()>,
    {
        let mut path = Vec::new();
        let mut path = VecStack::new(&mut path);
        self.root
            .path_complete(store, &mut path, prefix, full_paths, acceptable, visitor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::tests::MapStore;
    use crate::store::NullStore;

    // Test files in order.  Note lexicographic ordering of file9 and file10.
    static TEST_FILES: [(&[u8], u32, i32, i32); 16] = [
        (b"dirA/subdira/file1", 0o644, 1, 10001),
        (b"dirA/subdira/file2", 0o644, 2, 10002),
        (b"dirA/subdirb/file3", 0o644, 3, 10003),
        (b"dirB/subdira/file4", 0o644, 4, 10004),
        (b"dirB/subdira/subsubdirx/file5", 0o644, 5, 10005),
        (b"dirB/subdira/subsubdiry/file6", 0o644, 6, 10006),
        (b"dirB/subdira/subsubdirz/file7", 0o755, 7, 10007),
        (b"dirB/subdira/subsubdirz/file8", 0o755, 8, 10008),
        (b"dirB/subdirb/file10", 0o644, 10, 10010),
        (b"dirB/subdirb/file9", 0o644, 9, 10009),
        (b"dirC/file11", 0o644, 11, 10011),
        (b"dirC/file12", 0o644, 12, 10012),
        (b"dirC/file13", 0o644, 13, 10013),
        (b"dirC/file14", 0o644, 14, 10014),
        (b"dirC/file15", 0o644, 15, 10015),
        (b"file16", 0o644, 16, 10016),
    ];

    fn populate(t: &mut Tree<FileState>, s: &MapStore) {
        for &(name, mode, size, mtime) in TEST_FILES.iter() {
            t.add(s, name, &FileState::new(b'n', mode, size, mtime))
                .expect("can add file");
        }
    }

    #[test]
    fn count_get_and_remove() {
        let ms = MapStore::new();
        let mut t = Tree::new();
        assert_eq!(t.file_count(), 0);
        assert_eq!(
            t.get(&ms, b"dirB/subdira/subsubdirz/file7")
                .expect("can get"),
            None
        );
        populate(&mut t, &ms);
        assert_eq!(t.file_count(), 16);
        assert_eq!(
            t.get(&ms, b"dirB/subdira/subsubdirz/file7")
                .expect("can get"),
            Some(&FileState::new(b'n', 0o755, 7, 10007))
        );
        t.remove(&ms, b"dirB/subdirb/file9").expect("can remove");
        assert_eq!(t.file_count(), 15);
        t.remove(&ms, b"dirB/subdirb/file10").expect("can remove");
        assert_eq!(t.file_count(), 14);
        assert_eq!(
            t.get(&ms, b"dirB/subdira/subsubdirz/file7")
                .expect("can get"),
            Some(&FileState::new(b'n', 0o755, 7, 10007))
        );
        assert_eq!(t.get(&ms, b"dirB/subdirb/file9").expect("can get"), None);
    }

    #[test]
    fn iterate() {
        let ms = MapStore::new();
        let mut t = Tree::new();
        assert_eq!(t.get_first(&ms).expect("can get first"), None);
        populate(&mut t, &ms);
        let mut expect_iter = TEST_FILES.iter();
        let expected = expect_iter.next().unwrap();
        let mut filename = expected.0.to_vec();
        assert_eq!(
            t.get_first(&ms).expect("can get first"),
            Some((
                filename.clone().into_boxed_slice(),
                &FileState::new(b'n', expected.1, expected.2, expected.3)
            ))
        );
        while let Some(expected) = expect_iter.next() {
            let actual = t.get_next(&ms, &filename).expect("can get next");
            filename = expected.0.to_vec();
            assert_eq!(
                actual,
                Some((
                    filename.clone().into_boxed_slice(),
                    &FileState::new(b'n', expected.1, expected.2, expected.3)
                ))
            );
        }
        assert_eq!(t.get_next(&ms, &filename).expect("can get next"), None);
    }

    #[test]
    fn has_dir() {
        let ms = MapStore::new();
        let mut t = Tree::new();
        assert_eq!(
            t.has_dir(&ms, b"anything/").expect("can check has_dir"),
            false
        );
        populate(&mut t, &ms);
        assert_eq!(
            t.has_dir(&ms, b"something else/")
                .expect("can check has_dir"),
            false
        );
        assert_eq!(t.has_dir(&ms, b"dirB/").expect("can check has_dir"), true);
        assert_eq!(
            t.has_dir(&ms, b"dirB/subdira/").expect("can check has_dir"),
            true
        );
        assert_eq!(
            t.has_dir(&ms, b"dirB/subdira/subsubdirz/")
                .expect("can check has_dir"),
            true
        );
        assert_eq!(
            t.has_dir(&ms, b"dirB/subdira/subsubdirz/file7")
                .expect("can check has_dir"),
            false
        );
        assert_eq!(
            t.has_dir(&ms, b"dirB/subdira/subsubdirz/file7/")
                .expect("can check has_dir"),
            false
        );
    }

    #[test]
    fn write_empty() {
        let ns = NullStore::new();
        let mut ms = MapStore::new();
        let mut t = Tree::<FileState>::new();
        t.write_full(&mut ms, &ns).expect("can write full");
        t.write_delta(&mut ms).expect("can write delta");
        let mut ms2 = MapStore::new();
        t.write_full(&mut ms2, &ms).expect("can write full");
        let t_root = t.root_id().unwrap();
        let t_count = t.file_count();
        let mut t2 = Tree::<FileState>::open(t_root, t_count);
        assert_eq!(t2.get_first(&ms2).expect("can get first"), None);
    }

    #[test]
    fn write() {
        let ns = NullStore::new();
        let mut ms = MapStore::new();
        let mut t = Tree::new();
        populate(&mut t, &ms);
        t.write_full(&mut ms, &ns).expect("can write full");
        t.write_delta(&mut ms).expect("can write delta");
        let mut ms2 = MapStore::new();
        t.write_full(&mut ms2, &ms).expect("can write full");
        let t_root = t.root_id().unwrap();
        let t_count = t.file_count();
        let mut t2 = Tree::open(t_root, t_count);
        assert_eq!(
            t2.get(&ms2, b"dirB/subdira/subsubdirz/file7")
                .expect("can get"),
            Some(&FileState::new(b'n', 0o755, 7, 10007))
        );
    }

    #[test]
    fn visit() {
        let mut ms = MapStore::new();
        let mut t = Tree::new();
        populate(&mut t, &ms);
        let mut files = Vec::new();
        {
            let mut v = |path: &Vec<KeyRef>, _fs: &mut FileState| {
                files.push(path.concat());
                Ok(VisitorResult::NotChanged)
            };
            t.visit(&mut ms, &mut v).expect("can visit");
        }
        assert_eq!(
            files,
            TEST_FILES
                .iter()
                .map(|t| t.0.to_vec())
                .collect::<Vec<Vec<u8>>>()
        );
    }

    #[test]
    fn visit_changed() {
        let ns = NullStore::new();
        let mut ms = MapStore::new();
        let mut t = Tree::new();
        populate(&mut t, &ms);
        t.write_full(&mut ms, &ns).expect("can write full");

        // Touch file5.  This file, and any file in an ancestor directory (file4, file5 and file16)
        // will be in directories marked as changed.
        t.add(
            &ms,
            b"dirB/subdira/subsubdirx/file5",
            &FileState::new(b'm', 0o644, 200, 2000),
        )
        .expect("can add");

        let mut files = Vec::new();
        {
            let mut v = |path: &Vec<KeyRef>, _fs: &mut FileState| {
                files.push(path.concat());
                Ok(VisitorResult::NotChanged)
            };
            t.visit_changed(&mut ms, &mut v).expect("can visit_changed");
        }
        assert_eq!(
            files,
            vec![
                b"dirB/subdira/file4".to_vec(),
                b"dirB/subdira/subsubdirx/file5".to_vec(),
                b"file16".to_vec(),
            ]
        );
    }

    #[test]
    fn filtered_keys() {
        let ms = MapStore::new();
        let mut t = Tree::new();
        populate(&mut t, &ms);

        // Define a mapping function that upper-cases 'A' characters:
        fn map_upper_a(k: KeyRef) -> Fallible<Key> {
            Ok(k.iter()
                .map(|c| if *c == b'a' { b'A' } else { *c })
                .collect::<Vec<u8>>()
                .into_boxed_slice())
        }

        // Another map function that does nothing.
        fn map_noop(k: KeyRef) -> Fallible<Key> {
            Ok(Vec::from(k).into_boxed_slice())
        }

        // Look-up with normalized name should give non-normalized version.
        assert_eq!(
            t.get_filtered_key(&ms, b"dirA/subdirA/file1", &mut map_upper_a, 0)
                .expect("should succeed"),
            vec![b"dirA/subdira/file1".to_vec().into_boxed_slice()]
        );

        // Look-up with non-normalized name should match nothing.
        assert_eq!(
            t.get_filtered_key(&ms, b"dirA/subdira/file1", &mut map_upper_a, 0)
                .expect("should succeed"),
            vec![]
        );

        // Change filter function should invalid existing cache.
        assert_eq!(
            t.get_filtered_key(&ms, b"dirA/subdirA/file1", &mut map_noop, 1)
                .unwrap(),
            vec![]
        );
    }

    #[test]
    fn filtered_keys_surjective() {
        let mut t = Tree::new();
        let ms = MapStore::new();
        t.add(&ms, b"a/a/A", &FileState::new(b'a', 0, 0, 0))
            .unwrap();
        t.add(&ms, b"A/a/A", &FileState::new(b'a', 0, 0, 0))
            .unwrap();
        t.add(&ms, b"A/A/a", &FileState::new(b'a', 0, 0, 0))
            .unwrap();

        fn map_upper_a(key: KeyRef) -> Fallible<Key> {
            Ok(key
                .iter()
                .map(|c| if *c == b'a' { b'A' } else { *c })
                .collect::<Vec<u8>>()
                .into_boxed_slice())
        }

        assert_eq!(
            t.get_filtered_key(&ms, b"A/A/", &mut map_upper_a, 0)
                .unwrap(),
            vec![
                b"A/A/".to_vec().into_boxed_slice(),
                b"A/a/".to_vec().into_boxed_slice(),
                b"a/a/".to_vec().into_boxed_slice(),
            ]
        );

        assert_eq!(
            t.get_filtered_key(&ms, b"A/A/A", &mut map_upper_a, 0)
                .unwrap(),
            vec![
                b"A/A/a".to_vec().into_boxed_slice(),
                b"A/a/A".to_vec().into_boxed_slice(),
                b"a/a/A".to_vec().into_boxed_slice(),
            ]
        );
    }
}
