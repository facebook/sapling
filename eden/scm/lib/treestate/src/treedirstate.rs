/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Directory State.

use crate::filestate::FileState;
use crate::filestore::FileStore;
use crate::serialization::Serializable;
use crate::store::{BlockId, NullStore, Store, StoreView};
use crate::tree::{Key, KeyRef, Tree, VisitorResult};
use failure::Fallible as Result;
use std::io::Cursor;
use std::path::Path;

/// Selected backend implementation for the treedirstate.
enum Backend {
    /// The treedirstate is not currently backed by a file.
    Empty(NullStore),

    /// The treedirstate is backed by a file on disk.
    File(FileStore),
}

impl Backend {
    pub fn store<'a>(&'a mut self) -> &'a mut dyn Store {
        match *self {
            Backend::Empty(ref mut _null) => {
                panic!("attempt to write to uninitialized treedirstate")
            }
            Backend::File(ref mut file) => file,
        }
    }

    pub fn store_view<'a>(&'a self) -> &'a dyn StoreView {
        match *self {
            Backend::Empty(ref null) => null,
            Backend::File(ref file) => file,
        }
    }

    pub fn cache(&mut self) -> Result<()> {
        match *self {
            Backend::Empty(ref _null) => Ok(()),
            Backend::File(ref mut file) => file.cache(),
        }
    }

    pub fn offset(&self) -> Option<u64> {
        match *self {
            Backend::Empty(ref _null) => None,
            Backend::File(ref file) => Some(file.position()),
        }
    }
}

/// A treedirstate object.  This contains the state of all files in the dirstate, stored in tree
/// structures, and backed by an append-only store on disk.
pub struct TreeDirstate {
    /// The store currently in use by the Dirstate.
    store: Backend,

    /// The tree of tracked files.
    tracked: Tree<FileState>,

    /// The tree of removed files.
    removed: Tree<FileState>,

    /// The ID of the root block.
    root_id: Option<BlockId>,
}

/// Representation of the root of a dirstate tree that can be serialized to disk.
pub(crate) struct TreeDirstateRoot {
    pub(crate) tracked_root_id: BlockId,
    pub(crate) tracked_file_count: u32,
    pub(crate) removed_root_id: BlockId,
    pub(crate) removed_file_count: u32,
}

impl TreeDirstate {
    /// Create a new, empty treedirstate, with no backend store.
    pub fn new() -> TreeDirstate {
        TreeDirstate {
            store: Backend::Empty(NullStore::new()),
            tracked: Tree::new(),
            removed: Tree::new(),
            root_id: None,
        }
    }

    /// Open an existing treedirstate file.  The entries in the file will not be materialized from
    /// the disk until they are accessed.
    pub fn open<P: AsRef<Path>>(&mut self, filename: P, root_id: BlockId) -> Result<()> {
        let store = FileStore::open(filename)?;
        let root = TreeDirstateRoot::deserialize(&mut Cursor::new(store.read(root_id)?))?;
        self.tracked = Tree::open(root.tracked_root_id, root.tracked_file_count);
        self.removed = Tree::open(root.removed_root_id, root.removed_file_count);
        self.store = Backend::File(store);
        self.root_id = Some(root_id);
        Ok(())
    }

    /// Write a new root block to the store.  This contains the identities of the tree roots
    /// and the tree sizes.
    fn write_root(&mut self) -> Result<()> {
        let root = TreeDirstateRoot {
            tracked_root_id: self.tracked.root_id().unwrap(),
            tracked_file_count: self.tracked.file_count(),
            removed_root_id: self.removed.root_id().unwrap(),
            removed_file_count: self.removed.file_count(),
        };
        let store = self.store.store();
        let mut data = Vec::new();
        root.serialize(&mut data)?;
        self.root_id = Some(store.append(&data)?);
        store.flush()?;
        Ok(())
    }

    /// Write a full copy of the treedirstate out to a new file.
    pub fn write_full<P: AsRef<Path>>(&mut self, filename: P) -> Result<()> {
        {
            let mut store = FileStore::create(filename)?;
            {
                let old_store = self.store.store_view();
                self.tracked.write_full(&mut store, old_store)?;
                self.removed.write_full(&mut store, old_store)?;
            }
            self.store = Backend::File(store);
        }
        self.write_root()
    }

