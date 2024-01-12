/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::fmt;
use std::io::Cursor;
use std::iter::Iterator;
use std::ops::Deref;
use std::path::Path;
use std::path::PathBuf;
use std::str::FromStr;

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Result;
use types::HgId;
use util::path::create_dir;

use crate::filestate::FileStateV2;
use crate::filestate::StateFlags;
use crate::filestore::FileStore;
use crate::legacy_eden_dirstate::read_eden_dirstate;
use crate::legacy_eden_dirstate::write_eden_dirstate;
use crate::metadata::Metadata;
use crate::root::TreeStateRoot;
use crate::serialization::Serializable;
use crate::store::BlockId;
use crate::store::Store;
use crate::store::StoreView;
use crate::tree::AggregatedState;
use crate::tree::Key;
use crate::tree::KeyRef;
use crate::tree::Node;
use crate::tree::Tree;
use crate::tree::VisitorResult;

const FILTER_LOWERCASE: u64 = 0x1;
/// `TreeState` uses a single tree to track an extended state of `TreeDirstate`.
/// See the comment about `FileStateV2` for the difference.
/// In short, `TreeState` combines dirstate and fsmonitor state.
pub struct TreeState {
    store: FileStore,
    tree: Tree<FileStateV2>,
    root: TreeStateRoot,
    original_root_id: BlockId,
    // eden_dirstate_path is only used in the case the case that the treestate is
    // wrapping a legacy eden dirstate which is necessary for EdenFS compatility.
    // TODO: Remove once EdenFS has migrated to treestate.
    eden_dirstate_path: Option<PathBuf>,
    case_sensitive: bool,
    pending_change_count: u64,
}

impl fmt::Debug for TreeState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "TreeState")
    }
}

impl TreeState {
    /// Read `TreeState` from a file, or create an empty new `TreeState` if `root_id` is None.
    pub fn open<P: AsRef<Path>>(path: P, root_id: BlockId, case_sensitive: bool) -> Result<Self> {
        let path = path.as_ref();
        tracing::trace!(target: "treestate::open", "creating filestore at {path:?}");
        let store = FileStore::open(path)?;
        let root = {
            tracing::trace!(target: "treestate::open", "reading root data");
            let mut root_buf = Cursor::new(store.read(root_id)?);
            tracing::trace!(target: "treestate::open", "deserializing root data");
            TreeStateRoot::deserialize(&mut root_buf)?
        };
        tracing::trace!(target: "treestate::open", "constructing tree");
        let tree = Tree::open(root.tree_block_id(), root.file_count());
        Ok(TreeState {
            store,
            tree,
            root,
            original_root_id: root_id,
            eden_dirstate_path: None,
            case_sensitive,
            pending_change_count: 0,
        })
    }

    pub fn new(directory: &Path, case_sensitive: bool) -> Result<(Self, BlockId)> {
        tracing::trace!(target: "treestate::create", "creating directory {directory:?}");
        create_dir(directory)?;
        let name = format!("{:x}", uuid::Uuid::new_v4());
        let path = directory.join(name);
        tracing::trace!(target: "treestate::create", "creating filestore {path:?}");
        let mut store = FileStore::create(&path)?;
        let _lock = store.lock()?;
        let root = TreeStateRoot::default();
        let tree = Tree::new();
        let mut treestate = TreeState {
            store,
            tree,
            root,
            original_root_id: BlockId(0),
            eden_dirstate_path: None,
            case_sensitive,
            pending_change_count: 0,
        };
        tracing::trace!(target: "treestate::create", "flushing treestate");
        let root_id = treestate.flush()?;

        tracing::trace!(target: "treestate::create", "treestate created");
        Ok((treestate, root_id))
    }

    pub fn reset(&mut self) -> Result<BlockId> {
        let directory = self
            .store
            .path()
            .and_then(|p| p.parent())
            .context("when getting the store directory for resetting treestate")?;
        let (new_treestate, root_id) = Self::new(directory, self.case_sensitive)?;
        *self = new_treestate;
        Ok(root_id)
    }

