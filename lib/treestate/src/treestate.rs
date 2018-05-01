// Copyright Facebook, Inc. 2017

use errors::Result;
use filestate::FileStateV2;
use filestore::FileStore;
use serialization::Serializable;
use std::io::Cursor;
use std::path::Path;
use store::{BlockId, Store, StoreView};
use tree::Tree;

/// `TreeState` uses a single tree to track an extended state of `TreeDirstate`.
/// See the comment about `FileStateV2` for the difference.
/// In short, `TreeState` combines dirstate and fsmonitor state.
pub struct TreeState {
    store: FileStore,
    tree: Tree<FileStateV2>,
    root: TreeStateRoot,
}

/// `TreeStateRoot` contains block id to the root `Tree`, and other metadata.
#[derive(Default)]
pub(crate) struct TreeStateRoot {
    pub version: u32,
    pub file_count: u32,
    pub tree_block_id: BlockId,
    pub watchman_clock: Box<[u8]>,
}

impl TreeState {
    /// Read `TreeState` from a file, or create an empty new `TreeState` if `root_id` is None.
    pub fn open<P: AsRef<Path>>(path: P, root_id: Option<BlockId>) -> Result<Self> {
        match root_id {
            Some(root_id) => {
                let store = FileStore::open(path)?;
                let root = {
                    let mut root_buf = Cursor::new(store.read(root_id)?);
                    TreeStateRoot::deserialize(&mut root_buf)?
                };
                let tree = Tree::open(root.tree_block_id, root.file_count);
                Ok(TreeState { store, tree, root })
            }
            None => {
                let store = FileStore::create(path)?;
                let root = TreeStateRoot::default();
                let tree = Tree::new();
                Ok(TreeState { store, tree, root })
            }
        }
    }

    /// Create or replace the existing entry.
    pub fn insert<K: AsRef<[u8]>>(&mut self, path: K, state: &FileStateV2) -> Result<()> {
        self.tree.add(&self.store, path.as_ref(), state)
    }

    pub fn remove<K: AsRef<[u8]>>(&mut self, path: K) -> Result<bool> {
        self.tree.remove(&self.store, path.as_ref())
    }

    pub fn get<K: AsRef<[u8]>>(&mut self, path: K) -> Result<Option<&FileStateV2>> {
        self.tree.get(&self.store, path.as_ref())
    }

    pub fn get_mut<K: AsRef<[u8]>>(&mut self, path: K) -> Result<Option<&mut FileStateV2>> {
        self.tree.get_mut(&self.store, path.as_ref())
    }

    pub fn len(&self) -> usize {
        self.tree.file_count() as usize
    }
}
