/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::{
    cell::RefCell,
    collections::vec_deque::{Iter, IterMut},
    collections::VecDeque,
    fs::read_dir,
    io::ErrorKind,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

use anyhow::Result;
use parking_lot::Mutex;

use types::{Key, NodeInfo};

use crate::{
    datapack::{DataPack, DataPackVersion},
    datastore::{Delta, HgIdDataStore, HgIdMutableDeltaStore, Metadata, StoreResult},
    historypack::{HistoryPack, HistoryPackVersion},
    historystore::{HgIdHistoryStore, HgIdMutableHistoryStore},
    localstore::LocalStore,
    mutabledatapack::MutableDataPack,
    mutablehistorypack::MutableHistoryPack,
    repack::Repackable,
    types::StoreKey,
    uniondatastore::UnionHgIdDataStore,
    unionhistorystore::UnionHgIdHistoryStore,
};

/// Naive implementation of a store that order its underlying stores based on how recently we found
/// data in them. This helps in reducing the number of stores that are iterated on.
///
/// The implementation uses a `VecDeque` and always moves the last used store to the front.
///
/// Open source crates were considered, but none support both having an unbounded size, and being
/// able to move one element to the front.
struct LruStore<T> {
    stores: VecDeque<T>,
}

impl<T> LruStore<T> {
    fn new() -> Self {
        Self {
            stores: VecDeque::new(),
        }
    }

    /// Move the store at `index` at the front.
    ///
    /// From a complexity standpoint, the complexity is at worst O(N). In practice, we're expecting
    /// the most recent stores to be near the beginning, which would reduce the observed cost of
    /// this. A more efficient datastructure should allow for a lower complexity.
    fn update(&mut self, index: usize) {
        if let Some(store) = self.stores.remove(index) {
            self.stores.push_front(store);
        }
    }

    /// Add the store at the front of the `LruStore`.
    fn add(&mut self, store: T) {
        self.stores.push_front(store);
    }

    /// Remove an element from the `LruStore`. The order will not be preserved.
    fn remove(&mut self, index: usize) -> T {
        self.stores.swap_remove_back(index).unwrap()
    }

    /// Iterates over all the element, the most recently used items will be returned first.
    fn iter(&self) -> Iter<T> {
        self.stores.iter()
    }

    /// Iterates over all the element, the most recently used items will be returned first.
    fn iter_mut(&mut self) -> IterMut<T> {
        self.stores.iter_mut()
    }
}

impl<'a, T> IntoIterator for &'a LruStore<T> {
    type Item = &'a T;
    type IntoIter = Iter<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<T> From<Vec<T>> for LruStore<T> {
    fn from(other: Vec<T>) -> Self {
        Self {
            stores: other.into(),
        }
    }
}

#[derive(PartialEq)]
pub enum CorruptionPolicy {
    IGNORE,
    REMOVE,
}

struct PackStoreInner<T> {
    pack_dir: PathBuf,
    extension: &'static str,
    corruption_policy: CorruptionPolicy,
    scan_frequency: Duration,
    last_scanned: RefCell<Instant>,
    packs: RefCell<LruStore<T>>,
}

/// A `PackStore` automatically keeps track of packfiles in a given directory. New on-disk
/// packfiles will be periodically scanned and opened accordingly.
pub struct PackStore<T> {
    inner: Mutex<PackStoreInner<T>>,
}

pub type DataPackStore = PackStore<DataPack>;
pub type HistoryPackStore = PackStore<HistoryPack>;

struct PackStoreOptions {
    pack_dir: PathBuf,
    scan_frequency: Duration,
    extension: &'static str,
    corruption_policy: CorruptionPolicy,
}

impl PackStoreOptions {
    fn new() -> Self {
        Self {
            pack_dir: PathBuf::new(),
            scan_frequency: Duration::from_secs(10),
            extension: "",
            corruption_policy: CorruptionPolicy::IGNORE,
        }
    }

    fn directory<P: AsRef<Path>>(mut self, dir: P) -> Self {
        self.pack_dir = PathBuf::from(dir.as_ref());
        self
    }

    #[cfg(test)]
    fn scan_frequency(mut self, frequency: Duration) -> Self {
        self.scan_frequency = frequency;
        self
    }

    fn extension(mut self, extension: &'static str) -> Self {
        self.extension = extension;
        self
    }

    /// When a packfile is detected to be corrupted, should we automatically remove it from disk or
    /// simply ignore it?
    fn corruption_policy(mut self, corruption_policy: CorruptionPolicy) -> Self {
        self.corruption_policy = corruption_policy;
        self
    }

    fn build<T>(self) -> PackStore<T> {
        let now = Instant::now();
        let force_rescan = now - self.scan_frequency;

        PackStore {
            inner: Mutex::new(PackStoreInner {
                pack_dir: self.pack_dir,
                scan_frequency: self.scan_frequency,
                extension: self.extension,
                corruption_policy: self.corruption_policy,
                last_scanned: RefCell::new(force_rescan),
                packs: RefCell::new(LruStore::new()),
            }),
        }
    }
}

impl<T> PackStore<T> {
    /// Force a rescan to be performed. Since rescan are expensive, this should only be called for
    /// out-of-process created packfiles.
    pub fn force_rescan(&self) {
        let packstore = self.inner.lock();
        packstore
            .last_scanned
            .replace(Instant::now() - packstore.scan_frequency);
    }

    /// Add a packfile to this store.
    fn add_pack(&self, pack: T) {
        self.inner.lock().packs.borrow_mut().add(pack);
    }
}

impl DataPackStore {
    /// Build a new DataPackStore. The default rescan rate is 10 seconds.
    ///
    /// Only use for data that can be recoverd from the network, corrupted datapacks will be
    /// automatically removed from disk.
    pub fn new<P: AsRef<Path>>(pack_dir: P, corruption_policy: CorruptionPolicy) -> Self {
        PackStoreOptions::new()
            .directory(pack_dir)
            .corruption_policy(corruption_policy)
            .extension("datapack")
            .build()
    }
}

impl HistoryPackStore {
    /// Build a new HistoryPackStore. The default rescan rate is 10 seconds.
    ///
    /// Only use for data that can be recoverd from the network, corrupted datapacks will be
    /// automatically removed from disk.
    pub fn new<P: AsRef<Path>>(pack_dir: P, corruption_policy: CorruptionPolicy) -> Self {
        PackStoreOptions::new()
            .directory(pack_dir)
            .corruption_policy(corruption_policy)
            .extension("histpack")
            .build()
    }
}

impl<T: LocalStore + Repackable> PackStoreInner<T> {
    /// Open new on-disk packfiles, and close removed ones.
    fn rescan(&self) -> Result<()> {
        let mut new_packs = Vec::new();

        let readdir = match read_dir(&self.pack_dir) {
            Ok(readdir) => readdir,
            Err(e) => {
                if e.kind() == ErrorKind::NotFound {
                    return Ok(());
                } else {
                    return Err(e.into());
                }
            }
        };

        for entry in readdir {
            let entry = entry?;
            if entry.file_type()?.is_file() {
                let path = entry.path();

                if let Some(ext) = path.extension() {
                    if ext == self.extension {
                        if let Ok(pack) = T::from_path(&path) {
                            new_packs.push(pack);
                        }
                    }
                }
            }
        }

        self.packs.replace(new_packs.into());
        Ok(())
    }

    /// Scan the store when too much time has passed since the last scan. Returns whether the
    /// filesystem was actually scanned.
    fn try_scan(&self) -> Result<bool> {
        let now = Instant::now();

        if now.duration_since(*self.last_scanned.borrow()) >= self.scan_frequency {
            self.rescan()?;
            self.last_scanned.replace(now);
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Execute the `op` function. May call `rescan` when `op` fails with `KeyError`.
    fn run<R, F>(&self, op: F) -> Result<Option<R>>
    where
        F: Fn(&T) -> Result<Option<R>>,
    {
        for _ in 0..2 {
            let mut found = None;
            {
                let mut corrupted = Vec::new();

                let mut lrustore = self.packs.try_borrow_mut()?;
                for (index, store) in lrustore.iter_mut().enumerate() {
                    match op(store) {
                        Ok(None) => continue,
                        Ok(Some(result)) => {
                            found = Some((index, result));
                            break;
                        }
                        Err(_) => {
                            corrupted.push(index);
                        }
                    }
                }

                if !corrupted.is_empty() {
                    for store_index in corrupted.into_iter().rev() {
                        let store = lrustore.remove(store_index);
                        if self.corruption_policy == CorruptionPolicy::REMOVE {
                            let _ = store.delete();
                        }
                    }
                }
            }

            if let Some((index, result)) = found {
                self.packs.borrow_mut().update(index);
                return Ok(Some(result));
            }

            // We didn't find anything, let's try to probe the filesystem to discover new packfiles
            // and retry.
            if !self.try_scan()? {
                break;
            }
        }

        Ok(None)
    }
}

impl<T: LocalStore + Repackable> LocalStore for PackStore<T> {
    fn get_missing(&self, keys: &[StoreKey]) -> Result<Vec<StoreKey>> {
        // Since the packfiles are loaded lazily, it's possible that `get_missing` is called before
        // any packfiles have been loaded. Let's tentatively scan the store before iterating over
        // all the known packs.
        let packstore = self.inner.lock();
        packstore.try_scan()?;

        let initial_keys = Ok(keys.to_vec());
        let packs = packstore.packs.try_borrow()?;
        packs
            .into_iter()
            .fold(initial_keys, |missing_keys, store| match missing_keys {
                Ok(missing_keys) => store.get_missing(&missing_keys),
                Err(e) => Err(e),
            })
    }
}

impl HgIdDataStore for DataPackStore {
    fn get(&self, key: StoreKey) -> Result<StoreResult<Vec<u8>>> {
        let res = self
            .inner
            .lock()
            .run(|store| match store.get(key.clone())? {
                StoreResult::Found(content) => Ok(Some(content)),
                StoreResult::NotFound(_) => Ok(None),
            })?;

        match res {
            None => Ok(StoreResult::NotFound(key)),
            Some(content) => Ok(StoreResult::Found(content)),
        }
    }

    fn get_meta(&self, key: StoreKey) -> Result<StoreResult<Metadata>> {
        let res = self
            .inner
            .lock()
            .run(|store| match store.get_meta(key.clone())? {
                StoreResult::Found(meta) => Ok(Some(meta)),
                StoreResult::NotFound(_) => Ok(None),
            })?;

        match res {
            None => Ok(StoreResult::NotFound(key)),
            Some(meta) => Ok(StoreResult::Found(meta)),
        }
    }
}

impl HgIdHistoryStore for HistoryPackStore {
    fn get_node_info(&self, key: &Key) -> Result<Option<NodeInfo>> {
        self.inner.lock().run(|store| store.get_node_info(key))
    }
}

struct MutableDataPackStoreInner {
    pack_store: Arc<DataPackStore>,
    mutable_pack: Arc<MutableDataPack>,
    union_store: UnionHgIdDataStore<Arc<dyn HgIdDataStore>>,
}

/// A `MutableDataPackStore` allows both reading and writing to data packfiles.
pub struct MutableDataPackStore {
    inner: MutableDataPackStoreInner,
}

impl MutableDataPackStore {
    pub fn new(pack_dir: impl AsRef<Path>, corruption_policy: CorruptionPolicy) -> Result<Self> {
        let pack_store = Arc::new(DataPackStore::new(pack_dir.as_ref(), corruption_policy));
        let mutable_pack = Arc::new(MutableDataPack::new(pack_dir, DataPackVersion::One)?);
        let mut union_store: UnionHgIdDataStore<Arc<dyn HgIdDataStore>> = UnionHgIdDataStore::new();
        union_store.add(pack_store.clone());
        union_store.add(mutable_pack.clone());

        Ok(Self {
            inner: MutableDataPackStoreInner {
                pack_store,
                mutable_pack,
                union_store,
            },
        })
    }
}

impl HgIdDataStore for MutableDataPackStore {
    fn get(&self, key: StoreKey) -> Result<StoreResult<Vec<u8>>> {
        self.inner.union_store.get(key)
    }

    fn get_meta(&self, key: StoreKey) -> Result<StoreResult<Metadata>> {
        self.inner.union_store.get_meta(key)
    }
}

impl LocalStore for MutableDataPackStore {
    fn get_missing(&self, keys: &[StoreKey]) -> Result<Vec<StoreKey>> {
        self.inner.union_store.get_missing(keys)
    }
}

impl HgIdMutableDeltaStore for MutableDataPackStore {
    fn add(&self, delta: &Delta, metadata: &Metadata) -> Result<()> {
        self.inner.mutable_pack.add(delta, metadata)
    }

    /// Flush the current mutable datapack to disk and add it to the `PackStore`.
    fn flush(&self) -> Result<Option<PathBuf>> {
        if let Some(path) = self.inner.mutable_pack.flush()? {
            let datapack = DataPack::new(path.as_path())?;
            self.inner.pack_store.add_pack(datapack);
            Ok(Some(path))
        } else {
            Ok(None)
        }
    }
}

struct MutableHistoryPackStoreInner {
    pack_store: Arc<HistoryPackStore>,
    mutable_pack: Arc<MutableHistoryPack>,
    union_store: UnionHgIdHistoryStore<Arc<dyn HgIdHistoryStore>>,
}

/// A `MutableHistoryPackStore` allows both reading and writing to history packfiles.
pub struct MutableHistoryPackStore {
    inner: MutableHistoryPackStoreInner,
    pending: AtomicU64,
    result_packs: Arc<Mutex<Vec<PathBuf>>>,
    max_pending: u64,
}

impl MutableHistoryPackStore {
    pub fn new(
        pack_dir: impl AsRef<Path>,
        corruption_policy: CorruptionPolicy,
        max_pending: u64,
    ) -> Result<Self> {
        let pack_store = Arc::new(HistoryPackStore::new(pack_dir.as_ref(), corruption_policy));
        let mutable_pack = Arc::new(MutableHistoryPack::new(pack_dir, HistoryPackVersion::One)?);
        let mut union_store: UnionHgIdHistoryStore<Arc<dyn HgIdHistoryStore>> =
            UnionHgIdHistoryStore::new();
        union_store.add(pack_store.clone());
        union_store.add(mutable_pack.clone());

        Ok(Self {
            inner: MutableHistoryPackStoreInner {
                pack_store,
                mutable_pack,
                union_store,
            },
            pending: AtomicU64::new(0),
            result_packs: Arc::new(Mutex::new(Vec::new())),
            max_pending,
        })
    }

    fn inner_flush(&self) -> Result<()> {
        self.pending.store(0, Ordering::SeqCst);
        if let Some(paths) = self.inner.mutable_pack.flush()? {
            let mut result_packs = self.result_packs.lock();
            for path in paths {
                let histpack = HistoryPack::new(path.as_path())?;
                self.inner.pack_store.add_pack(histpack);
                result_packs.push(path);
            }
        }
        Ok(())
    }
}

impl HgIdHistoryStore for MutableHistoryPackStore {
    fn get_node_info(&self, key: &Key) -> Result<Option<NodeInfo>> {
        self.inner.union_store.get_node_info(key)
    }
}

impl LocalStore for MutableHistoryPackStore {
    fn get_missing(&self, keys: &[StoreKey]) -> Result<Vec<StoreKey>> {
        self.inner.union_store.get_missing(keys)
    }
}

impl HgIdMutableHistoryStore for MutableHistoryPackStore {
    fn add(&self, key: &Key, info: &NodeInfo) -> Result<()> {
        self.inner.mutable_pack.add(key, info)?;
        let pending = self.pending.fetch_add(1, Ordering::SeqCst) + 1;
        if pending >= self.max_pending {
            self.inner_flush()?;
        }
        Ok(())
    }

    /// Flush the current mutable historypack to disk and add it to the `PackStore`.
    fn flush(&self) -> Result<Option<Vec<PathBuf>>> {
        self.inner_flush()?;
        let mut packs = self.result_packs.lock();
        let result = std::mem::take(&mut *packs);

        Ok(if result.len() > 0 { Some(result) } else { None })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::fs::{self, OpenOptions};

    use bytes::Bytes;
    use rand::SeedableRng;
    use rand_chacha::ChaChaRng;
    use tempfile::TempDir;

    use types::testutil::*;

    use crate::datapack::tests::make_datapack;
    use crate::historypack::tests::{get_nodes, make_historypack};

    #[test]
    fn test_datapack_created_before() -> Result<()> {
        let tempdir = TempDir::new()?;

        let k = key("a", "2");
        let revision = (
            Delta {
                data: Bytes::from(&[1, 2, 3, 4][..]),
                base: None,
                key: k.clone(),
            },
            Default::default(),
        );
        make_datapack(&tempdir, &vec![revision.clone()]);

        let store = DataPackStore::new(&tempdir, CorruptionPolicy::REMOVE);
        let stored = store.get(StoreKey::hgid(k))?;
        assert_eq!(
            stored,
            StoreResult::Found(revision.0.data.as_ref().to_vec())
        );
        Ok(())
    }

    #[test]
    fn test_datapack_get_missing() -> Result<()> {
        let tempdir = TempDir::new()?;

        let k = key("a", "2");
        let revision = (
            Delta {
                data: Bytes::from(&[1, 2, 3, 4][..]),
                base: Some(key("a", "1")),
                key: k.clone(),
            },
            Default::default(),
        );
        make_datapack(&tempdir, &vec![revision.clone()]);

        let store = DataPackStore::new(&tempdir, CorruptionPolicy::REMOVE);
        let missing = store.get_missing(&vec![StoreKey::from(k)])?;
        assert_eq!(missing.len(), 0);
        Ok(())
    }

    #[test]
    fn test_datapack_created_after() -> Result<()> {
        let tempdir = TempDir::new()?;
        let store = DataPackStore::new(&tempdir, CorruptionPolicy::REMOVE);

        let k = key("a", "2");
        let revision = (
            Delta {
                data: Bytes::from(&[1, 2, 3, 4][..]),
                base: None,
                key: k.clone(),
            },
            Default::default(),
        );
        make_datapack(&tempdir, &vec![revision.clone()]);

        let stored = store.get(StoreKey::hgid(k))?;
        assert_eq!(
            stored,
            StoreResult::Found(revision.0.data.as_ref().to_vec())
        );
        Ok(())
    }

    #[test]
    fn test_slow_rescan() {
        let tempdir = TempDir::new().unwrap();
        let store = PackStoreOptions::new()
            .directory(&tempdir)
            .extension("datapack")
            .scan_frequency(Duration::from_secs(1000))
            .build();

        let k = key("a", "2");
        let revision = (
            Delta {
                data: Bytes::from(&[1, 2, 3, 4][..]),
                base: None,
                key: k.clone(),
            },
            Default::default(),
        );
        make_datapack(&tempdir, &vec![revision.clone()]);

        store.get(StoreKey::hgid(k)).unwrap();

        let k = key("a", "3");
        let revision = (
            Delta {
                data: Bytes::from(&[1, 2, 3, 4][..]),
                base: None,
                key: k.clone(),
            },
            Default::default(),
        );
        make_datapack(&tempdir, &vec![revision.clone()]);

        let k = StoreKey::hgid(k);
        assert_eq!(store.get(k.clone()).unwrap(), StoreResult::NotFound(k));
    }

    #[test]
    fn test_force_rescan() -> Result<()> {
        let tempdir = TempDir::new()?;
        let store = PackStoreOptions::new()
            .directory(&tempdir)
            .extension("datapack")
            .scan_frequency(Duration::from_secs(1000))
            .build();

        let k = key("a", "2");
        let revision = (
            Delta {
                data: Bytes::from(&[1, 2, 3, 4][..]),
                base: None,
                key: k.clone(),
            },
            Default::default(),
        );
        make_datapack(&tempdir, &vec![revision.clone()]);

        store.get(StoreKey::hgid(k))?;

        let k = key("a", "3");
        let revision = (
            Delta {
                data: Bytes::from(&[1, 2, 3, 4][..]),
                base: None,
                key: k.clone(),
            },
            Default::default(),
        );
        make_datapack(&tempdir, &vec![revision.clone()]);

        store.force_rescan();
        assert_eq!(
            store.get(StoreKey::hgid(k))?,
            StoreResult::Found(vec![1, 2, 3, 4])
        );
        Ok(())
    }

    #[test]
    fn test_histpack() -> Result<()> {
        let mut rng = ChaChaRng::from_seed([0u8; 32]);
        let tempdir = TempDir::new()?;
        let store = HistoryPackStore::new(&tempdir, CorruptionPolicy::REMOVE);

        let nodes = get_nodes(&mut rng);
        make_historypack(&tempdir, &nodes);
        for (key, info) in nodes.iter() {
            let response: NodeInfo = store.get_node_info(key)?.unwrap();
            assert_eq!(response, *info);
        }

        Ok(())
    }

    #[test]
    fn test_lrustore_order() -> Result<()> {
        let tempdir = TempDir::new()?;

        let k1 = key("a", "2");
        let revision1 = (
            Delta {
                data: Bytes::from(&[1, 2, 3, 4][..]),
                base: None,
                key: k1.clone(),
            },
            Default::default(),
        );
        make_datapack(&tempdir, &vec![revision1.clone()]);

        let k2 = key("b", "3");
        let revision2 = (
            Delta {
                data: Bytes::from(&[1, 2, 3, 4][..]),
                base: None,
                key: k2.clone(),
            },
            Default::default(),
        );
        make_datapack(&tempdir, &vec![revision2.clone()]);

        let packstore = DataPackStore::new(&tempdir, CorruptionPolicy::REMOVE);

        let k2 = StoreKey::hgid(k2);
        let _ = packstore.get(k2.clone())?;
        assert!(packstore.inner.lock().packs.borrow().stores[0]
            .get(k2)
            .is_ok());

        let k1 = StoreKey::hgid(k1);
        let _ = packstore.get(k1.clone())?;
        assert!(packstore.inner.lock().packs.borrow().stores[0]
            .get(k1)
            .is_ok());

        Ok(())
    }

    #[test]
    fn test_rescan_no_dir() -> Result<()> {
        let tempdir = TempDir::new()?;
        let mut non_present_tempdir = tempdir.into_path();
        non_present_tempdir.push("non_present");
        let store = HistoryPackStore::new(&non_present_tempdir, CorruptionPolicy::REMOVE);

        let store = store.inner.lock();
        store.rescan()
    }

    #[test]
    fn test_corrupted() {
        let tempdir = TempDir::new().unwrap();

        let k1 = key("a", "2");
        let revision1 = (
            Delta {
                data: Bytes::from(&[1, 2, 3, 4][..]),
                base: None,
                key: k1.clone(),
            },
            Default::default(),
        );
        let path = make_datapack(&tempdir, &vec![revision1.clone()])
            .pack_path()
            .to_path_buf();

        let metadata = fs::metadata(&path).unwrap();
        let mut permissions = metadata.permissions();
        permissions.set_readonly(false);
        fs::set_permissions(&path, permissions).unwrap();

        let datapack = OpenOptions::new().write(true).open(path).unwrap();
        datapack
            .set_len(datapack.metadata().unwrap().len() / 2)
            .unwrap();

        let packstore = DataPackStore::new(&tempdir, CorruptionPolicy::REMOVE);
        let k1 = StoreKey::hgid(k1);
        assert_eq!(
            packstore.get(k1.clone()).unwrap(),
            StoreResult::NotFound(k1)
        );
    }

    #[test]
    fn test_ignore_corrupted() -> Result<()> {
        let tempdir = TempDir::new()?;

        let k1 = key("a", "2");
        let revision1 = (
            Delta {
                data: Bytes::from(&[1, 2, 3, 4][..]),
                base: None,
                key: k1.clone(),
            },
            Default::default(),
        );
        let path = make_datapack(&tempdir, &vec![revision1.clone()])
            .pack_path()
            .to_path_buf();

        let metadata = fs::metadata(&path).unwrap();
        let mut permissions = metadata.permissions();
        permissions.set_readonly(false);
        fs::set_permissions(&path, permissions).unwrap();

        let datapack = OpenOptions::new().write(true).open(path)?;
        datapack.set_len(datapack.metadata()?.len() / 2)?;

        assert_eq!(read_dir(&tempdir)?.count(), 2);

        let packstore = DataPackStore::new(&tempdir, CorruptionPolicy::IGNORE);
        let k1 = StoreKey::hgid(k1);
        assert_eq!(packstore.get(k1.clone())?, StoreResult::NotFound(k1));

        assert_eq!(read_dir(&tempdir)?.count(), 2);
        Ok(())
    }

    #[test]
    fn test_add_flush() -> Result<()> {
        let tempdir = TempDir::new()?;
        let packstore = MutableDataPackStore::new(&tempdir, CorruptionPolicy::REMOVE)?;

        let k1 = key("a", "2");
        let delta = Delta {
            data: Bytes::from(&[1, 2, 3, 4][..]),
            base: Some(key("a", "1")),
            key: k1.clone(),
        };

        packstore.add(&delta, &Default::default())?;
        packstore.flush()?;
        Ok(())
    }

    #[test]
    fn test_add_get_delta() -> Result<()> {
        let tempdir = TempDir::new()?;
        let packstore = MutableDataPackStore::new(&tempdir, CorruptionPolicy::REMOVE)?;

        let k1 = key("a", "2");
        let delta = Delta {
            data: Bytes::from(&[1, 2, 3, 4][..]),
            base: None,
            key: k1.clone(),
        };

        packstore.add(&delta, &Default::default())?;
        let stored = packstore.get(StoreKey::hgid(k1))?;
        assert_eq!(stored, StoreResult::Found(delta.data.as_ref().to_vec()));
        Ok(())
    }

    #[test]
    fn test_add_flush_get_delta() -> Result<()> {
        let tempdir = TempDir::new()?;
        let packstore = MutableDataPackStore::new(&tempdir, CorruptionPolicy::REMOVE)?;

        let k1 = key("a", "2");
        let delta = Delta {
            data: Bytes::from(&[1, 2, 3, 4][..]),
            base: None,
            key: k1.clone(),
        };

        packstore.add(&delta, &Default::default())?;
        packstore.flush()?;
        let stored = packstore.get(StoreKey::hgid(k1))?;
        assert_eq!(stored, StoreResult::Found(delta.data.as_ref().to_vec()));
        Ok(())
    }

    #[test]
    fn test_histpack_add_get() -> Result<()> {
        let tempdir = TempDir::new()?;
        let packstore = MutableHistoryPackStore::new(&tempdir, CorruptionPolicy::REMOVE, 1000)?;

        let mut rng = ChaChaRng::from_seed([0u8; 32]);
        let nodes = get_nodes(&mut rng);
        for (key, info) in &nodes {
            packstore.add(key, info)?;
        }

        for (key, info) in nodes {
            let nodeinfo = packstore.get_node_info(&key)?.unwrap();
            assert_eq!(nodeinfo, info);
        }
        Ok(())
    }

    #[test]
    fn test_histpack_add_flush_get() -> Result<()> {
        let tempdir = TempDir::new()?;
        let packstore = MutableHistoryPackStore::new(&tempdir, CorruptionPolicy::REMOVE, 1000)?;

        let mut rng = ChaChaRng::from_seed([0u8; 32]);
        let nodes = get_nodes(&mut rng);
        for (key, info) in &nodes {
            packstore.add(key, info)?;
        }

        packstore.flush()?;

        for (key, info) in nodes {
            let nodeinfo = packstore.get_node_info(&key)?.unwrap();
            assert_eq!(nodeinfo, info);
        }
        Ok(())
    }

    #[test]
    fn test_histpack_auto_flush() -> Result<()> {
        let tempdir = TempDir::new()?;
        let packstore = MutableHistoryPackStore::new(&tempdir, CorruptionPolicy::REMOVE, 0)?;

        let mut rng = ChaChaRng::from_seed([0u8; 32]);
        let nodes = get_nodes(&mut rng);
        for (key, info) in &nodes {
            packstore.add(key, info)?;
        }

        let packs = packstore.flush().unwrap().unwrap();
        assert_eq!(packs.len(), 3);

        for (key, info) in nodes {
            let nodeinfo = packstore.get_node_info(&key)?.unwrap();
            assert_eq!(nodeinfo, info);
        }
        Ok(())
    }

    #[test]
    fn test_datapack_flush_empty() -> Result<()> {
        let tempdir = TempDir::new()?;
        let packstore = MutableDataPackStore::new(&tempdir, CorruptionPolicy::REMOVE)?;
        packstore.flush()?;
        Ok(())
    }

    #[test]
    fn test_histpack_flush_empty() -> Result<()> {
        let tempdir = TempDir::new()?;
        let packstore = MutableHistoryPackStore::new(&tempdir, CorruptionPolicy::REMOVE, 1000)?;
        packstore.flush()?;
        Ok(())
    }
}
