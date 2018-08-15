use std::collections::HashMap;
use std::path::{Path, PathBuf};

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
        unimplemented!();
    }

    fn get_node_info(&self, key: &Key) -> Result<NodeInfo> {
        unimplemented!();
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

    use tempfile::tempdir;

    quickcheck! {
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
