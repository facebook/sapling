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

use failure::{Fail, Fallible};

use types::Key;

use crate::datapack::{DataPack, DataPackVersion};
use crate::datastore::DataStore;
use crate::historypack::{HistoryPack, HistoryPackVersion};
use crate::historystore::HistoryStore;
use crate::mutabledatapack::MutableDataPack;
use crate::mutablehistorypack::MutableHistoryPack;
use crate::mutablepack::MutablePack;

#[derive(Debug, Clone, PartialEq)]
pub enum RepackOutputType {
    Data,
    History,
}

pub trait IterableStore {
    fn iter<'a>(&'a self) -> Box<Iterator<Item = Fallible<Key>> + 'a>;
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
    ) -> Box<Iterator<Item = Fallible<(Arc<PathBuf>, RepackOutputType, Key)>> + 'a> {
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

            let meta = Some(data_pack.get_meta(&delta.key)?);
            mut_pack.add(&delta, meta)?;
        }
    }

    Ok(())
}

#[derive(Debug, Fail)]
#[fail(display = "Repack failure: {:?}", _0)]
struct RepackFailure(Vec<(PathBuf, Vec<Key>)>);

pub fn repack_datapacks<'a>(
    paths: impl IntoIterator<Item = &'a PathBuf> + Clone,
    outdir: &Path,
) -> Fallible<PathBuf> {
    let mut mut_pack = MutableDataPack::new(outdir, DataPackVersion::One)?;

    if paths.clone().into_iter().count() <= 1 {
        if let Some(path) = paths.into_iter().next() {
            return Ok(path.to_path_buf());
        } else {
            return Ok(PathBuf::new());
        }
    }

    for path in paths.clone() {
        let data_pack = DataPack::new(&path)?;
        repack_datapack(&data_pack, &mut mut_pack)?;
    }

    let new_pack_path = mut_pack.close()?;
    let new_pack = DataPack::new(&new_pack_path)?;

    let mut errors = vec![];
    for path in paths {
        let datapack = DataPack::new(&path)?;

        if datapack.base_path() != new_pack_path {
            let keys = datapack
                .iter()
                .filter_map(|res| res.ok())
                .collect::<Vec<Key>>();
            let missing = new_pack.get_missing(&keys)?;

            if missing.len() == 0 {
                datapack.delete()?;
            } else {
                errors.push((path.clone(), missing));
            }
        }
    }

    if errors.len() != 0 {
        Err(RepackFailure(errors).into())
    } else {
        Ok(new_pack_path)
    }
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
    let mut mut_pack = MutableHistoryPack::new(outdir, HistoryPackVersion::One)?;

    if paths.clone().into_iter().count() <= 1 {
        if let Some(path) = paths.into_iter().next() {
            return Ok(path.to_path_buf());
        } else {
            return Ok(PathBuf::new());
        }
    }

    for path in paths.clone() {
        let history_pack = HistoryPack::new(path)?;
        repack_historypack(&history_pack, &mut mut_pack)?;
    }

    let new_pack_path = mut_pack.close()?;
    let new_pack = HistoryPack::new(&new_pack_path)?;

    let mut errors = vec![];
    for path in paths {
        let history_pack = HistoryPack::new(path)?;
        if history_pack.base_path() != new_pack_path {
            let keys = history_pack
                .iter()
                .filter_map(|res| res.ok())
                .collect::<Vec<Key>>();
            let missing = new_pack.get_missing(&keys)?;

            if missing.len() == 0 {
                history_pack.delete()?;
            } else {
                errors.push((path.clone(), missing));
            }
        }
    }

    if errors.len() != 0 {
        Err(RepackFailure(errors).into())
    } else {
        Ok(new_pack_path)
    }
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
    let repackmaxpacksize = 4 * 1024 * 1024 * 1024;
    let repacksizelimit = 100 * 1024 * 1024;

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

    let mut accumulated_sizes = 0;
    Ok(packssizes
        .into_iter()
        .take_while(|e| {
            if e.1 > repacksizelimit || e.1 + accumulated_sizes > repackmaxpacksize {
                false
            } else {
                accumulated_sizes += e.1;
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

    use types::node::Node;

    use std::{
        collections::HashMap,
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
        fn iter<'a>(&'a self) -> Box<Iterator<Item = Fallible<Key>> + 'a> {
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
        let mut rng = ChaChaRng::from_seed([0u8; 32]);
        let is_deleted = Rc::new(AtomicBool::new(false));
        let store = FakeStore {
            kind: RepackOutputType::Data,
            id: Arc::new(PathBuf::from("foo/bar")),
            keys: vec![
                Key::new(vec![0], Node::random(&mut rng)),
                Key::new(vec![0], Node::random(&mut rng)),
            ],
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

        // Test cleanup where the packed keys don't some store keys
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
        let mut rng = ChaChaRng::from_seed([0u8; 32]);
        let tempdir = TempDir::new().unwrap();

        let revisions = vec![(
            Delta {
                data: Bytes::from(&[1u8, 2, 3, 4][..]),
                base: None,
                key: Key::new(vec![0], Node::random(&mut rng)),
            },
            None,
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
        let mut rng = ChaChaRng::from_seed([0u8; 32]);
        let tempdir = TempDir::new().unwrap();
        let mut revisions = Vec::new();
        let mut paths = Vec::new();

        for _ in 0..2 {
            let base = Key::new(vec![0], Node::random(&mut rng));
            let rev = vec![
                (
                    Delta {
                        data: Bytes::from(&[1, 2, 3, 4][..]),
                        base: None,
                        key: base.clone(),
                    },
                    None,
                ),
                (
                    Delta {
                        data: Bytes::from(&[1, 2, 3, 4][..]),
                        base: Some(base),
                        key: Key::new(vec![0], Node::random(&mut rng)),
                    },
                    None,
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
