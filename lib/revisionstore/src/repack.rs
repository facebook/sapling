// Copyright 2018 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

use failure::{format_err, Error, Fail, Fallible};

use types::Key;

use crate::datapack::{DataPack, DataPackVersion};
use crate::datastore::{DataStore, MutableDeltaStore};
use crate::historypack::{HistoryPack, HistoryPackVersion};
use crate::historystore::{HistoryStore, MutableHistoryStore};
use crate::localstore::LocalStore;
use crate::mutabledatapack::MutableDataPack;
use crate::mutablehistorypack::MutableHistoryPack;
use crate::mutablepack::MutablePack;

#[derive(Debug, Clone, PartialEq)]
pub enum RepackOutputType {
    Data,
    History,
}

pub trait IterableStore {
    fn iter<'a>(&'a self) -> Box<dyn Iterator<Item = Fallible<Key>> + 'a>;
}

pub struct RepackResult {
    packed_keys: HashSet<Key>,
    created_packs: HashSet<PathBuf>,
}

impl RepackResult {
    // new() should probably be crate-local, since the repack implementation is the only thing that
    // constructs it. But the python integration layer currently needs to construct this, so it
    // needs to be externally public for now.
    pub fn new(packed_keys: HashSet<Key>, created_packs: HashSet<PathBuf>) -> Self {
        RepackResult {
            packed_keys,
            created_packs,
        }
    }

    /// Returns the set of created pack files. The paths do not include the .pack/.idx suffixes.
    pub fn created_packs(&self) -> &HashSet<PathBuf> {
        &self.created_packs
    }

    pub fn packed_keys(&self) -> &HashSet<Key> {
        &self.packed_keys
    }
}

pub trait Repackable: IterableStore {
    fn delete(self) -> Fallible<()>;
    fn id(&self) -> &Arc<PathBuf>;
    fn kind(&self) -> RepackOutputType;

    /// An iterator containing every key in the store, and identifying information for where it
    /// came from and what type it is (data vs history).
    fn repack_iter<'a>(
        &'a self,
    ) -> Box<dyn Iterator<Item = Fallible<(Arc<PathBuf>, RepackOutputType, Key)>> + 'a> {
        let id = self.id().clone();
        let kind = self.kind().clone();
        Box::new(
            self.iter()
                .map(move |k| k.map(|k| (id.clone(), kind.clone(), k))),
        )
    }

    fn cleanup(self, result: &RepackResult) -> Fallible<()>
    where
        Self: Sized,
    {
        let owned_keys = self.iter().collect::<Fallible<HashSet<Key>>>()?;
        if owned_keys.is_subset(result.packed_keys())
            && !result.created_packs().contains(self.id().as_ref())
        {
            self.delete()?;
        }

        Ok(())
    }
}

fn repack_datapack(data_pack: &DataPack, mut_pack: &mut MutableDataPack) -> Fallible<()> {
    for k in data_pack.iter() {
        let key = k?;
        let chain = data_pack.get_delta_chain(&key)?;
        for delta in chain.iter() {
            if mut_pack.contains(&delta.key)? {
                break;
            }

            let meta = data_pack.get_meta(&delta.key)?;
            mut_pack.add(&delta, &meta)?;
        }
    }

    Ok(())
}

#[derive(Debug, Fail)]
enum RepackFailure {
    #[fail(display = "Repack failure: {:?}", _0)]
    Total(Vec<(PathBuf, Error)>),

    #[fail(display = "Repack successful but with errors: {:?}", _1)]
    Partial(PathBuf, Vec<(PathBuf, Error)>),
}

/// Repack all pack files in the paths iterator. Once repacked, the repacked packs will be removed
/// from the filesystem.
fn repack_packs<'a, T: MutablePack, U: LocalStore + Repackable>(
    paths: impl IntoIterator<Item = &'a PathBuf> + Clone,
    mut mut_pack: T,
    repack_pack: impl Fn(&U, &mut T) -> Fallible<()>,
) -> Fallible<PathBuf> {
    if paths.clone().into_iter().count() <= 1 {
        if let Some(path) = paths.into_iter().next() {
            return Ok(path.to_path_buf());
        } else {
            return Ok(PathBuf::new());
        }
    }

    let mut repacked = vec![];
    let mut errors = vec![];

    for path in paths {
        match U::from_path(&path) {
            Ok(pack) => {
                if let Err(e) = repack_pack(&pack, &mut mut_pack) {
                    errors.push((path.clone(), e));
                } else {
                    repacked.push(path);
                }
            }
            Err(e) => errors.push((path.clone(), e)),
        }
    }

    if repacked.len() == 0 {
        return Err(RepackFailure::Total(errors).into());
    }

    let new_pack_path = mut_pack.close_pack()?;
    let new_pack = U::from_path(&new_pack_path)?;

    let mut successfully_repacked = 0;
    for path in repacked {
        if *path != new_pack_path {
            let pack = match U::from_path(&path) {
                Ok(pack) => pack,
                Err(_e) => {
                    // We were about to remove this file, let's just ignore the failures to open
                    // it.
                    successfully_repacked += 1;
                    continue;
                }
            };

            let keys = pack.iter().filter_map(|res| res.ok()).collect::<Vec<Key>>();
            let missing = new_pack.get_missing(&keys)?;

            if missing.len() == 0 {
                let _ = pack.delete();
                successfully_repacked += 1;
            } else {
                errors.push((path.clone(), format_err!("{:?}", missing)));
            }
        } else {
            successfully_repacked += 1;
        }
    }

    if successfully_repacked == 0 {
        Err(RepackFailure::Total(errors).into())
    } else if errors.len() != 0 {
        Err(RepackFailure::Partial(new_pack_path, errors).into())
    } else {
        Ok(new_pack_path)
    }
}

