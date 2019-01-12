// Copyright 2018 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use datapack::{DataPack, DataPackVersion};
use datastore::DataStore;
use error::Result;
use historypack::{HistoryPack, HistoryPackVersion};
use historystore::HistoryStore;
use key::Key;
use mutabledatapack::MutableDataPack;
use mutablehistorypack::MutableHistoryPack;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq)]
pub enum RepackOutputType {
    Data,
    History,
}

pub trait IterableStore {
    fn iter<'a>(&'a self) -> Box<Iterator<Item = Result<Key>> + 'a>;
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
    fn delete(&self) -> Result<()>;
    fn id(&self) -> &Arc<PathBuf>;
    fn kind(&self) -> RepackOutputType;

    /// An iterator containing every key in the store, and identifying information for where it
    /// came from and what type it is (data vs history).
    fn repack_iter<'a>(
        &'a self,
    ) -> Box<Iterator<Item = Result<(Arc<PathBuf>, RepackOutputType, Key)>> + 'a> {
        let id = self.id().clone();
        let kind = self.kind().clone();
        Box::new(
            self.iter()
                .map(move |k| k.map(|k| (id.clone(), kind.clone(), k))),
        )
    }

    fn cleanup(&self, result: &RepackResult) -> Result<()> {
        let owned_keys = self.iter().collect::<Result<HashSet<Key>>>()?;
        if owned_keys.is_subset(result.packed_keys())
            && !result.created_packs().contains(self.id().as_ref())
        {
            self.delete()?;
        }

        Ok(())
    }
}

fn repack_datapack(data_pack: &DataPack, mut_pack: &mut MutableDataPack) -> Result<()> {
    for k in data_pack.iter() {
        let key = k?;
        let chain = data_pack.get_delta_chain(&key)?;
        for delta in chain.iter() {
            if mut_pack.get_delta(&delta.key).is_ok() {
                break;
            }

            let meta = Some(data_pack.get_meta(&delta.key)?);
            mut_pack.add(&delta, meta)?;
        }
    }

    Ok(())
}

pub fn repack_datapacks<'a>(
    paths: impl IntoIterator<Item = &'a PathBuf> + Clone,
    outdir: &Path,
) -> Result<PathBuf> {
    let mut empty = true;
    let mut mut_pack = MutableDataPack::new(outdir, DataPackVersion::One)?;

    for path in paths.clone() {
        let data_pack = DataPack::new(&path)?;
        repack_datapack(&data_pack, &mut mut_pack)?;
        empty = false;
    }

    if empty {
        Ok(PathBuf::new())
    } else {
        let new_pack_path = mut_pack.close()?;
        for path in paths {
            let datapack = DataPack::new(&path)?;
            if datapack.base_path() != new_pack_path {
                datapack.delete()?;
            }
        }

        Ok(new_pack_path)
    }
}

fn repack_historypack(history_pack: &HistoryPack, mut_pack: &mut MutableHistoryPack) -> Result<()> {
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
) -> Result<PathBuf> {
    let mut empty = true;
    let mut mut_pack = MutableHistoryPack::new(outdir, HistoryPackVersion::One)?;

    for path in paths.clone() {
        let history_pack = HistoryPack::new(path)?;
        repack_historypack(&history_pack, &mut mut_pack)?;
        empty = false;
    }

    if empty {
        Ok(PathBuf::new())
    } else {
        let new_pack_path = mut_pack.close()?;
        for path in paths {
            let history_pack = HistoryPack::new(path)?;
            if history_pack.base_path() != new_pack_path {
                history_pack.delete()?;
            }
        }

        Ok(new_pack_path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use datapack::tests::make_datapack;
    use datastore::Delta;
    use historypack::tests::{get_nodes, make_historypack};
    use historystore::Ancestors;
    use rand::SeedableRng;
    use rand_chacha::ChaChaRng;
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::rc::Rc;
    use tempfile::TempDir;
    use types::node::Node;

    struct FakeStore {
        pub kind: RepackOutputType,
        pub id: Arc<PathBuf>,
        pub keys: Vec<Key>,
        pub deleted: RefCell<bool>,
    }

    impl IterableStore for FakeStore {
        fn iter<'a>(&'a self) -> Box<Iterator<Item = Result<Key>> + 'a> {
            Box::new(self.keys.iter().map(|k| Ok(k.clone())))
        }
    }

    impl Repackable for FakeStore {
        fn delete(&self) -> Result<()> {
            let mut deleted = self.deleted.borrow_mut();
            *deleted = true;
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
        let store = FakeStore {
            kind: RepackOutputType::Data,
            id: Arc::new(PathBuf::from("foo/bar")),
            keys: vec![
                Key::new(Box::new([0]), Node::random(&mut rng)),
                Key::new(Box::new([0]), Node::random(&mut rng)),
            ],
            deleted: RefCell::new(false),
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
        assert_eq!(*store.deleted.borrow(), false);

        // Test cleanup where all keys are packe but created includes this store
        packed_keys.insert(store.keys[1].clone());
        created_packs.insert(store.id().to_path_buf());
        store
            .cleanup(&RepackResult::new(
                packed_keys.clone(),
                created_packs.clone(),
            ))
            .unwrap();
        assert_eq!(*store.deleted.borrow(), false);

        // Test cleanup where all keys are packed and created doesn't include this store
        created_packs.clear();
        store
            .cleanup(&RepackResult::new(
                packed_keys.clone(),
                created_packs.clone(),
            ))
            .unwrap();
        assert_eq!(*store.deleted.borrow(), true);
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
                data: Rc::new([1, 2, 3, 4]),
                base: None,
                key: Key::new(Box::new([0]), Node::random(&mut rng)),
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
            newpack.iter().collect::<Result<Vec<Key>>>().unwrap(),
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
            let base = Key::new(Box::new([0]), Node::random(&mut rng));
            let rev = vec![
                (
                    Delta {
                        data: Rc::new([1, 2, 3, 4]),
                        base: None,
                        key: base.clone(),
                    },
                    None,
                ),
                (
                    Delta {
                        data: Rc::new([1, 2, 3, 4]),
                        base: Some(base),
                        key: Key::new(Box::new([0]), Node::random(&mut rng)),
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
            newpack.iter().collect::<Result<Vec<Key>>>().unwrap(),
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
