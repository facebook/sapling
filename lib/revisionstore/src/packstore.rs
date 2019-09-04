// Copyright Facebook, Inc. 2019

use std::{
    cell::RefCell,
    collections::vec_deque::{Iter, IterMut},
    collections::VecDeque,
    fs::read_dir,
    io::ErrorKind,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use failure::{format_err, Fallible};

use types::{Key, NodeInfo};

use crate::datapack::DataPack;
use crate::datastore::{DataStore, Delta, Metadata};
use crate::error::KeyError;
use crate::historypack::HistoryPack;
use crate::historystore::{Ancestors, HistoryStore};
use crate::localstore::LocalStore;

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

/// A `PackStore` automatically keeps track of packfiles in a given directory. New on-disk
/// packfiles will be periodically scanned and opened accordingly.
pub struct PackStore<T> {
    pack_dir: PathBuf,
    extension: &'static str,
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
}

impl PackStoreOptions {
    fn new() -> Self {
        Self {
            pack_dir: PathBuf::new(),
            scan_frequency: Duration::from_secs(10),
            extension: "",
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

    fn build<T>(self) -> PackStore<T> {
        let now = Instant::now();
        let force_rescan = now - self.scan_frequency;

        PackStore {
            pack_dir: self.pack_dir,
            scan_frequency: self.scan_frequency,
            extension: self.extension,
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
}

impl DataPackStore {
    /// Build a new DataPackStore. The default rescan rate is 10 seconds.
    pub fn new<P: AsRef<Path>>(pack_dir: P) -> Self {
        PackStoreOptions::new()
            .directory(pack_dir)
            .extension("datapack")
            .build()
    }
}

impl HistoryPackStore {
    /// Build a new HistoryPackStore. The default rescan rate is 10 seconds.
    pub fn new<P: AsRef<Path>>(pack_dir: P) -> Self {
        PackStoreOptions::new()
            .directory(pack_dir)
            .extension("histpack")
            .build()
    }
}

impl<T: LocalStore> PackStore<T> {
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
    fn run<R, F>(&self, op: F, key: &Key) -> Fallible<R>
    where
        F: Fn(&T) -> Fallible<R>,
    {
        for _ in 0..2 {
            let mut found = None;
            {
                let mut lrustore = self.packs.try_borrow_mut()?;
                for (index, store) in lrustore.iter_mut().enumerate() {
                    match op(store) {
                        Ok(result) => {
                            found = Some((index, result));
                            break;
                        }
                        Err(e) => {
                            // When a store doesn't contain the asked data, it returns
                            // Err(KeyError). Ideally, the store interface should return a
                            // Fallible<Option<T>> and Ok(None) would indicate that the data asked
                            // isn't present. Until we make this change, we have to resort to using
                            // an ugly downcast :(
                            if e.downcast_ref::<KeyError>().is_none() {
                                return Err(e);
                            }
                        }
                    }
                }
            }

            if let Some((index, result)) = found {
                self.packs.borrow_mut().update(index);
                return Ok(result);
            }

            // We didn't find anything, let's try to probe the filesystem to discover new packfiles
            // and retry.
            if !self.try_scan()? {
                break;
            }
        }

        Err(KeyError::new(format_err!("Key {:?} not found in PackStore", key)).into())
    }
}

impl<T: LocalStore> LocalStore for PackStore<T> {
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
    fn get(&self, key: &Key) -> Fallible<Vec<u8>> {
        self.run(|store| store.get(key), key)
    }

    fn get_delta(&self, key: &Key) -> Fallible<Delta> {
        self.run(|store| store.get_delta(key), key)
    }

    fn get_delta_chain(&self, key: &Key) -> Fallible<Vec<Delta>> {
        self.run(|store| store.get_delta_chain(key), key)
    }

    fn get_meta(&self, key: &Key) -> Fallible<Metadata> {
        self.run(|store| store.get_meta(key), key)
    }
}

impl HistoryStore for HistoryPackStore {
    fn get_ancestors(&self, key: &Key) -> Fallible<Ancestors> {
        self.run(|store| store.get_ancestors(key), key)
    }

    fn get_node_info(&self, key: &Key) -> Fallible<NodeInfo> {
        self.run(|store| store.get_node_info(key), key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

        let store = DataPackStore::new(&tempdir);
        let delta = store.get_delta(&k)?;
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

        let store = DataPackStore::new(&tempdir);
        let missing = store.get_missing(&vec![k])?;
        assert_eq!(missing.len(), 0);
        Ok(())
    }

    #[test]
    fn test_datapack_created_after() -> Fallible<()> {
        let tempdir = TempDir::new()?;
        let store = DataPackStore::new(&tempdir);

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

        let delta = store.get_delta(&k)?;
        assert_eq!(delta, revision.0);
        Ok(())
    }

    #[test]
    #[should_panic(expected = "KeyError")]
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

        store.get_delta(&k).unwrap();
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
        store.get_delta(&k)?;
        Ok(())
    }

    #[test]
    fn test_histpack() -> Fallible<()> {
        let mut rng = ChaChaRng::from_seed([0u8; 32]);
        let tempdir = TempDir::new()?;
        let store = HistoryPackStore::new(&tempdir);

        let (nodes, _) = get_nodes(&mut rng);
        make_historypack(&tempdir, &nodes);
        for (key, info) in nodes.iter() {
            let response: NodeInfo = store.get_node_info(key)?;
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

        let packstore = DataPackStore::new(&tempdir);

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
        let store = HistoryPackStore::new(&non_present_tempdir);

        store.rescan()
    }
}