    /// Write updated entries in the treedirstate to the store.
    pub fn write_delta(&mut self) -> Result<()> {
        {
            match self.store {
                Backend::Empty(ref mut store) => {
                    self.tracked.write_delta(store)?;
                    self.removed.write_delta(store)?;
                }
                Backend::File(ref mut store) => {
                    self.tracked.write_delta(store)?;
                    self.removed.write_delta(store)?;
                }
            }
        }
        self.write_root()
    }

    /// Clears all entries from the treedirstate.
    pub fn clear(&mut self) {
        self.tracked.clear();
        self.removed.clear();
    }

    /// Returns the ID of the root block.
    pub fn root_id(&self) -> Option<BlockId> {
        self.root_id
    }

    /// Returns the current append offset for the file store.
    pub fn store_offset(&self) -> Option<u64> {
        self.store.offset()
    }

    /// Add or update a file entry in the treedirstate.
    pub fn add_file(&mut self, name: KeyRef, state: &FileState) -> Result<()> {
        let store = self.store.store_view();
        self.removed.remove(store, name)?;
        self.tracked.add(store, name, state)?;
        Ok(())
    }

    /// Mark a file as removed in the treedirstate.
    pub fn remove_file(&mut self, name: KeyRef, state: &FileState) -> Result<()> {
        let store = self.store.store_view();
        self.tracked.remove(store, name)?;
        self.removed.add(store, name, state)?;
        Ok(())
    }

    /// Drop a file from the treedirstate.
    pub fn drop_file(&mut self, name: KeyRef) -> Result<bool> {
        let store = self.store.store_view();
        let tracked = self.tracked.remove(store, name)?;
        let removed = self.removed.remove(store, name)?;
        Ok(tracked || removed)
    }

    pub fn tracked_count(&self) -> u32 {
        self.tracked.file_count()
    }

    pub fn removed_count(&self) -> u32 {
        self.removed.file_count()
    }

    /// Get an entry from the tracked tree.
    pub fn get_tracked<'a>(&'a mut self, name: KeyRef) -> Result<Option<&'a FileState>> {
        self.tracked.get(self.store.store_view(), name)
    }

    pub fn get_tracked_filtered_key<F>(
        &mut self,
        name: KeyRef,
        filter: &mut F,
        filter_id: u64,
    ) -> Result<Option<Key>>
    where
        F: FnMut(KeyRef) -> Result<Key>,
    {
        self.tracked
            .get_filtered_key(self.store.store_view(), name, filter, filter_id)
            .map(|keys| keys.first().cloned())
    }

    /// Visit all tracked files with a visitor.
    pub fn visit_tracked<F>(&mut self, visitor: &mut F) -> Result<()>
    where
        F: FnMut(&Vec<KeyRef>, &mut FileState) -> Result<VisitorResult>,
    {
        self.store.cache()?;
        self.tracked.visit(self.store.store_view(), visitor)
    }

    /// Visit all tracked files in changed nodes with a visitor.
    pub fn visit_changed_tracked<F>(&mut self, visitor: &mut F) -> Result<()>
    where
        F: FnMut(&Vec<KeyRef>, &mut FileState) -> Result<VisitorResult>,
    {
        self.store.cache()?;
        self.tracked.visit_changed(self.store.store_view(), visitor)
    }

    /// Get the name and state of the first file in the tracked tree.
    pub fn get_first_tracked<'a>(&'a mut self) -> Result<Option<(Key, &'a FileState)>> {
        self.store.cache()?;
        self.tracked.get_first(self.store.store_view())
    }

    /// Get the name and state of the next file in the tracked tree after the named file.
    pub fn get_next_tracked<'a>(
        &'a mut self,
        name: KeyRef,
    ) -> Result<Option<(Key, &'a FileState)>> {
        self.tracked.get_next(self.store.store_view(), name)
    }

    pub fn has_tracked_dir(&mut self, name: KeyRef) -> Result<bool> {
        self.tracked.has_dir(self.store.store_view(), name)
    }

    /// Get an entry from the removed tree.
    pub fn get_removed<'a>(&'a mut self, name: KeyRef) -> Result<Option<&'a FileState>> {
        self.removed.get(self.store.store_view(), name)
    }

    /// Visit all removed files with a visitor.
    pub fn visit_removed<F>(&mut self, visitor: &mut F) -> Result<()>
    where
        F: FnMut(&Vec<KeyRef>, &mut FileState) -> Result<VisitorResult>,
    {
        self.removed.visit(self.store.store_view(), visitor)
    }

    /// Get the name and state of the first file in the removed tree.
    pub fn get_first_removed<'a>(&'a mut self) -> Result<Option<(Key, &'a FileState)>> {
        self.removed.get_first(self.store.store_view())
    }

    /// Get the name and state of the next file in the removed tree after the named file.
    pub fn get_next_removed<'a>(
        &'a mut self,
        name: KeyRef,
    ) -> Result<Option<(Key, &'a FileState)>> {
        self.removed.get_next(self.store.store_view(), name)
    }

    pub fn has_removed_dir(&mut self, name: KeyRef) -> Result<bool> {
        self.removed.has_dir(self.store.store_view(), name)
    }

    /// Visit all completed acceptable paths that match the given prefix.
    pub fn path_complete_tracked<FA, FV>(
        &mut self,
        prefix: KeyRef,
        full_paths: bool,
        acceptable: &FA,
        visitor: &mut FV,
    ) -> Result<()>
    where
        FA: Fn(&FileState) -> bool,
        FV: FnMut(&Vec<KeyRef>) -> Result<()>,
    {
        self.tracked.path_complete(
            self.store.store_view(),
            prefix,
            full_paths,
            acceptable,
            visitor,
        )
    }

    /// Visit all completed acceptable paths that match the given prefix.
    pub fn path_complete_removed<FA, FV>(
        &mut self,
        prefix: KeyRef,
        full_paths: bool,
        acceptable: &FA,
        visitor: &mut FV,
    ) -> Result<()>
    where
        FA: Fn(&FileState) -> bool,
        FV: FnMut(&Vec<KeyRef>) -> Result<()>,
    {
        self.removed.path_complete(
            self.store.store_view(),
            prefix,
            full_paths,
            acceptable,
            visitor,
        )
    }
}

