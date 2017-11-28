// Copyright Facebook, Inc. 2017
//! Directory State.

use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};
use errors::*;
use filestore::FileStore;
use std::io::{Cursor, Read, Write};
use std::path::Path;
use store::{BlockId, NullStore, Store, StoreView};
use tree::{Key, KeyRef, Storable, Tree};

/// Marker indicating that a block is probably a root node.
const MAGIC: &[u8] = b"////";
const MAGIC_LEN: usize = 4;

/// Selected backend implementation for the dirstate.
enum Backend {
    /// The dirstate is not currently backed by a file.
    Empty(NullStore),

    /// The dirstate is backed by a file on disk.
    File(FileStore),
}

impl Backend {
    pub fn store<'a>(&'a mut self) -> &'a mut Store {
        match *self {
            Backend::Empty(ref mut _null) => panic!("attempt to write to uninitialized dirstate"),
            Backend::File(ref mut file) => file,
        }
    }

    pub fn store_view<'a>(&'a self) -> &'a StoreView {
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

/// A dirstate object.  This contains the state of all files in the dirstate, stored in tree
/// structures, and backed by an append-only store on disk.
pub struct Dirstate<T> {
    /// The store currently in use by the Dirstate.
    store: Backend,

    /// The tree of tracked files.
    tracked: Tree<T>,

    /// The tree of removed files.
    removed: Tree<T>,

    /// The ID of the root block.
    root_id: Option<BlockId>,
}

impl<T: Storable + Clone> Dirstate<T> {
    /// Create a new, empty dirstate, with no backend store.
    pub fn new() -> Dirstate<T> {
        Dirstate {
            store: Backend::Empty(NullStore::new()),
            tracked: Tree::new(),
            removed: Tree::new(),
            root_id: None,
        }
    }

    /// Open an existing dirstate file.  The entries in the file will not be materialized from
    /// the disk until they are accessed.
    pub fn open<P: AsRef<Path>>(&mut self, filename: P, root_id: BlockId) -> Result<()> {
        let store = FileStore::open(filename)?;
        {
            let root_data = store.read(root_id)?;
            let mut root = Cursor::new(root_data);

            // Sanity check that this is a root
            let mut buffer = [0; MAGIC_LEN];
            root.read_exact(&mut buffer)?;
            if buffer != MAGIC {
                bail!(ErrorKind::InvalidStoreId(root_id.0));
            }

            let tracked_root_id = BlockId(root.read_u64::<BigEndian>()?);
            let tracked_file_count = root.read_u32::<BigEndian>()?;
            let removed_root_id = BlockId(root.read_u64::<BigEndian>()?);
            let removed_file_count = root.read_u32::<BigEndian>()?;
            self.tracked = Tree::open(tracked_root_id, tracked_file_count);
            self.removed = Tree::open(removed_root_id, removed_file_count);
        }
        self.store = Backend::File(store);
        self.root_id = Some(root_id);
        Ok(())
    }

    /// Write a new root block to the store.  This contains the identities of the tree roots
    /// and the tree sizes.
    fn write_root(&mut self) -> Result<()> {
        let store = self.store.store();
        let mut data = Vec::new();
        data.write(MAGIC)?;
        data.write_u64::<BigEndian>(self.tracked.root_id().unwrap().0)?;
        data.write_u32::<BigEndian>(self.tracked.file_count())?;
        data.write_u64::<BigEndian>(self.removed.root_id().unwrap().0)?;
        data.write_u32::<BigEndian>(self.removed.file_count())?;
        self.root_id = Some(store.append(&data)?);
        store.flush()?;
        Ok(())
    }

    /// Write a full copy of the dirstate out to a new file.
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

    /// Write updated entries in the dirstate to the store.
    pub fn write_delta(&mut self) -> Result<()> {
        {
            let store = self.store.store();
            self.tracked.write_delta(store)?;
            self.removed.write_delta(store)?;
        }
        self.write_root()
    }

    /// Clears all entries from the dirstate.
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

    /// Add or update a file entry in the dirstate.
    pub fn add_file(&mut self, name: KeyRef, state: &T) -> Result<()> {
        let store = self.store.store_view();
        self.removed.remove(store, name)?;
        self.tracked.add(store, name, state)?;
        Ok(())
    }

    /// Mark a file as removed in the dirstate.
    pub fn remove_file(&mut self, name: KeyRef, state: &T) -> Result<()> {
        let store = self.store.store_view();
        self.tracked.remove(store, name)?;
        self.removed.add(store, name, state)?;
        Ok(())
    }