    /// Provides the ability to populate a treestate from a legacy eden dirstate.
    /// Clean up once EdenFS has been migrated from legacy dirstate to
    /// treestate.
    /// N.B: A legacy eden dirstate has a different binary format to a legacy
    /// dirstate.
    pub fn from_eden_dirstate<P: AsRef<Path>>(
        eden_dirstate_path: P,
        case_sensitive: bool,
    ) -> Result<Self> {
        let store = FileStore::in_memory()?;
        let root = TreeStateRoot::default();
        let tree = Tree::new();

        let (metadata, entries) = read_eden_dirstate(eden_dirstate_path.as_ref())?;

        let path = eden_dirstate_path.as_ref().to_path_buf();
        let mut treestate = TreeState {
            store,
            tree,
            root,
            original_root_id: BlockId(0),
            eden_dirstate_path: Some(path),
            case_sensitive,
            pending_change_count: 0,
        };

        treestate.set_metadata(metadata)?;

        for (key, state) in entries {
            treestate.insert(key, &state)?;
        }

        Ok(treestate)
    }

    pub fn path(&self) -> Option<&Path> {
        self.store.path()
    }

    pub fn file_name(&self) -> Result<String> {
        Ok(self
            .path()
            .ok_or_else(|| anyhow!("missing store path for TreeState"))?
            .file_name()
            .ok_or_else(|| anyhow!("missing file name for TreeState"))?
            .to_string_lossy()
            .to_string())
    }

    /// The root_id from when the treestate was loaded or last saved. Gets updated upon flush.
    pub fn original_root_id(&self) -> BlockId {
        self.original_root_id
    }

    pub fn dirty(&self) -> bool {
        self.tree.dirty() || self.root.dirty()
    }

    /// Return approximate number of pending insertions, removals and mutations.
    pub fn pending_change_count(&self) -> u64 {
        self.pending_change_count
    }

    /// Flush dirty entries. Return new `root_id` that can be passed to `open`.
    pub fn flush(&mut self) -> Result<BlockId> {
        let _lock = self.store.lock()?;
        let tree_block_id = { self.tree.write_delta(&mut self.store)? };
        self.write_root(tree_block_id)
    }

    /// Save as a new file.
    pub fn write_new<P: AsRef<Path>>(&mut self, directory: P) -> Result<BlockId> {
        let name = format!("{:x}", uuid::Uuid::new_v4());
        let path = directory.as_ref().join(name);
        tracing::trace!(target: "treestate::write_new", ?path);
        let mut new_store = FileStore::create(path)?;
        let _lock = new_store.lock()?;
        let tree_block_id = self.tree.write_full(&mut new_store, &self.store)?;
        self.store = new_store;
        let root_id = self.write_root(tree_block_id)?;
        Ok(root_id)
    }

    fn write_root(&mut self, tree_block_id: BlockId) -> Result<BlockId> {
        self.root.set_tree_block_id(tree_block_id);
        self.root.set_file_count(self.len() as u32);

        let mut root_buf = Vec::new();
        self.root.serialize(&mut root_buf)?;
        let result = self.store.append(&root_buf)?;
        self.store.flush()?;

        // TODO: Clean up once we migrate EdenFS to TreeState and no longer
        // need to write to legacy eden dirstate format.
        if let Some(eden_dirstate_path) = self.eden_dirstate_path.clone() {
            let metadata = self.metadata()?;
            let entries = self.flatten_tree()?;
            write_eden_dirstate(&eden_dirstate_path, metadata, entries)?;
        }

        self.original_root_id = result;
        self.pending_change_count = 0;
        Ok(result)
    }

    fn flatten_tree(&mut self) -> Result<HashMap<Box<[u8]>, FileStateV2>> {
        let mut results = HashMap::with_capacity(self.len());
        self.visit(
            &mut |path_components, state| {
                results.insert(path_components.concat().into_boxed_slice(), state.clone());
                Ok(VisitorResult::NotChanged)
            },
            &|_, _| true, // Visit all directories
            &|_, _| true, // Visit all files
        )?;

        Ok(results)
    }

    /// Create or replace the existing entry.
    pub fn insert<K: AsRef<[u8]>>(&mut self, path: K, state: &FileStateV2) -> Result<()> {
        let res = self.tree.add(&self.store, path.as_ref(), state);
        if res.is_ok() {
            self.pending_change_count += 1;
        }
        res
    }

    pub fn remove<K: AsRef<[u8]>>(&mut self, path: K) -> Result<bool> {
        let res = self.tree.remove(&self.store, path.as_ref());
        if res.is_ok() {
            self.pending_change_count += 1;
        }
        res
    }

    pub fn get<K: AsRef<[u8]>>(&mut self, path: K) -> Result<Option<&FileStateV2>> {
        self.tree.get(&self.store, path.as_ref())
    }