#[cfg(test)]
mod tests {
    use crate::filestate::FileState;
    use crate::treedirstate::TreeDirstate;
    use tempdir::TempDir;

    fn make_state(state: u8) -> FileState {
        FileState::new(state, 0, 0, 0)
    }

    #[test]
    fn goodpath() {
        let dir = TempDir::new("dirstate_test").expect("create temp dir");
        let p = dir.path().join("store");
        let mut ds = TreeDirstate::new();
        ds.write_full(&p)
            .expect("can write full empty treedirstate");
        ds.add_file(b"dirA/file1", &make_state(b'n'))
            .expect("can add");
        ds.remove_file(b"dirA/file2", &make_state(b'r'))
            .expect("can remove");
        ds.write_delta().expect("can write delta");
        ds.add_file(b"dirA/file2", &make_state(b'n'))
            .expect("can add");
        ds.remove_file(b"dirA/file1", &make_state(b'r'))
            .expect("can remove");
        ds.write_delta().expect("can write delta");
        let ds_root = ds.root_id().unwrap();
        drop(ds);
        let mut ds2 = TreeDirstate::new();
        ds2.open(&p, ds_root).expect("can re-open");
        ds2.add_file(b"dirB/file3", &make_state(b'm'))
            .expect("can add");
        ds2.remove_file(b"dirC/file4", &make_state(b'r'))
            .expect("can remove");
        assert_eq!(ds2.get_tracked(b"dirA/file1").expect("can get"), None);
        assert_eq!(
            ds2.get_tracked(b"dirA/file2").expect("can get"),
            Some(&make_state(b'n'))
        );
        assert_eq!(
            ds2.get_removed(b"dirA/file1").expect("can get"),
            Some(&make_state(b'r'))
        );
        assert_eq!(ds2.get_removed(b"dirA/file2").expect("can get"), None);
        assert_eq!(ds2.tracked_count(), 2);
        assert_eq!(ds2.removed_count(), 2);
        ds2.drop_file(b"dirA/file1").expect("can drop");
        ds2.drop_file(b"dirA/file2").expect("can drop");
        ds2.write_delta().expect("can write delta");
        assert_eq!(ds2.tracked_count(), 1);
        assert_eq!(ds2.removed_count(), 1);
        ds2.clear();
        ds2.write_delta().expect("can write delta");
        assert_eq!(ds2.tracked_count(), 0);
        assert_eq!(ds2.removed_count(), 0);
    }
}