    /// Drop a file from the dirstate.
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
    pub fn get_tracked<'a>(&'a mut self, name: KeyRef) -> Result<Option<&'a T>> {
        self.tracked.get(self.store.store_view(), name)
    }

    pub fn get_tracked_filtered_key<F>(
        &mut self,
        name: KeyRef,
        filter: &mut F,
    ) -> Result<Option<Key>>
    where
        F: FnMut(KeyRef) -> Result<Key>,
    {
        self.tracked
            .get_filtered_key(self.store.store_view(), name, filter)
    }

    /// Visit all tracked files with a visitor.
    pub fn visit_tracked<F>(&mut self, visitor: &mut F) -> Result<()>
    where
        F: FnMut(&Vec<KeyRef>, &mut T) -> Result<()>,
    {
        self.store.cache()?;
        self.tracked.visit(self.store.store_view(), visitor)
    }

    /// Visit all tracked files in changed nodes with a visitor.
    pub fn visit_changed_tracked<F>(&mut self, visitor: &mut F) -> Result<()>
    where
        F: FnMut(&Vec<KeyRef>, &mut T) -> Result<()>,
    {
        self.store.cache()?;
        self.tracked.visit_changed(self.store.store_view(), visitor)
    }

    /// Get the name and state of the first file in the tracked tree.
    pub fn get_first_tracked<'a>(&'a mut self) -> Result<Option<(Key, &'a T)>> {
        self.store.cache()?;
        self.tracked.get_first(self.store.store_view())
    }

    /// Get the name and state of the next file in the tracked tree after the named file.
    pub fn get_next_tracked<'a>(&'a mut self, name: KeyRef) -> Result<Option<(Key, &'a T)>> {
        self.tracked.get_next(self.store.store_view(), name)
    }

    pub fn has_tracked_dir(&mut self, name: KeyRef) -> Result<bool> {
        self.tracked.has_dir(self.store.store_view(), name)
    }

    /// Get an entry from the removed tree.
    pub fn get_removed<'a>(&'a mut self, name: KeyRef) -> Result<Option<&'a T>> {
        self.removed.get(self.store.store_view(), name)
    }

    /// Visit all removed files with a visitor.
    pub fn visit_removed<F>(&mut self, visitor: &mut F) -> Result<()>
    where
        F: FnMut(&Vec<KeyRef>, &mut T) -> Result<()>,
    {
        self.removed.visit(self.store.store_view(), visitor)
    }

    /// Get the name and state of the first file in the removed tree.
    pub fn get_first_removed<'a>(&'a mut self) -> Result<Option<(Key, &'a T)>> {
        self.removed.get_first(self.store.store_view())
    }

    /// Get the name and state of the next file in the removed tree after the named file.
    pub fn get_next_removed<'a>(&'a mut self, name: KeyRef) -> Result<Option<(Key, &'a T)>> {
        self.removed.get_next(self.store.store_view(), name)
    }

    pub fn has_removed_dir(&mut self, name: KeyRef) -> Result<bool> {
        self.removed.has_dir(self.store.store_view(), name)
    }

    pub fn clear_filtered_keys(&mut self) {
        self.tracked.clear_filtered_keys();
    }
}

#[cfg(test)]
mod tests {
    use dirstate::Dirstate;
    use tempdir::TempDir;
    use tree::Storable;
    use std::io::{Read, Write};
    use byteorder::{ReadBytesExt, WriteBytesExt};
    use errors::*;

    #[derive(PartialEq, Clone, Debug)]
    struct State(char);

    impl Storable for State {
        fn write(&self, w: &mut Write) -> Result<()> {
            w.write_u8(self.0 as u8)?;
            Ok(())
        }

        fn read(r: &mut Read) -> Result<State> {
            Ok(State(r.read_u8()? as char))
        }
    }

    #[test]
    fn goodpath() {
        let dir = TempDir::new("dirstate_test").expect("create temp dir");
        let p = dir.path().join("store");
        let mut ds = Dirstate::<State>::new();
        ds.write_full(&p).expect("can write full empty dirstate");
        ds.add_file(b"dirA/file1", &State('n')).expect("can add");
        ds.remove_file(b"dirA/file2", &State('r'))
            .expect("can remove");
        ds.write_delta().expect("can write delta");
        ds.add_file(b"dirA/file2", &State('n')).expect("can add");
        ds.remove_file(b"dirA/file1", &State('r'))
            .expect("can remove");
        ds.write_delta().expect("can write delta");
        let ds_root = ds.root_id().unwrap();
        drop(ds);
        let mut ds2 = Dirstate::<State>::new();
        ds2.open(&p, ds_root).expect("can re-open");
        ds2.add_file(b"dirB/file3", &State('m')).expect("can add");
        ds2.remove_file(b"dirC/file4", &State('r'))
            .expect("can remove");
        assert_eq!(ds2.get_tracked(b"dirA/file1").expect("can get"), None);
        assert_eq!(
            ds2.get_tracked(b"dirA/file2").expect("can get"),
            Some(&State('n'))
        );
        assert_eq!(
            ds2.get_removed(b"dirA/file1").expect("can get"),
            Some(&State('r'))
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
