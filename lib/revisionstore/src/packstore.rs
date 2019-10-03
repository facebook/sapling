// Copyright Facebook, Inc. 2019

use std::{
    cell::RefCell,
    collections::vec_deque::{Iter, IterMut},
    collections::VecDeque,
    fs::read_dir,
    io::ErrorKind,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

use failure::Fallible;

use types::{Key, NodeInfo};

use crate::datapack::{DataPack, DataPackVersion};
use crate::datastore::{DataStore, Delta, Metadata, MutableDeltaStore};
use crate::historypack::{HistoryPack, HistoryPackVersion};
use crate::historystore::{Ancestors, HistoryStore, MutableHistoryStore};
use crate::localstore::LocalStore;
use crate::mutabledatapack::MutableDataPack;
use crate::mutablehistorypack::MutableHistoryPack;
use crate::repack::Repackable;
use crate::uniondatastore::UnionDataStore;
use crate::unionhistorystore::UnionHistoryStore;

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

/// A `PackStore` automatically keeps track of packfiles in a given directory. New on-disk
/// packfiles will be periodically scanned and opened accordingly.
pub struct PackStore<T> {
    pack_dir: PathBuf,
    extension: &'static str,
    corruption_policy: CorruptionPolicy,
    scan_frequency: Duration,
    last_scanned: RefCell<Instant>,
    packs: RefCell<LruStore<T>>,
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
            pack_dir: self.pack_dir,
            scan_frequency: self.scan_frequency,
            extension: self.extension,
            corruption_policy: self.corruption_policy,
            last_scanned: RefCell::new(force_rescan),
            packs: RefCell::new(LruStore::new()),
        }
    }
}

impl<T> PackStore<T> {
    /// Force a rescan to be performed. Since rescan are expensive, this should only be called for
    /// out-of-process created packfiles.
    pub fn force_rescan(&self) {
        self.last_scanned
            .replace(Instant::now() - self.scan_frequency);
    }

