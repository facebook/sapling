use std::collections::HashMap;
use std::path::{Path, PathBuf};

use ancestors::AncestorIterator;
use error::Result;
use historypack::HistoryPackVersion;
use historystore::{Ancestors, HistoryStore, NodeInfo};
use key::Key;

#[derive(Debug, Fail)]
#[fail(display = "Mutable History Pack Error: {:?}", _0)]
struct MutableHistoryPackError(String);

pub struct MutableHistoryPack {
    version: HistoryPackVersion,
    dir: PathBuf,
    mem_index: HashMap<Box<[u8]>, HashMap<Key, NodeInfo>>,
}

impl MutableHistoryPack {
    pub fn new(dir: &Path, version: HistoryPackVersion) -> Result<Self> {
        if !dir.is_dir() {
            return Err(MutableHistoryPackError(format!(
                "cannot create mutable historypack in non-directory '{:?}'",
                dir
            )).into());
        }

        Ok(MutableHistoryPack {
            version: version,
            dir: dir.to_path_buf(),
            mem_index: HashMap::new(),
        })
    }

    pub fn add(&mut self, key: &Key, info: &NodeInfo) -> Result<()> {
        // Ideally we could use something like:
        //     self.mem_index.entry(key.name()).or_insert_with(|| HashMap::new())
        // To get the inner map, then insert our new NodeInfo. Unfortunately it requires
        // key.name().clone() though. So we have to do it the long way to avoid the allocation.
        let entries = self.mem_index
            .entry(key.name().to_vec().into_boxed_slice())
            .or_insert_with(|| HashMap::new());
        entries.insert(key.clone(), info.clone());
        Ok(())
    }
}

impl HistoryStore for MutableHistoryPack {
    fn get_ancestors(&self, key: &Key) -> Result<Ancestors> {
        AncestorIterator::new(key, |k, _seen| self.get_node_info(k)).collect()
    }

    fn get_node_info(&self, key: &Key) -> Result<NodeInfo> {
        Ok(self.mem_index
            .get(key.name())
            .ok_or(MutableHistoryPackError(format!(
                "key '{:?}' not present in mutable history pack",
                key
            )))?
            .get(key)
            .ok_or(MutableHistoryPackError(format!(
                "key '{:?}' not present in mutable history pack",
                key
            )))?
            .clone())
    }

    fn get_missing(&self, keys: &[Key]) -> Result<Vec<Key>> {
        Ok(keys.iter()
            .filter(|k| match self.mem_index.get(k.name()) {
                Some(e) => e.get(k).is_none(),
                None => true,
            })
            .map(|k| k.clone())
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::Rng;
    use rand::SeedableRng;
    use rand::chacha::ChaChaRng;
    use tempfile::tempdir;

    use node::Node;

    quickcheck! {
        fn test_get_ancestors(keys: Vec<(Key, bool)>) -> bool {
            let mut rng = ChaChaRng::from_seed([0u8; 32]);
            let tempdir = tempdir().unwrap();
            let mut muthistorypack =
                MutableHistoryPack::new(tempdir.path(), HistoryPackVersion::One).unwrap();

            // Insert all the keys, randomly choosing nodes from the already inserted keys
            let mut chains = HashMap::<Key, Ancestors>::new();
            chains.insert(Key::default(), Ancestors::new());
            for &(ref key, ref has_p2) in keys.iter() {
                let mut p1 = Key::default();
                let mut p2 = Key::default();
                let available_parents = chains.keys().map(|k| k.clone()).collect::<Vec<Key>>();

                if !chains.is_empty() {
                    p1 = rng.choose(&available_parents[..])
                        .expect("choose p1")
                        .clone();

                    if *has_p2 {
                        p2 = rng.choose(&available_parents[..])
                            .expect("choose p2")
                            .clone();
                    }
                }

                // Insert into the history pack
                let info = NodeInfo {
                    parents: [p1.clone(), p2.clone()],
                    linknode: Node::random(&mut rng),
                };
                muthistorypack.add(&key, &info).unwrap();

                // Compute the ancestors for the inserted key
                let p1_ancestors = chains.get(&p1).expect("get p1 ancestors").clone();
                let p2_ancestors = chains.get(&p2).expect("get p2 ancestors").clone();
                let mut ancestors = Ancestors::new();
                ancestors.extend(p1_ancestors);
                ancestors.extend(p2_ancestors);
                ancestors.insert(key.clone(), info.clone());
                chains.insert(key.clone(), ancestors);
            }

            for &(ref key, _) in keys.iter() {
                let in_pack = muthistorypack.get_ancestors(&key).expect("get ancestors");
                if in_pack != chains[&key] {
                    return false;
                }
            }

            true
        }

        fn test_get_node_info(insert: HashMap<Key, NodeInfo>, notinsert: Vec<Key>) -> bool {
            let tempdir = tempdir().unwrap();
            let mut muthistorypack =
                MutableHistoryPack::new(tempdir.path(), HistoryPackVersion::One).unwrap();

            for (key, info) in insert.iter() {
                muthistorypack.add(&key, &info).unwrap();
            }

            for (key, info) in insert.iter() {
                if *info != muthistorypack.get_node_info(key).unwrap() {
                    return false;
                }
            }

            for key in notinsert.iter() {
                if muthistorypack.get_node_info(key).is_ok() {
                    return false;
                }
            }

            true
        }

        fn test_get_missing(insert: HashMap<Key, NodeInfo>, notinsert: Vec<Key>) -> bool {
            let tempdir = tempdir().unwrap();
            let mut muthistorypack =
                MutableHistoryPack::new(tempdir.path(), HistoryPackVersion::One).unwrap();

            for (key, info) in insert.iter() {
                muthistorypack.add(&key, &info).unwrap();
            }

            let mut lookup = notinsert.clone();
            lookup.extend(insert.keys().map(|k| k.clone()));

            let missing = muthistorypack.get_missing(&lookup).unwrap();
            missing == notinsert
        }
    }
}
