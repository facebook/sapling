use byteorder::WriteBytesExt;
use crypto::digest::Digest;
use crypto::sha1::Sha1;
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use tempfile::NamedTempFile;

use ancestors::{AncestorIterator, AncestorTraversal};
use error::Result;
use historyindex::{FileSectionLocation, HistoryIndex, NodeLocation};
use historypack::{FileSectionHeader, HistoryEntry, HistoryPackVersion};
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
            .entry(Box::from(key.name()))
            .or_insert_with(|| HashMap::new());
        entries.insert(key.clone(), info.clone());
        Ok(())
    }

    /// Closes the mutable historypack, returning the path of the final immutable historypack on disk.
    /// The mutable historypack is no longer usable after being closed.
    pub fn close(self) -> Result<PathBuf> {
        let mut data_file = NamedTempFile::new_in(&self.dir)?;
        let mut hasher = Sha1::new();

        // Write the header
        let version_u8: u8 = self.version.clone().into();
        data_file.write_u8(version_u8)?;
        hasher.input(&[version_u8]);

        // Store data for the index
        let mut file_sections: Vec<(&Box<[u8]>, FileSectionLocation)> = Default::default();
        let mut nodes: HashMap<&Box<[u8]>, HashMap<Key, NodeLocation>> = Default::default();

        // Write the historypack
        let mut section_buf = Vec::new();
        let mut section_offset = data_file.as_ref().metadata()?.len();
        // - In sorted order for deterministic hashes.
        let mut keys = self.mem_index.keys().collect::<Vec<&Box<[u8]>>>();
        keys.sort_unstable();
        for file_name in keys {
            let node_map = self.mem_index.get(file_name).unwrap();
            self.write_section(
                &mut section_buf,
                file_name,
                node_map,
                section_offset as usize,
                &mut nodes,
            )?;
            hasher.input(&section_buf);
            data_file.write_all(&mut section_buf)?;

            let section_location = FileSectionLocation {
                offset: section_offset,
                size: section_buf.len() as u64,
            };
            file_sections.push((file_name, section_location));

            section_offset += section_buf.len() as u64;
            section_buf.clear();
        }

        // Compute the index
        let mut index_file = NamedTempFile::new_in(&self.dir)?;
        HistoryIndex::write(&mut index_file, &file_sections, &nodes)?;

        // Persist the temp files
        let base_filepath = self.dir.join(&hasher.result_str());
        let data_filepath = base_filepath.with_extension("histpack");
        let index_filepath = base_filepath.with_extension("histidx");

        data_file.persist(&data_filepath)?;
        index_file.persist(&index_filepath)?;
        Ok(base_filepath)
    }

    fn write_section<'a>(
        &self,
        writer: &mut Vec<u8>,
        file_name: &'a Box<[u8]>,
        node_map: &HashMap<Key, NodeInfo>,
        section_offset: usize,
        nodes: &mut HashMap<&'a Box<[u8]>, HashMap<Key, NodeLocation>>,
    ) -> Result<()> {
        let mut node_locations = HashMap::<Key, NodeLocation>::with_capacity(node_map.len());

        // Write section header
        FileSectionHeader {
            file_name: &file_name,
            count: node_map.len() as u32,
        }.write(writer)?;

        // TODO: Topo-sort nodes

        // Write nodes
        for (key, node_info) in node_map.iter() {
            let p1 = &node_info.parents[0];
            let copyfrom = if !p1.node().is_null() && p1.name() != key.name() {
                Some(p1.name())
            } else {
                None
            };

            let node_offset = section_offset + writer.len() as usize;
            HistoryEntry::write(
                writer,
                key.node(),
                node_info.parents[0].node(),
                node_info.parents[1].node(),
                &node_info.linknode,
                &copyfrom,
            )?;

            node_locations.insert(
                key.clone(),
                NodeLocation {
                    offset: node_offset as u64,
                },
            );
        }

        nodes.insert(file_name, node_locations);
        Ok(())
    }
}

impl HistoryStore for MutableHistoryPack {
    fn get_ancestors(&self, key: &Key) -> Result<Ancestors> {
        AncestorIterator::new(
            key,
            |k, _seen| self.get_node_info(k),
            AncestorTraversal::Partial,
        ).collect()
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

    use types::node::Node;

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
