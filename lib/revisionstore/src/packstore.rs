// Copyright Facebook, Inc. 2019

use std::{
    cell::RefCell,
    fs::read_dir,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use failure::Fallible;

use types::{Key, NodeInfo};

use crate::datapack::DataPack;
use crate::datastore::{DataStore, Delta, Metadata};
use crate::error::KeyError;
use crate::historypack::HistoryPack;
use crate::historystore::{Ancestors, HistoryStore};
use crate::localstore::LocalStore;
use crate::unionstore::UnionStore;

/// A `PackStore` automatically keeps track of packfiles in a given directory. New on-disk
/// packfiles will be periodically scanned and opened accordingly.
pub struct PackStore<T> {
    pack_dir: PathBuf,
    scan_frequency: Duration,
    last_scanned: RefCell<Instant>,
    packs: RefCell<UnionStore<T>>,
}

pub type DataPackStore = PackStore<DataPack>;
pub type HistoryPackStore = PackStore<HistoryPack>;

impl<T> PackStore<T> {
    /// Create a new PackStore. The default rescan period is 10 seconds.
    pub fn new<P: AsRef<Path>>(pack_dir: P) -> Self {
        Self::with_scan_frequency(pack_dir, Duration::from_secs(10))
    }

    fn with_scan_frequency<P: AsRef<Path>>(pack_dir: P, scan_frequency: Duration) -> Self {
        let now = Instant::now();
        let force_rescan = now - scan_frequency;

        Self {
            pack_dir: PathBuf::from(pack_dir.as_ref()),
            scan_frequency,
            last_scanned: RefCell::new(force_rescan),
            packs: RefCell::new(UnionStore::new()),
        }
    }

    /// Force a rescan to be performed. Since rescan are expensive, this should only be called for
    /// out-of-process created packfiles.
    pub fn force_rescan(&self) {
        self.last_scanned
            .replace(Instant::now() - self.scan_frequency);
    }
}

impl<T: LocalStore> PackStore<T> {
    /// Open new on-disk packfiles, and close removed ones.
    fn rescan(&self) -> Fallible<()> {
        let mut new_packs = UnionStore::new();
        for entry in read_dir(&self.pack_dir)? {
            let entry = entry?;
            if entry.file_type()?.is_file() {
                let path = entry.path();

                // Only open the packfile, not the indexes.
                if let Some(ext) = path.extension() {
                    if let Some(ext) = ext.to_str() {
                        if ext.ends_with("pack") {
                            if let Ok(pack) = T::from_path(&path) {
                                new_packs.add(pack);
                            }
                        }
                    }
                }
            }
        }

        self.packs.replace(new_packs);
        Ok(())
    }

    /// Execute the `op` function. May call `rescan` when `op` fails with `KeyError`.
    fn run<R, F>(&self, op: F) -> Fallible<R>
    where
        F: Fn(&UnionStore<T>) -> Fallible<R>,
    {
        let res = op(&*self.packs.try_borrow()?);
        match res {
            Ok(ret) => Ok(ret),
            Err(e) => {
                let now = Instant::now();

                // When a store doesn't contain the asked data, it returns Err(KeyError). Ideally,
                // the store interface should return a Fallible<Option<T>> and Ok(None) would
                // indicate that the data asked isn't present. Until we make this change, we have
                // to resort to using an ugly downcast :(
                if now.duration_since(*self.last_scanned.borrow()) >= self.scan_frequency
                    && e.downcast_ref::<KeyError>().is_some()
                {
                    self.rescan()?;
                    self.last_scanned.replace(now);
                    op(&*self.packs.try_borrow()?)
                } else {
                    Err(e)
                }
            }
        }
    }
}

impl<T: LocalStore> LocalStore for PackStore<T> {
    fn get_missing(&self, keys: &[Key]) -> Fallible<Vec<Key>> {
        self.packs.try_borrow()?.get_missing(keys)
    }
}

impl DataStore for DataPackStore {
    fn get(&self, key: &Key) -> Fallible<Vec<u8>> {
        self.run(|store| store.get(key))
    }

    fn get_delta(&self, key: &Key) -> Fallible<Delta> {
        self.run(|store| store.get_delta(key))
    }

    fn get_delta_chain(&self, key: &Key) -> Fallible<Vec<Delta>> {
        self.run(|store| store.get_delta_chain(key))
    }

    fn get_meta(&self, key: &Key) -> Fallible<Metadata> {
        self.run(|store| store.get_meta(key))
    }
}

impl HistoryStore for HistoryPackStore {
    fn get_ancestors(&self, key: &Key) -> Fallible<Ancestors> {
        self.run(|store| store.get_ancestors(key))
    }

    fn get_node_info(&self, key: &Key) -> Fallible<NodeInfo> {
        self.run(|store| store.get_node_info(key))
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
        let store = DataPackStore::with_scan_frequency(&tempdir, Duration::from_secs(1000));

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
        let store = DataPackStore::with_scan_frequency(&tempdir, Duration::from_secs(1000));

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
}