    pub fn get_keys_ignorecase<K: AsRef<[u8]>>(&mut self, path: K) -> Result<Vec<Key>> {
        self.get_keys_ignorecase_with_prefix(path).map(|r| r.0)
    }

    fn get_keys_ignorecase_with_prefix<K: AsRef<[u8]>>(
        &mut self,
        path: K,
    ) -> Result<(Vec<Key>, Option<Key>)> {
        fn map_lowercase(k: KeyRef) -> Result<Key> {
            let s = std::str::from_utf8(k);
            Ok(if let Ok(s) = s {
                s.to_lowercase().into_bytes().into_boxed_slice()
            } else {
                k.to_vec().into_boxed_slice()
            })
        }
        self.get_filtered_key_with_prefix(
            &map_lowercase(path.as_ref())?,
            &mut map_lowercase,
            FILTER_LOWERCASE,
        )
    }

    pub fn normalize_path_and_get<'a>(
        &mut self,
        path: &'a [u8],
    ) -> Result<(Cow<'a, [u8]>, Option<FileStateV2>)> {
        if self.case_sensitive {
            return Ok((Cow::Borrowed(path), self.get(path)?.cloned()));
        }

        // Optimistic check for normal case where path matches exactly.
        // Case insensitive filesystems are normally still case preserving.
        if let Some(state) = self.get(path)? {
            if state.state.intersects(StateFlags::EXIST_NEXT) {
                return Ok((Cow::Borrowed(path), Some(state.clone())));
            }
        }

        let (keys, prefix) = self.get_keys_ignorecase_with_prefix(path)?;

        let mut best = None;
        for key in keys {
            let state = match self.get(&key)? {
                None => continue,
                Some(state) => state.clone(),
            };

            // If there are multiple matches, prefer the format for the version that still exists.
            if best.is_none() || state.state.intersects(StateFlags::EXIST_NEXT) {
                best = Some((Cow::Owned(key.into()), state));
            }
        }

        match best {
            Some((path, state)) => Ok((path, Some(state))),
            None => match prefix {
                None => Ok((Cow::Borrowed(path), None)),
                Some(prefix) => Ok((Cow::Owned([&prefix, &path[prefix.len()..]].concat()), None)),
            },
        }
    }

    pub fn normalize_path<'a>(&mut self, path: &'a [u8]) -> Result<Cow<'a, [u8]>> {
        Ok(self.normalize_path_and_get(path)?.0)
    }

    pub fn normalized_get<K: AsRef<[u8]>>(&mut self, path: K) -> Result<Option<FileStateV2>> {
        Ok(self.normalize_path_and_get(path.as_ref())?.1)
    }

    /// Get the aggregated state of a directory. This is useful, for example, to tell if a
    /// directory only contains removed files.
    pub fn get_dir<K: AsRef<[u8]>>(&mut self, path: K) -> Result<Option<AggregatedState>> {
        self.tree.get_dir(&self.store, path.as_ref())
    }

    pub fn len(&self) -> usize {
        self.tree.file_count() as usize
    }

    pub fn set_metadata_bytes<T: AsRef<[u8]>>(&mut self, metadata: T) {
        self.root
            .set_metadata(Vec::from(metadata.as_ref()).into_boxed_slice());
    }

    pub fn set_metadata(&mut self, metadata: BTreeMap<String, String>) -> Result<()> {
        let mut buf = Vec::new();
        Metadata(metadata).serialize(&mut buf)?;
        self.set_metadata_bytes(buf);
        Ok(())
    }

    pub fn metadata_bytes(&self) -> &[u8] {
        self.root.metadata().deref()
    }

    pub fn metadata(&self) -> Result<BTreeMap<String, String>> {
        let metadata = Metadata::deserialize(&mut self.metadata_bytes())?;
        Ok(metadata.0)
    }

    pub fn update_metadata(&mut self, new: &[(String, Option<String>)]) -> Result<()> {
        let mut metadata = self.metadata()?;

        for (key, value) in new.iter() {
            match value {
                Some(value) => metadata.insert(key.to_string(), value.to_string()),
                None => metadata.remove(key),
            };
        }

        let mut buf = Vec::new();
        Metadata(metadata).serialize(&mut buf)?;
        self.root.set_metadata(buf.into_boxed_slice());
        Ok(())
    }

    pub fn parents<'a>(&'a self) -> impl Iterator<Item = Result<HgId>> + 'a {
        (1..).map_while(|i| match self.metadata() {
            Err(err) => Some(Err(err)),
            Ok(metadata) => metadata
                .get(&format!("p{}", i))
                .map(|parent_hash| HgId::from_str(parent_hash).map_err(|e| e.into())),
        })
    }

    pub fn set_parents(&mut self, parents: &mut dyn Iterator<Item = &HgId>) -> Result<()> {
        let mut values: Vec<(String, Option<String>)> = Vec::with_capacity(2);
        for (i, p) in parents.enumerate() {
            // i+1 because parents are 1-indexed
            values.push((format!("p{}", i + 1), Some(p.to_string())));
        }
        // Set p1 or p2 to None to remove it from the metadata if necessary.
        if values.is_empty() {
            values.push(("p1".to_string(), None));
        }
        if values.len() == 1 {
            values.push(("p2".to_string(), None));
        }
        self.update_metadata(&values)
    }

    pub fn has_dir<P: AsRef<[u8]>>(&mut self, path: P) -> Result<bool> {
        self.tree.has_dir(&self.store, path.as_ref())
    }

    pub fn visit<F, VD, VF>(
        &mut self,
        visitor: &mut F,
        visit_dir: &VD,
        visit_file: &VF,
    ) -> Result<()>
    where
        F: FnMut(&Vec<&[u8]>, &mut FileStateV2) -> Result<VisitorResult>,
        VD: Fn(&Vec<KeyRef>, &Node<FileStateV2>) -> bool,
        VF: Fn(&Vec<KeyRef>, &FileStateV2) -> bool,
    {
        self.pending_change_count +=
            self.tree
                .visit_advanced(&self.store, visitor, visit_dir, visit_file)?;
        Ok(())
    }

    pub fn get_filtered_key<F>(
        &mut self,
        name: KeyRef,
        filter: &mut F,
        filter_id: u64,
    ) -> Result<Vec<Key>>
    where
        F: FnMut(KeyRef) -> Result<Key>,
    {
        self.tree
            .get_filtered_key(&self.store, name, filter, filter_id)
            .map(|r| r.0)
    }

    fn get_filtered_key_with_prefix<F>(
        &mut self,
        name: KeyRef,
        filter: &mut F,
        filter_id: u64,
    ) -> Result<(Vec<Key>, Option<Key>)>
    where
        F: FnMut(KeyRef) -> Result<Key>,
    {
        self.tree
            .get_filtered_key(&self.store, name, filter, filter_id)
    }

    pub fn path_complete<FA, FV>(
        &mut self,
        prefix: KeyRef,
        full_paths: bool,
        acceptable: &FA,
        visitor: &mut FV,
    ) -> Result<()>
    where
        FA: Fn(&FileStateV2) -> bool,
        FV: FnMut(&Vec<KeyRef>) -> Result<()>,
    {
        self.tree
            .path_complete(&self.store, prefix, full_paths, acceptable, visitor)
    }

    // Distrust changed files with a mtime of `fsnow`. Rewrite their mtime to -1.
    // See mercurial/pure/parsers.py:pack_dirstate in core Mercurial for motivation.
    // Basically, this is required for the following case:
    //
    // $ hg update rev; write foo; hg commit -m update-foo
    //
    //   Time (second) | 0   | 1           |
    //   hg update       ...----|
    //   write foo               |--|
    //   hg commit                   |---...
    //
    // If "write foo" changes a file without changing its mtime and size, the file
    // change won't be detected. Therefore if mtime is `fsnow`, reset it to a different
    // value and mark it as NEED_CHECK, at the end of update to workaround the issue.
    // Here, hg assumes nobody else is touching the working copy when it holds wlock
    // (ex. during second 0).
    //
    // This is used before "flush" or "saveas".
    //
    // Note: In TreeState's case, NEED_CHECK might mean "perform a quick mtime check",
    // or "perform a content check" depending on the caller. Be careful when removing
    // "mtime = -1" statement.
    pub fn invalidate_mtime(&mut self, now: i32) -> Result<()> {
        self.visit(
            &mut |path, state| {
                if state.mtime >= now {
                    state.mtime = -1;
                    tracing::trace!(
                        "invalidate_mtime setting NEED_CHECK for {}",
                        util::utf8::escape_non_utf8(&path.concat())
                    );
                    state.state |= StateFlags::NEED_CHECK;
                    Ok(VisitorResult::Changed)
                } else {
                    Ok(VisitorResult::NotChanged)
                }
            },
            &|_, dir| {
                if !dir.is_changed() {
                    false
                } else {
                    match dir.get_aggregated_state() {
                        Some(x) => x
                            .union
                            .intersects(StateFlags::EXIST_P1 | StateFlags::EXIST_P2),
                        None => true,
                    }
                }
            },
            &|_, file| {
                file.state
                    .intersects(StateFlags::EXIST_P1 | StateFlags::EXIST_P2)
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use rand::Rng;
    use rand::SeedableRng;
    use rand_chacha::ChaChaRng;
    use tempfile::tempdir;

    use super::*;
    use crate::filestate::StateFlags;

    #[test]
    fn test_new() {
        let dir = tempdir().expect("tempdir");
        let state = TreeState::new(dir.path(), true).expect("open").0;
        assert!(state.metadata_bytes().is_empty());
        assert_eq!(state.len(), 0);
    }

    #[test]
    fn test_empty_flush() {
        let dir = tempdir().expect("tempdir");
        let mut state = TreeState::new(dir.path(), true).expect("open").0;
        let block_id = state.flush().expect("flush");
        let state = TreeState::open(dir.path().join(state.file_name().unwrap()), block_id, true)
            .expect("open");
        assert!(state.metadata_bytes().is_empty());
        assert_eq!(state.len(), 0);
    }

    #[test]
    fn test_empty_write_as() {
        let dir = tempdir().expect("tempdir");
        let mut state = TreeState::new(dir.path(), true).expect("open").0;
        let block_id = state.write_new(dir.path()).expect("write_as");
        let state = TreeState::open(dir.path().join(state.file_name().unwrap()), block_id, true)
            .expect("open");
        assert!(state.metadata_bytes().is_empty());
        assert_eq!(state.len(), 0);
    }

    #[test]
    fn test_set_metadata() {
        let dir = tempdir().expect("tempdir");
        let mut state = TreeState::new(dir.path(), true).expect("open").0;
        state.set_metadata_bytes(b"foobar");
        let orig_name = state.file_name().unwrap();
        let block_id1 = state.flush().expect("flush");
        let block_id2 = state.write_new(dir.path()).expect("write_as");
        let new_name = state.file_name().unwrap();
        let state = TreeState::open(dir.path().join(orig_name), block_id1, true).expect("open");
        assert_eq!(state.metadata_bytes()[..], b"foobar"[..]);
        let state = TreeState::open(dir.path().join(new_name), block_id2, true).expect("open");
        assert_eq!(state.metadata_bytes()[..], b"foobar"[..]);
    }

    #[test]
    fn test_update_metadata() {
        let dir = tempdir().expect("tempdir");
        let mut state = TreeState::new(dir.path(), true).expect("open").0;
        state
            .update_metadata(&[
                ("key1".to_string(), Some("value1".to_string())),
                ("key2".to_string(), Some("value2".to_string())),
            ])
            .unwrap();
        assert_eq!(
            state.metadata().unwrap().get("key1").cloned(),
            Some("value1".to_string())
        );
        assert_eq!(
            state.metadata().unwrap().get("key2").cloned(),
            Some("value2".to_string())
        );

        state
            .update_metadata(&[("key1".to_string(), Some("value1.b".to_string()))])
            .unwrap();
        assert_eq!(
            state.metadata().unwrap().get("key1").cloned(),
            Some("value1.b".to_string())
        );
        assert_eq!(
            state.metadata().unwrap().get("key2").cloned(),
            Some("value2".to_string())
        );

        state
            .update_metadata(&[
                ("key1".to_string(), Some("value1.c".to_string())),
                ("key2".to_string(), None),
            ])
            .unwrap();
        assert_eq!(
            state.metadata().unwrap().get("key1").cloned(),
            Some("value1.c".to_string())
        );
        assert_eq!(state.metadata().unwrap().get("key2"), None);
    }

    // Some random paths extracted from fb-ext, plus some manually added entries, shuffled.
    const SAMPLE_PATHS: [&[u8]; 21] = [
        b".fbarcanist",
        b"phabricator/phabricator_graphql_client_urllib.pyc",
        b"ext3rd/__init__.py",
        b"ext3rd/.git/objects/14/8f179e7e702ddedb54c53f2726e7f81b14a33f",
        b"rust/radixbuf/.git/objects/pack/pack-c0bc37a255e59f5563de9a76013303d8df46a659.idx",
        b".hg/shelved/default-106.patch",
        b"rust/radixbuf/.git/objects/20/94e0274ba1ef2ec30de884e3ca4d7093838064",
        b"rust/radixbuf/.git/hooks/prepare-commit-msg.sample",
        b"rust/radixbuf/.git/objects/b3/9acb828290b77704cc44e748d6e7d4a528d6ae",
        b"scripts/lint.py",
        b".fbarcanist/unit/MercurialTestEngine.php",
        b".hg/shelved/default-37.patch",
        b"rust/radixbuf/.git/objects/01/d8e75b3bae0819c4095ae96ebdc889e9e5d806",
        b"ext3rd/fastannotate/error.py",
        b"rust/radixbuf/.git/objects/pack/pack-c0bc37a255e59f5563de9a76013303d8df46a659.pack",
        b"distutils_rust/__init__.py",
        b".editorconfig",
        b"rust/radixbuf/.git/objects/01/89a583d7e9aff802cdfed3ff3cc3a473253281",
        b"ext3rd/fastannotate/commands.py",
        b"distutils_rust/__init__.pyc",
        b"rust/radixbuf/.git/objects/b3/9b2824f47b66462e92ffa4f978bc95f5fdad2e",
    ];

    fn new_treestate<P: AsRef<Path>>(directory: P) -> TreeState {
        let mut state = TreeState::new(directory.as_ref(), true).expect("open").0;
        let mut rng = ChaChaRng::from_seed([0; 32]);
        for path in &SAMPLE_PATHS {
            let file = rng.gen();
            state.insert(path, &file).expect("insert");
        }
        state
    }

    #[test]
    fn test_insert() {
        let dir = tempdir().expect("tempdir");
        let mut state = new_treestate(dir.path());
        let mut rng = ChaChaRng::from_seed([0; 32]);
        for path in &SAMPLE_PATHS {
            let file: FileStateV2 = rng.gen();
            assert_eq!(state.get(path).unwrap().unwrap(), &file);
        }
        assert_eq!(state.len(), SAMPLE_PATHS.len());
    }

    #[test]
    fn test_remove() {
        let dir = tempdir().expect("tempdir");
        let mut state = new_treestate(dir.path());
        for path in &SAMPLE_PATHS {
            assert!(state.remove(path).unwrap())
        }
        for path in &SAMPLE_PATHS {
            assert!(!state.remove(path).unwrap())
        }
        assert_eq!(state.len(), 0);
    }

    #[test]
    fn test_non_empty_flush() {
        let dir = tempdir().expect("tempdir");
        let mut state = new_treestate(dir.path());
        let block_id = state.flush().expect("flush");
        let mut state =
            TreeState::open(dir.path().join(state.file_name().unwrap()), block_id, true)
                .expect("open");
        let mut rng = ChaChaRng::from_seed([0; 32]);
        for path in &SAMPLE_PATHS {
            let file: FileStateV2 = rng.gen();
            assert_eq!(state.get(path).unwrap().unwrap(), &file);
        }
        assert_eq!(state.len(), SAMPLE_PATHS.len());
    }

    #[test]
    fn test_non_empty_write_as() {
        let dir = tempdir().expect("tempdir");
        let mut state = new_treestate(dir.path());
        let block_id = state.write_new(dir.path()).expect("write_as");
        let mut state =
            TreeState::open(dir.path().join(state.file_name().unwrap()), block_id, true)
                .expect("open");
        let mut rng = ChaChaRng::from_seed([0; 32]);
        for path in &SAMPLE_PATHS {
            let file: FileStateV2 = rng.gen();
            assert_eq!(state.get(path).unwrap().unwrap(), &file);
        }
        assert_eq!(state.len(), SAMPLE_PATHS.len());
    }

    #[test]
    fn test_has_dir() {
        let dir = tempdir().expect("tempdir");
        let mut state = new_treestate(dir.path());
        assert!(state.has_dir(b"/").unwrap());
        assert!(state.has_dir(b"ext3rd/").unwrap());
        assert!(!state.has_dir(b"ext4th/").unwrap());
        assert!(state.has_dir(b"rust/radixbuf/.git/objects/").unwrap());
        assert!(!state.has_dir(b"rust/radixbuf/.git2/objects/").unwrap());
    }

    #[test]
    fn test_get_keys_ignorecase() {
        let dir = tempdir().expect("tempdir");
        let mut state = new_treestate(dir.path());
        let expected = vec![b"ext3rd/__init__.py".to_vec().into_boxed_slice()];
        assert_eq!(
            state.get_keys_ignorecase(b"ext3rd/__init__.py").unwrap(),
            expected
        );
        assert_eq!(
            state.get_keys_ignorecase(b"exT3rd/__init__.py").unwrap(),
            expected
        );
        assert_eq!(
            state.get_keys_ignorecase(b"ext3rd/__Init__.py").unwrap(),
            expected
        );
    }

    #[test]
    fn test_normalize_casesensitive() {
        let dir = tempdir().expect("tempdir");
        let mut state = TreeState::new(dir.as_ref(), true).expect("open").0;

        let mut rng = ChaChaRng::from_seed([0; 32]);
        let mut file = rng.gen();
        state.insert(b"dir/file", &file).unwrap();
        assert_eq!(
            state.normalize_path(b"dir/file").unwrap().as_ref(),
            b"dir/file"
        );
        assert_eq!(
            state.normalize_path(b"dir/FILE").unwrap().as_ref(),
            b"dir/FILE"
        );
        assert_eq!(
            state.normalize_path(b"DIR/file").unwrap().as_ref(),
            b"DIR/file"
        );

        file.state = StateFlags::EXIST_NEXT;
        state.insert(b"dir/RENAME", &file).unwrap();
        file.state = StateFlags::EXIST_P1;
        state.insert(b"dir/rename", &file).unwrap();
        assert_eq!(
            state.normalize_path(b"dir/rename").unwrap().as_ref(),
            b"dir/rename"
        );
        assert_eq!(
            state.normalize_path(b"dir/RENAME").unwrap().as_ref(),
            b"dir/RENAME"
        );
    }

    #[test]
    fn test_normalize_incasesensitive() {
        let dir = tempdir().expect("tempdir");
        let mut state = TreeState::new(dir.as_ref(), false).expect("open").0;

        let mut rng = ChaChaRng::from_seed([0; 32]);
        let mut file = rng.gen();
        state.insert(b"dir/file", &file).unwrap();
        assert_eq!(
            std::str::from_utf8(state.normalize_path(b"dir/file").unwrap().as_ref()).unwrap(),
            "dir/file"
        );
        assert_eq!(
            std::str::from_utf8(state.normalize_path(b"dir/FILE").unwrap().as_ref()).unwrap(),
            "dir/file"
        );
        assert_eq!(
            std::str::from_utf8(state.normalize_path(b"DIR/file").unwrap().as_ref()).unwrap(),
            "dir/file"
        );

        // Choose paths that will exist over paths that used to exist.
        file.state = StateFlags::EXIST_NEXT;
        state.insert(b"dir/RENAME", &file).unwrap();
        file.state = StateFlags::EXIST_P1;
        state.insert(b"dir/rename", &file).unwrap();
        assert_eq!(
            std::str::from_utf8(state.normalize_path(b"dir/rename").unwrap().as_ref()).unwrap(),
            "dir/RENAME"
        );
        assert_eq!(
            std::str::from_utf8(state.normalize_path(b"dir/RENAME").unwrap().as_ref()).unwrap(),
            "dir/RENAME"
        );
    }

    #[test]
    fn test_parents() {
        let dir = tempdir().expect("tempdir");
        let mut state = TreeState::new(dir.path(), true).expect("open").0;
        let orig_name = state.file_name().unwrap();
        let mut rng = ChaChaRng::from_seed([0; 32]);

        let p1 = HgId::random(&mut rng);
        let p2 = HgId::random(&mut rng);
        let p3 = HgId::random(&mut rng);

        state.set_parents(&mut [p1].iter()).unwrap();
        assert_eq!(
            state.parents().collect::<Result<Vec<_>>>().unwrap(),
            [p1].to_vec()
        );

        state.set_parents(&mut [p1, p2].iter()).unwrap();
        assert_eq!(
            state.parents().collect::<Result<Vec<_>>>().unwrap(),
            [p1, p2].to_vec()
        );

        state.set_parents(&mut [p1, p3].iter()).unwrap();
        assert_eq!(
            state.parents().collect::<Result<Vec<_>>>().unwrap(),
            [p1, p3].to_vec()
        );

        let block_id = state.flush().expect("flush");

        let state = TreeState::open(dir.path().join(orig_name), block_id, true).expect("open");
        assert_eq!(
            state.parents().collect::<Result<Vec<_>>>().unwrap(),
            [p1, p3].to_vec()
        );
    }

    #[test]
    fn test_concurrent_writes() {
        check_concurrent_writes(&[b"a"], &[b"b"]);
        check_concurrent_writes(&[b"a/1"], &[b"a/2"]);

        let paths1 = SAMPLE_PATHS.into_iter().take(10).collect::<Vec<_>>();
        let paths2 = SAMPLE_PATHS.into_iter().rev().take(1).collect::<Vec<_>>();
        check_concurrent_writes(&paths1, &paths2);

        let paths2 = SAMPLE_PATHS.into_iter().rev().take(10).collect::<Vec<_>>();
        check_concurrent_writes(&paths1, &paths2);
    }

    // Test appending paths1, and paths2 "concurrently".
    // Paths should not overlap.
    fn check_concurrent_writes(paths1: &[&'static [u8]], paths2: &[&'static [u8]]) {
        let dir = tempdir().unwrap();
        let dir_path = dir.path();

        // Prepare initial state.
        let (path, root_id) = {
            let mut state = new_treestate(dir_path);
            let root_id = state.flush().unwrap();
            (dir_path.join(state.file_name().unwrap()), root_id)
        };

        // Start M threads for concurrent writes.
        // Each thread writes N updates for paths, and returns the root_id and state to check for
        // the paths.
        const M: usize = 10;
        const N: usize = 20;
        let threads = (0..M)
            .map(|i| {
                let path = path.to_path_buf();
                let paths = if i & 1 == 0 { paths1 } else { paths2 };
                let paths = paths.to_vec();
                std::thread::spawn(move || {
                    let mut state = TreeState::open(&path, root_id, true).unwrap();
                    let mut file_state = FileStateV2 {
                        size: (i * N) as i32,
                        ..Default::default()
                    };
                    let mut result = Vec::new();
                    for _ in 0..N {
                        // Change `file_state` so `result` contains different `file_state`s to
                        // check. This makes the test more interesting.
                        file_state.mtime += 1;
                        for p in &paths {
                            state.insert(p, &file_state).unwrap();
                        }
                        let root_id = state.flush().unwrap();
                        result.push((root_id, file_state.clone()));
                    }
                    (paths, result)
                })
            })
            .collect::<Vec<_>>();

        let to_check: Vec<(Vec<&[u8]>, Vec<(BlockId, FileStateV2)>)> =
            threads.into_iter().map(|t| t.join().unwrap()).collect();

        // Check that things can be read properly (aka. no "invalid store id: ..." errors),
        // and are written properly (file_state1 and file_state2).
        for (paths, root_id_file_states) in to_check {
            for (root_id, file_state) in root_id_file_states {
                let mut state = TreeState::open(&path, root_id, true).unwrap();
                for p in &paths {
                    let got = state.get(p).unwrap().unwrap();
                    assert_eq!(got, &file_state);
                }
            }
        }
    }

    #[test]
    fn test_pending_change_count() -> Result<()> {
        let dir = tempdir()?;

        let mut ts = TreeState::new(dir.path(), true)?.0;
        assert_eq!(ts.pending_change_count(), 0);

        let mut rng = ChaChaRng::from_seed([0; 32]);
        ts.insert("foo", &rng.gen())?;
        ts.insert("bar", &rng.gen())?;
        ts.insert("baz", &rng.gen())?;
        assert_eq!(ts.pending_change_count(), 3);

        ts.remove("foo")?;
        assert_eq!(ts.pending_change_count(), 4);

        ts.visit(
            &mut |_, _| Ok(VisitorResult::Changed),
            &|_, _| true,
            &|_, _| true,
        )?;
        assert_eq!(ts.pending_change_count(), 6);

        ts.flush()?;
        assert_eq!(ts.pending_change_count(), 0);

        Ok(())
    }

    #[test]
    fn test_normalize_untracked_file() -> Result<()> {
        let dir = tempdir()?;

        let mut ts = TreeState::new(dir.path(), false)?.0;

        let mut rng = ChaChaRng::from_seed([0; 32]);
        ts.insert("a/b/c/d", &rng.gen())?;

        assert_eq!(ts.normalize_path(b"A").unwrap().as_ref(), b"A");
        assert_eq!(ts.normalize_path(b"A/a").unwrap().as_ref(), b"a/a");
        assert_eq!(ts.normalize_path(b"a/B/c/e").unwrap().as_ref(), b"a/b/c/e");
        assert_eq!(
            ts.normalize_path(b"a/B/x/Y/z").unwrap().as_ref(),
            b"a/b/x/Y/z"
        );

        Ok(())
    }
}