pub fn repack_datapacks<'a>(
    paths: impl IntoIterator<Item = &'a PathBuf> + Clone,
    outdir: &Path,
) -> Fallible<PathBuf> {
    let mut_pack = MutableDataPack::new(outdir, DataPackVersion::One)?;

    repack_packs(paths, mut_pack, repack_datapack)
}

fn repack_historypack(
    history_pack: &HistoryPack,
    mut_pack: &mut MutableHistoryPack,
) -> Fallible<()> {
    for k in history_pack.iter() {
        let key = k?;
        let node = history_pack.get_node_info(&key)?;
        mut_pack.add(&key, &node)?;
    }

    Ok(())
}

pub fn repack_historypacks<'a>(
    paths: impl IntoIterator<Item = &'a PathBuf> + Clone,
    outdir: &Path,
) -> Fallible<PathBuf> {
    let mut_pack = MutableHistoryPack::new(outdir, HistoryPackVersion::One)?;

    repack_packs(paths, mut_pack, repack_historypack)
}

/// List all the pack files in the directory `dir` that ends with `extension`.
pub fn list_packs(dir: &Path, extension: &str) -> Fallible<Vec<PathBuf>> {
    let mut dirents = fs::read_dir(dir)?
        .filter_map(|e| match e {
            Err(_) => None,
            Ok(entry) => {
                let entrypath = entry.path();
                if entrypath.extension() == Some(extension.as_ref()) {
                    Some(entrypath.with_extension(""))
                } else {
                    None
                }
            }
        })
        .collect::<Vec<PathBuf>>();
    dirents.sort_unstable();
    Ok(dirents)
}