    /// Add a packfile to this store.
    fn add_pack(&self, pack: T) {
        self.packs.borrow_mut().add(pack);
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

impl<T: LocalStore + Repackable> PackStore<T> {
    /// Open new on-disk packfiles, and close removed ones.
    fn rescan(&self) -> Fallible<()> {
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
    fn try_scan(&self) -> Fallible<bool> {
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
    fn run<R, F>(&self, op: F) -> Fallible<Option<R>>
    where
        F: Fn(&T) -> Fallible<Option<R>>,
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
    fn get_missing(&self, keys: &[Key]) -> Fallible<Vec<Key>> {
        // Since the packfiles are loaded lazily, it's possible that `get_missing` is called before
        // any packfiles have been loaded. Let's tentatively scan the store before iterating over
        // all the known packs.
        self.try_scan()?;

        let initial_keys = Ok(keys.iter().cloned().collect());
        self.packs
            .try_borrow()?
            .into_iter()
            .fold(initial_keys, |missing_keys, store| match missing_keys {
                Ok(missing_keys) => store.get_missing(&missing_keys),
                Err(e) => Err(e),
            })
    }
}

impl DataStore for DataPackStore {
    fn get(&self, key: &Key) -> Fallible<Option<Vec<u8>>> {
        self.run(|store| store.get(key))
    }

    fn get_delta(&self, key: &Key) -> Fallible<Option<Delta>> {
        self.run(|store| store.get_delta(key))
    }

    fn get_delta_chain(&self, key: &Key) -> Fallible<Option<Vec<Delta>>> {
        self.run(|store| store.get_delta_chain(key))
    }

    fn get_meta(&self, key: &Key) -> Fallible<Option<Metadata>> {
        self.run(|store| store.get_meta(key))
    }
}

impl HistoryStore for HistoryPackStore {
    fn get_ancestors(&self, key: &Key) -> Fallible<Option<Ancestors>> {
        self.run(|store| store.get_ancestors(key))
    }

    fn get_node_info(&self, key: &Key) -> Fallible<Option<NodeInfo>> {
        self.run(|store| store.get_node_info(key))
    }
}

struct MutableDataPackStoreInner {
    pack_store: Arc<DataPackStore>,
    mutable_pack: MutableDataPack,
    union_store: UnionDataStore<Box<dyn DataStore>>,
}

/// A `MutableDataPackStore` allows both reading and writing to data packfiles.
#[derive(Clone)]
pub struct MutableDataPackStore {
    inner: Arc<MutableDataPackStoreInner>,
}

impl MutableDataPackStore {
    pub fn new(pack_dir: impl AsRef<Path>, corruption_policy: CorruptionPolicy) -> Fallible<Self> {
        let pack_store = Arc::new(DataPackStore::new(pack_dir.as_ref(), corruption_policy));
        let mutable_pack = MutableDataPack::new(pack_dir, DataPackVersion::One)?;
        let mut union_store: UnionDataStore<Box<dyn DataStore>> = UnionDataStore::new();
        union_store.add(Box::new(pack_store.clone()));
        union_store.add(Box::new(mutable_pack.clone()));

        Ok(Self {
            inner: Arc::new(MutableDataPackStoreInner {
                pack_store,
                mutable_pack,
                union_store,
            }),
        })
    }
}

impl DataStore for MutableDataPackStore {
    fn get(&self, key: &Key) -> Fallible<Option<Vec<u8>>> {
        self.inner.union_store.get(key)
    }

    fn get_delta(&self, key: &Key) -> Fallible<Option<Delta>> {
        self.inner.union_store.get_delta(key)
    }

    fn get_delta_chain(&self, key: &Key) -> Fallible<Option<Vec<Delta>>> {
        self.inner.union_store.get_delta_chain(key)
    }

    fn get_meta(&self, key: &Key) -> Fallible<Option<Metadata>> {
        self.inner.union_store.get_meta(key)
    }
}

impl LocalStore for MutableDataPackStore {
    fn get_missing(&self, keys: &[Key]) -> Fallible<Vec<Key>> {
        self.inner.union_store.get_missing(keys)
    }
}

impl MutableDeltaStore for MutableDataPackStore {
    fn add(&self, delta: &Delta, metadata: &Metadata) -> Fallible<()> {
        self.inner.mutable_pack.add(delta, metadata)
    }

    /// Flush the current mutable datapack to disk and add it to the `PackStore`.
    fn flush(&self) -> Fallible<Option<PathBuf>> {
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
    mutable_pack: MutableHistoryPack,
    union_store: UnionHistoryStore<Box<dyn HistoryStore>>,
}

/// A `MutableHistoryPackStore` allows both reading and writing to history packfiles.
pub struct MutableHistoryPackStore {
    inner: Arc<MutableHistoryPackStoreInner>,
}

impl MutableHistoryPackStore {
    pub fn new(pack_dir: impl AsRef<Path>, corruption_policy: CorruptionPolicy) -> Fallible<Self> {
        let pack_store = Arc::new(HistoryPackStore::new(pack_dir.as_ref(), corruption_policy));
        let mutable_pack = MutableHistoryPack::new(pack_dir, HistoryPackVersion::One)?;
        let mut union_store: UnionHistoryStore<Box<dyn HistoryStore>> = UnionHistoryStore::new();
        union_store.add(Box::new(pack_store.clone()));
        union_store.add(Box::new(mutable_pack.clone()));

        Ok(Self {
            inner: Arc::new(MutableHistoryPackStoreInner {
                pack_store,
                mutable_pack,
                union_store,
            }),
        })
    }
}

impl HistoryStore for MutableHistoryPackStore {
    fn get_ancestors(&self, key: &Key) -> Fallible<Option<Ancestors>> {
        self.inner.union_store.get_ancestors(key)
    }

    fn get_node_info(&self, key: &Key) -> Fallible<Option<NodeInfo>> {
        self.inner.union_store.get_node_info(key)
    }
}

impl LocalStore for MutableHistoryPackStore {
    fn get_missing(&self, keys: &[Key]) -> Fallible<Vec<Key>> {
        self.inner.union_store.get_missing(keys)
    }
}

impl MutableHistoryStore for MutableHistoryPackStore {
    fn add(&self, key: &Key, info: &NodeInfo) -> Fallible<()> {
        self.inner.mutable_pack.add(key, info)
    }

    /// Flush the current mutable historypack to disk and add it to the `PackStore`.
    fn flush(&self) -> Fallible<Option<PathBuf>> {
        if let Some(path) = self.inner.mutable_pack.flush()? {
            let histpack = HistoryPack::new(path.as_path())?;
            self.inner.pack_store.add_pack(histpack);
            Ok(Some(path))
        } else {
            Ok(None)
        }
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
    fn test_datapack_created_before() -> Fallible<()> {
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
        let delta = store.get_delta(&k)?.unwrap();
        assert_eq!(delta, revision.0);
        Ok(())
    }

    #[test]
    fn test_datapack_get_missing() -> Fallible<()> {
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
        let missing = store.get_missing(&vec![k])?;
        assert_eq!(missing.len(), 0);
        Ok(())
    }

    #[test]
    fn test_datapack_created_after() -> Fallible<()> {
        let tempdir = TempDir::new()?;
        let store = DataPackStore::new(&tempdir, CorruptionPolicy::REMOVE);

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

        let delta = store.get_delta(&k)?.unwrap();
        assert_eq!(delta, revision.0);
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
                base: Some(key("a", "1")),
                key: k.clone(),
            },
            Default::default(),
        );
        make_datapack(&tempdir, &vec![revision.clone()]);

        store.get_delta(&k).unwrap();

        let k = key("a", "3");
        let revision = (
            Delta {
                data: Bytes::from(&[1, 2, 3, 4][..]),
                base: Some(key("a", "1")),
                key: k.clone(),
            },
            Default::default(),
        );
        make_datapack(&tempdir, &vec![revision.clone()]);

        assert_eq!(store.get_delta(&k).unwrap(), None);
    }

    #[test]
    fn test_force_rescan() -> Fallible<()> {
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
                base: Some(key("a", "1")),
                key: k.clone(),
            },
            Default::default(),
        );
        make_datapack(&tempdir, &vec![revision.clone()]);

        store.get_delta(&k)?;

        let k = key("a", "3");
        let revision = (
            Delta {
                data: Bytes::from(&[1, 2, 3, 4][..]),
                base: Some(key("a", "1")),
                key: k.clone(),
            },
            Default::default(),
        );
        make_datapack(&tempdir, &vec![revision.clone()]);

        store.force_rescan();
        assert!(store.get_delta(&k)?.is_some());
        Ok(())
    }

    #[test]
    fn test_histpack() -> Fallible<()> {
        let mut rng = ChaChaRng::from_seed([0u8; 32]);
        let tempdir = TempDir::new()?;
        let store = HistoryPackStore::new(&tempdir, CorruptionPolicy::REMOVE);

        let (nodes, _) = get_nodes(&mut rng);
        make_historypack(&tempdir, &nodes);
        for (key, info) in nodes.iter() {
            let response: NodeInfo = store.get_node_info(key)?.unwrap();
            assert_eq!(response, *info);
        }

        Ok(())
    }

    #[test]
    fn test_lrustore_order() -> Fallible<()> {
        let tempdir = TempDir::new()?;

        let k1 = key("a", "2");
        let revision1 = (
            Delta {
                data: Bytes::from(&[1, 2, 3, 4][..]),
                base: Some(key("a", "1")),
                key: k1.clone(),
            },
            Default::default(),
        );
        make_datapack(&tempdir, &vec![revision1.clone()]);

        let k2 = key("b", "3");
        let revision2 = (
            Delta {
                data: Bytes::from(&[1, 2, 3, 4][..]),
                base: Some(key("a", "1")),
                key: k2.clone(),
            },
            Default::default(),
        );
        make_datapack(&tempdir, &vec![revision2.clone()]);

        let packstore = DataPackStore::new(&tempdir, CorruptionPolicy::REMOVE);

        let _ = packstore.get_delta(&k2)?;
        assert!(packstore.packs.borrow().stores[0].get_delta(&k2).is_ok());

        let _ = packstore.get_delta(&k1)?;
        assert!(packstore.packs.borrow().stores[0].get_delta(&k1).is_ok());

        Ok(())
    }

    #[test]
    fn test_rescan_no_dir() -> Fallible<()> {
        let tempdir = TempDir::new()?;
        let mut non_present_tempdir = tempdir.into_path();
        non_present_tempdir.push("non_present");
        let store = HistoryPackStore::new(&non_present_tempdir, CorruptionPolicy::REMOVE);

        store.rescan()
    }

    #[test]
    fn test_corrupted() {
        let tempdir = TempDir::new().unwrap();

        let k1 = key("a", "2");
        let revision1 = (
            Delta {
                data: Bytes::from(&[1, 2, 3, 4][..]),
                base: Some(key("a", "1")),
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
        assert_eq!(packstore.get_delta(&k1).unwrap(), None);
    }

    #[test]
    fn test_ignore_corrupted() -> Fallible<()> {
        let tempdir = TempDir::new()?;

        let k1 = key("a", "2");
        let revision1 = (
            Delta {
                data: Bytes::from(&[1, 2, 3, 4][..]),
                base: Some(key("a", "1")),
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
        assert!(packstore.get_delta(&k1)?.is_none());

        assert_eq!(read_dir(&tempdir)?.count(), 2);
        Ok(())
    }

    #[test]
    fn test_add_flush() -> Fallible<()> {
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
    fn test_add_get_delta() -> Fallible<()> {
        let tempdir = TempDir::new()?;
        let packstore = MutableDataPackStore::new(&tempdir, CorruptionPolicy::REMOVE)?;

        let k1 = key("a", "2");
        let delta = Delta {
            data: Bytes::from(&[1, 2, 3, 4][..]),
            base: Some(key("a", "1")),
            key: k1.clone(),
        };

        packstore.add(&delta, &Default::default())?;
        assert_eq!(packstore.get_delta(&k1)?.unwrap(), delta);
        Ok(())
    }

    #[test]
    fn test_add_flush_get_delta() -> Fallible<()> {
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
        assert_eq!(packstore.get_delta(&k1)?.unwrap(), delta);
        Ok(())
    }

    #[test]
    fn test_histpack_add_get() -> Fallible<()> {
        let tempdir = TempDir::new()?;
        let packstore = MutableHistoryPackStore::new(&tempdir, CorruptionPolicy::REMOVE)?;

        let mut rng = ChaChaRng::from_seed([0u8; 32]);
        let (nodes, _) = get_nodes(&mut rng);
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
    fn test_histpack_add_flush_get() -> Fallible<()> {
        let tempdir = TempDir::new()?;
        let packstore = MutableHistoryPackStore::new(&tempdir, CorruptionPolicy::REMOVE)?;

        let mut rng = ChaChaRng::from_seed([0u8; 32]);
        let (nodes, _) = get_nodes(&mut rng);
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
}