/// Select all the packs from `packs` that needs to be repacked during an incremental repack.
///
/// The filtering is fairly basic and is intended to reduce the fragmentation of pack files.
pub fn filter_incrementalpacks<'a>(packs: Vec<PathBuf>, extension: &str) -> Fallible<Vec<PathBuf>> {
    // XXX: Read these from the configuration.
    let mut repackmaxpacksize = 4 * 1024 * 1024 * 1024;
    if extension == "histpack" {
        // Per 100MB of histpack size, the memory consumption is over 1GB, thus repacking 4GB
        // would need over 40GB of RAM.
        repackmaxpacksize = 400 * 1024 * 1024;
    }
    let repacksizelimit = 100 * 1024 * 1024;
    let min_packs = 50;

    let mut packssizes = packs
        .into_iter()
        .map(|p| {
            let size = p
                .with_extension(extension)
                .metadata()
                .and_then(|m| Ok(m.len()))
                .unwrap_or(u64::max_value());
            (p, size)
        })
        .collect::<Vec<(PathBuf, u64)>>();

    // Sort by file size in increasing order
    packssizes.sort_unstable_by(|a, b| a.1.cmp(&b.1));

    let mut num_packs = packssizes.len();
    let mut accumulated_sizes = 0;
    Ok(packssizes
        .into_iter()
        .take_while(|e| {
            if e.1 + accumulated_sizes > repackmaxpacksize {
                return false;
            }

            if e.1 > repacksizelimit && num_packs < min_packs {
                false
            } else {
                accumulated_sizes += e.1;
                num_packs -= 1;

                true
            }
        })
        .map(|e| e.0)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    use bytes::Bytes;
    use rand::SeedableRng;
    use rand_chacha::ChaChaRng;
    use tempfile::TempDir;

    use types::testutil::*;

    use std::{
        collections::HashMap,
        fs::{set_permissions, OpenOptions},
        io::Write,
        rc::Rc,
        sync::atomic::{AtomicBool, Ordering},
    };

    use crate::datapack::tests::make_datapack;
    use crate::datastore::Delta;
    use crate::historypack::tests::{get_nodes, make_historypack};
    use crate::historystore::Ancestors;

    #[derive(Clone)]
    struct FakeStore {
        pub kind: RepackOutputType,
        pub id: Arc<PathBuf>,
        pub keys: Vec<Key>,
        pub deleted: Rc<AtomicBool>,
    }

    impl IterableStore for FakeStore {
        fn iter<'a>(&'a self) -> Box<dyn Iterator<Item = Fallible<Key>> + 'a> {
            Box::new(self.keys.iter().map(|k| Ok(k.clone())))
        }
    }

    impl Repackable for FakeStore {
        fn delete(self) -> Fallible<()> {
            self.deleted.store(true, Ordering::Release);
            Ok(())
        }

        fn id(&self) -> &Arc<PathBuf> {
            &self.id
        }

        fn kind(&self) -> RepackOutputType {
            self.kind.clone()
        }
    }

    #[test]
    fn test_repackable() {
        let is_deleted = Rc::new(AtomicBool::new(false));
        let store = FakeStore {
            kind: RepackOutputType::Data,
            id: Arc::new(PathBuf::from("foo/bar")),
            keys: vec![key("a", "1"), key("b", "2")],
            deleted: is_deleted.clone(),
        };

        let mut marked: Vec<(Arc<PathBuf>, RepackOutputType, Key)> = vec![];
        for entry in store.repack_iter() {
            marked.push(entry.unwrap());
        }
        assert_eq!(
            marked,
            vec![
                (store.id.clone(), store.kind.clone(), store.keys[0].clone()),
                (store.id.clone(), store.kind.clone(), store.keys[1].clone()),
            ]
        );

        let store2 = store.clone();

        // Test cleanup where the pack, testutil::*ed keys don't some store keys
        let mut packed_keys = HashSet::new();
        packed_keys.insert(store.keys[0].clone());
        let mut created_packs = HashSet::new();
        store
            .cleanup(&RepackResult::new(
                packed_keys.clone(),
                created_packs.clone(),
            ))
            .unwrap();
        assert_eq!(is_deleted.load(Ordering::Acquire), false);

        let store = store2.clone();

        // Test cleanup where all keys are packe but created includes this store
        packed_keys.insert(store.keys[1].clone());
        created_packs.insert(store.id().to_path_buf());
        store
            .cleanup(&RepackResult::new(
                packed_keys.clone(),
                created_packs.clone(),
            ))
            .unwrap();
        assert_eq!(is_deleted.load(Ordering::Acquire), false);

        let store = store2.clone();

        // Test cleanup where all keys are packed and created doesn't include this store
        created_packs.clear();
        store
            .cleanup(&RepackResult::new(
                packed_keys.clone(),
                created_packs.clone(),
            ))
            .unwrap();
        assert_eq!(is_deleted.load(Ordering::Acquire), true);
    }

    #[test]
    fn test_repack_no_datapack() {
        let tempdir = TempDir::new().unwrap();

        let newpath = repack_datapacks(vec![].iter(), tempdir.path());
        assert!(newpath.is_ok());
        let newpath = newpath.unwrap();
        assert_eq!(newpath.to_str(), Some(""));
    }

    #[test]
    fn test_repack_one_datapack() {
        let tempdir = TempDir::new().unwrap();

        let revisions = vec![(
            Delta {
                data: Bytes::from(&[1u8, 2, 3, 4][..]),
                base: None,
                key: key("a", "1"),
            },
            Default::default(),
        )];

        let pack = make_datapack(&tempdir, &revisions);
        let newpath = repack_datapacks(vec![pack.base_path().to_path_buf()].iter(), tempdir.path());
        assert!(newpath.is_ok());
        let newpath2 = newpath.unwrap();
        assert_eq!(newpath2.with_extension("datapack"), pack.pack_path());
        let datapack = DataPack::new(&newpath2);
        assert!(datapack.is_ok());
        let newpack = datapack.unwrap();
        assert_eq!(
            newpack.iter().collect::<Fallible<Vec<Key>>>().unwrap(),
            revisions
                .iter()
                .map(|d| d.0.key.clone())
                .collect::<Vec<Key>>()
        );
    }

    #[test]
    fn test_repack_multiple_datapacks() {
        let tempdir = TempDir::new().unwrap();
        let mut revisions = Vec::new();
        let mut paths = Vec::new();

        for i in 1..=2 {
            let base = key("a", &i.to_string());
            let rev = vec![
                (
                    Delta {
                        data: Bytes::from(&[1, 2, 3, 4][..]),
                        base: None,
                        key: base.clone(),
                    },
                    Default::default(),
                ),
                (
                    Delta {
                        data: Bytes::from(&[1, 2, 3, 4][..]),
                        base: Some(base),
                        key: key("a", &(100 + i).to_string()),
                    },
                    Default::default(),
                ),
            ];
            let pack = make_datapack(&tempdir, &rev);
            let path = pack.base_path().to_path_buf();
            revisions.push(rev);
            paths.push(path);
        }

        let newpath = repack_datapacks(paths.iter(), tempdir.path());
        assert!(newpath.is_ok());
        let newpack = DataPack::new(&newpath.unwrap()).unwrap();
        assert_eq!(
            newpack.iter().collect::<Fallible<Vec<Key>>>().unwrap(),
            revisions
                .iter()
                .flatten()
                .map(|d| d.0.key.clone())
                .collect::<Vec<Key>>()
        );
    }

    #[test]
    fn test_repack_missing_files() {
        let tempdir = TempDir::new().unwrap();

        let paths = vec![PathBuf::from("foo.datapack"), PathBuf::from("bar.datapack")];
        let files = paths.iter().map(|p| p).collect::<Vec<&PathBuf>>();
        let res = repack_datapacks(files.clone(), tempdir.path())
            .err()
            .unwrap();

        if let Some(RepackFailure::Total(errors)) = res.downcast_ref() {
            assert!(errors.iter().map(|(path, _)| path).eq(files));
        } else {
            assert!(false);
        }
    }

    #[test]
    fn test_repack_corrupted() {
        let tempdir = TempDir::new().unwrap();
        let mut revisions = Vec::new();
        let mut paths = Vec::new();

        for i in 1..=2 {
            let base = key("a", &i.to_string());
            let rev = vec![
                (
                    Delta {
                        data: Bytes::from(&[1, 2, 3, 4][..]),
                        base: None,
                        key: base.clone(),
                    },
                    Default::default(),
                ),
                (
                    Delta {
                        data: Bytes::from(&[1, 2, 3, 4][..]),
                        base: Some(base),
                        key: key("a", &(100 + i).to_string()),
                    },
                    Default::default(),
                ),
            ];
            let pack = make_datapack(&tempdir, &rev);
            let path = pack.base_path().to_path_buf();
            revisions.push(rev);
            paths.push(path);
        }

        let mut to_corrupt = paths.get(0).unwrap().clone();
        to_corrupt.set_extension("datapack");
        let mut perms = to_corrupt.metadata().unwrap().permissions();
        perms.set_readonly(false);
        set_permissions(to_corrupt.clone(), perms).unwrap();
        let mut file = OpenOptions::new()
            .write(true)
            .open(to_corrupt.clone())
            .unwrap();
        file.write_all(b"FOOBARBAZ").unwrap();
        drop(file);

        let res = repack_datapacks(paths.iter(), tempdir.path())
            .err()
            .unwrap();

        if let Some(RepackFailure::Partial(_, errors)) = res.downcast_ref() {
            assert_eq!(errors.iter().count(), 1);
            to_corrupt.set_extension("");
            assert!(errors.iter().find(|(p, _)| p == &to_corrupt).is_some());
        } else {
            assert!(false);
        }
    }

    #[test]
    fn test_repack_one_historypack() {
        let mut rng = ChaChaRng::from_seed([0u8; 32]);
        let tempdir = TempDir::new().unwrap();

        let (nodes, ancestors) = get_nodes(&mut rng);

        let pack = make_historypack(&tempdir, &nodes);
        let newpath =
            repack_historypacks(vec![pack.base_path().to_path_buf()].iter(), tempdir.path());
        assert!(newpath.is_ok());
        let newpack = HistoryPack::new(&newpath.unwrap()).unwrap();

        for (ref key, _) in nodes.iter() {
            let response: Ancestors = newpack.get_ancestors(key).unwrap();
            assert_eq!(&response, ancestors.get(key).unwrap());
        }
    }

    #[test]
    fn test_repack_multiple_historypack() {
        let mut rng = ChaChaRng::from_seed([0u8; 32]);
        let tempdir = TempDir::new().unwrap();
        let mut ancestors = HashMap::new();
        let mut nodes = HashMap::new();
        let mut paths = Vec::new();

        for _ in 0..2 {
            let (node, ancestor) = get_nodes(&mut rng);
            let pack = make_historypack(&tempdir, &node);
            let path = pack.base_path().to_path_buf();

            ancestors.extend(ancestor.into_iter());
            nodes.extend(node.into_iter());
            paths.push(path);
        }

        let newpath = repack_historypacks(paths.iter(), tempdir.path());
        assert!(newpath.is_ok());
        let newpack = HistoryPack::new(&newpath.unwrap()).unwrap();

        for (key, _) in nodes.iter() {
            let response = newpack.get_ancestors(&key).unwrap();
            assert_eq!(&response, ancestors.get(key).unwrap());
        }
    }
}
