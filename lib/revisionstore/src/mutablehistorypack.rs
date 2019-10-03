// Copyright 2018 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::{
    collections::{HashMap, HashSet, VecDeque},
    io::Write,
    iter::FromIterator,
    mem::replace,
    path::{Path, PathBuf},
    sync::Arc,
};

use byteorder::WriteBytesExt;
use crypto::digest::Digest;
use crypto::sha1::Sha1;
use failure::{Fail, Fallible};
use parking_lot::Mutex;
use tempfile::NamedTempFile;

use types::{Key, NodeInfo, RepoPath, RepoPathBuf};

use crate::ancestors::{AncestorIterator, AncestorTraversal};
use crate::error::EmptyMutablePack;
use crate::historyindex::{FileSectionLocation, HistoryIndex, NodeLocation};
use crate::historypack::{FileSectionHeader, HistoryEntry, HistoryPackVersion};
use crate::historystore::{Ancestors, HistoryStore, MutableHistoryStore};
use crate::localstore::LocalStore;
use crate::mutablepack::MutablePack;
use crate::packwriter::PackWriter;

#[derive(Debug, Fail)]
#[fail(display = "Mutable History Pack Error: {:?}", _0)]
struct MutableHistoryPackError(String);

struct MutableHistoryPackInner {
    version: HistoryPackVersion,
    dir: PathBuf,
    mem_index: HashMap<RepoPathBuf, HashMap<Key, NodeInfo>>,
}

#[derive(Clone)]
pub struct MutableHistoryPack {
    inner: Arc<Mutex<MutableHistoryPackInner>>,
}

impl MutableHistoryPackInner {
    pub fn new(dir: impl AsRef<Path>, version: HistoryPackVersion) -> Fallible<Self> {
        let dir = dir.as_ref();
        if !dir.is_dir() {
            return Err(MutableHistoryPackError(format!(
                "cannot create mutable historypack in non-directory '{:?}'",
                dir
            ))
            .into());
        }

        Ok(Self {
            version,
            dir: dir.to_path_buf(),
            mem_index: HashMap::new(),
        })
    }

    fn write_section<'a>(
        &self,
        writer: &mut Vec<u8>,
        file_name: &'a RepoPath,
        node_map: &HashMap<Key, NodeInfo>,
        section_offset: usize,
        nodes: &mut HashMap<&'a RepoPath, HashMap<Key, NodeLocation>>,
    ) -> Fallible<()> {
        let mut node_locations = HashMap::<Key, NodeLocation>::with_capacity(node_map.len());

        // Write section header
        FileSectionHeader {
            file_name: &file_name,
            count: node_map.len() as u32,
        }
        .write(writer)?;

        // Sort the nodes in topological order (ancestors first), as required by the histpack spec
        let node_map = topo_sort(node_map)?;

        // Write nodes
        for (key, node_info) in node_map.iter() {
            let p1 = &node_info.parents[0];
            let copyfrom = if !p1.node.is_null() && p1.path != key.path {
                Some(p1.path.as_ref())
            } else {
                None
            };

            let node_offset = section_offset + writer.len() as usize;
            HistoryEntry::write(
                writer,
                &key.node,
                &node_info.parents[0].node,
                &node_info.parents[1].node,
                &node_info.linknode,
                &copyfrom,
            )?;

            node_locations.insert(
                (*key).clone(),
                NodeLocation {
                    offset: node_offset as u64,
                },
            );
        }

        nodes.insert(file_name, node_locations);
        Ok(())
    }
}

impl MutableHistoryPack {
    pub fn new(dir: impl AsRef<Path>, version: HistoryPackVersion) -> Fallible<Self> {
        Ok(Self {
            inner: Arc::new(Mutex::new(MutableHistoryPackInner::new(dir, version)?)),
        })
    }
}

impl MutableHistoryStore for MutableHistoryPack {
    fn add(&self, key: &Key, info: &NodeInfo) -> Fallible<()> {
        let mut inner = self.inner.lock();
        // Loops in the graph aren't allowed. Since this is a logic error in the code, let's
        // assert.
        assert_ne!(key.node, info.parents[0].node);
        assert_ne!(key.node, info.parents[1].node);

        // Ideally we could use something like:
        //     self.mem_index.entry(key.name()).or_insert_with(|| HashMap::new())
        // To get the inner map, then insert our new NodeInfo. Unfortunately it requires
        // key.name().clone() though. So we have to do it the long way to avoid the allocation.
        let entries = inner
            .mem_index
            .entry(key.path.clone())
            .or_insert_with(|| HashMap::new());
        entries.insert(key.clone(), info.clone());
        Ok(())
    }

    fn flush(&self) -> Fallible<Option<PathBuf>> {
        let mut guard = self.inner.lock();
        let new_inner = MutableHistoryPackInner::new(&guard.dir, HistoryPackVersion::One)?;
        let old_inner = replace(&mut *guard, new_inner);

        let path = old_inner.close_pack()?;
        Ok(Some(path))
    }
}

impl MutablePack for MutableHistoryPackInner {
    fn build_files(self) -> Fallible<(NamedTempFile, NamedTempFile, PathBuf)> {
        if self.mem_index.is_empty() {
            return Err(EmptyMutablePack().into());
        }

        let mut data_file = PackWriter::new(NamedTempFile::new_in(&self.dir)?);
        let mut hasher = Sha1::new();

        // Write the header
        let version_u8: u8 = self.version.clone().into();
        data_file.write_u8(version_u8)?;
        hasher.input(&[version_u8]);

        // Store data for the index
        let mut file_sections: Vec<(&RepoPath, FileSectionLocation)> = Default::default();
        let mut nodes: HashMap<&RepoPath, HashMap<Key, NodeLocation>> = Default::default();

        // Write the historypack
        let mut section_buf = Vec::new();
        let mut section_offset = data_file.bytes_written();
        // - In sorted order for deterministic hashes.
        let mut keys = self.mem_index.keys().collect::<Vec<_>>();
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
        let mut index_file = PackWriter::new(NamedTempFile::new_in(&self.dir)?);
        HistoryIndex::write(&mut index_file, &file_sections, &nodes)?;

        Ok((
            data_file.into_inner()?,
            index_file.into_inner()?,
            self.dir.join(hasher.result_str()),
        ))
    }

    fn extension(&self) -> &'static str {
        "hist"
    }
}

impl MutablePack for MutableHistoryPack {
    fn build_files(self) -> Fallible<(NamedTempFile, NamedTempFile, PathBuf)> {
        let mut guard = self.inner.lock();
        let new_inner = MutableHistoryPackInner::new(&guard.dir, HistoryPackVersion::One)?;
        let old_inner = replace(&mut *guard, new_inner);

        old_inner.build_files()
    }

    fn extension(&self) -> &'static str {
        "hist"
    }
}

fn topo_sort(node_map: &HashMap<Key, NodeInfo>) -> Fallible<Vec<(&Key, &NodeInfo)>> {
    // Sorts the given keys into newest-first topological order
    let mut roots = Vec::<&Key>::new();

    // Child map will be used to perform an oldest-first walk later.
    let mut child_map = HashMap::<&Key, HashSet<&Key>>::with_capacity(node_map.len());
    // Parent count will be used to keep track of when all a commit's parents have been processed.
    let mut parent_counts = HashMap::with_capacity(node_map.len());

    for (key, info) in node_map.iter() {
        let mut parent_count = 0;
        for i in 0..2 {
            let parent = &info.parents[i];

            // Only record the relationship if the parent is also in the provided node_map.
            // This also filters out null parents.
            if node_map.contains_key(parent) {
                parent_count += 1;
                let children = child_map.entry(parent).or_default();
                children.insert(key);
            }
        }

        if parent_count == 0 {
            roots.push(key);
        } else {
            parent_counts.insert(key, parent_count);
        }
    }

    // Sort the roots so things are deterministic.
    roots.sort_unstable();

    // Process roots, adding children to the queue once all their parents are processed.
    let mut pending = VecDeque::<&Key>::from_iter(roots.iter().cloned());
    let mut results = Vec::new();
    while let Some(key) = pending.pop_front() {
        results.push((key, node_map.get(key).unwrap()));

        if let Some(children) = child_map.get(key) {
            for child in children.iter() {
                let mut parent_count = parent_counts
                    .get(child)
                    .ok_or_else(|| {
                        MutableHistoryPackError(format!("missing {:?} during topo sort", child))
                    })?
                    .clone();
                parent_count -= 1;
                parent_counts.insert(child, parent_count);
                if parent_count == 0 {
                    // If a child has no more parents, its a root and is ready for processing.
                    // Put it at the front so ancestor chains are processed contiguously.
                    pending.push_front(child);
                }
            }
        }
    }

    // We built the result in oldest first order, but we need it in newest first order.
    results.reverse();

    assert_eq!(results.len(), node_map.len());
    Ok(results)
}

impl HistoryStore for MutableHistoryPack {
    fn get_ancestors(&self, key: &Key) -> Fallible<Option<Ancestors>> {
        AncestorIterator::new(
            key,
            |k, _seen| self.get_node_info(k),
            AncestorTraversal::Partial,
        )
        .collect()
    }

    fn get_node_info(&self, key: &Key) -> Fallible<Option<NodeInfo>> {
        let inner = self.inner.lock();
        Ok(inner
            .mem_index
            .get(&key.path)
            .and_then(|nodes| nodes.get(key))
            .cloned())
    }
}

impl LocalStore for MutableHistoryPack {
    fn get_missing(&self, keys: &[Key]) -> Fallible<Vec<Key>> {
        let inner = self.inner.lock();
        Ok(keys
            .iter()
            .filter(|k| match inner.mem_index.get(&k.path) {
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

    use std::fs;

    use quickcheck::quickcheck;
    use rand::seq::SliceRandom;
    use rand::SeedableRng;
    use rand_chacha::ChaChaRng;
    use tempfile::tempdir;

    use types::{node::Node, testutil::key};

    use crate::historypack::HistoryPack;
    use crate::repack::ToKeys;

    #[test]
    fn test_topo_order() {
        // Tests for exponential time complexity in a merge ancestory. This doesn't won't fail,
        // but may take a long time if there is bad time complexity.
        let mut rng = ChaChaRng::from_seed([0u8; 32]);
        let tempdir = tempdir().unwrap();
        let muthistorypack =
            MutableHistoryPack::new(tempdir.path(), HistoryPackVersion::One).unwrap();
        let null_key = Key::new(RepoPathBuf::new(), Node::null_id().clone());

        let chain_count = 2;
        let chain_len = 3;

        let mut chains = HashMap::<Key, Vec<(Key, NodeInfo)>>::new();
        let mut entries = Vec::<(Key, NodeInfo)>::new();
        for _ in 0..chain_count {
            let mut chain = Vec::<(Key, NodeInfo)>::new();
            for i in 0..chain_len {
                let p1 = if i > 0 {
                    chain[i - 1].0.clone()
                } else {
                    null_key.clone()
                };
                let p2 = if i > 1 {
                    chain[i - 2].0.clone()
                } else {
                    null_key.clone()
                };

                let key = Key::new(RepoPathBuf::new(), Node::random(&mut rng));
                let info = NodeInfo {
                    parents: [p1, p2],
                    linknode: Node::random(&mut rng),
                };
                entries.push((key.clone(), info.clone()));
                chain.push((key.clone(), info.clone()));
                if i == chain_len - 1 {
                    // Reverse it so the newest key is first.
                    chain.reverse();
                    chains.insert(key, chain.clone());
                }
            }
        }

        // Add them in random order, so we can verify they get sorted correctly
        entries.shuffle(&mut rng);
        for (key, info) in entries.iter() {
            muthistorypack.add(&key, &info).unwrap();
        }
        let path = muthistorypack.flush().unwrap().unwrap();
        let pack = HistoryPack::new(&path).unwrap();

        let actual_order = pack
            .to_keys()
            .into_iter()
            .collect::<Fallible<Vec<Key>>>()
            .unwrap();

        // Compute the expected order
        let mut chains = chains.iter().collect::<Vec<_>>();
        chains.sort_unstable();
        chains.reverse();
        let mut expected_order = vec![];
        for (_, chain) in chains.iter() {
            for (key, _) in chain.iter() {
                expected_order.push(key.clone());
            }
        }

        assert_eq!(actual_order, expected_order);
    }

    #[test]
    #[should_panic]
    fn test_loop() {
        let tempdir = tempdir().unwrap();
        let muthistorypack =
            MutableHistoryPack::new(tempdir.path(), HistoryPackVersion::One).unwrap();

        let k = key("a", "1");
        let nodeinfo = NodeInfo {
            parents: [k.clone(), k.clone()],
            linknode: Default::default(),
        };

        muthistorypack.add(&k, &nodeinfo).unwrap();
    }

    #[test]
    fn test_empty() {
        let tempdir = tempdir().unwrap();
        let muthistorypack =
            MutableHistoryPack::new(tempdir.path(), HistoryPackVersion::One).unwrap();
        muthistorypack.flush().unwrap();
        drop(muthistorypack);
        assert_eq!(fs::read_dir(tempdir.path()).unwrap().count(), 0);
    }

    quickcheck! {
        fn test_get_ancestors(keys: Vec<(Key, bool)>) -> bool {
            let mut rng = ChaChaRng::from_seed([0u8; 32]);
            let tempdir = tempdir().unwrap();
            let muthistorypack =
                MutableHistoryPack::new(tempdir.path(), HistoryPackVersion::One).unwrap();

            // Insert all the keys, randomly choosing nodes from the already inserted keys
            let mut chains = HashMap::<Key, Ancestors>::new();
            chains.insert(Key::default(), Ancestors::new());
            for &(ref key, ref has_p2) in keys.iter() {
                let mut p1 = Key::default();
                let mut p2 = Key::default();
                let available_parents = chains.keys().map(|k| k.clone()).collect::<Vec<Key>>();

                if !chains.is_empty() {
                    p1 = available_parents.choose(&mut rng)
                        .expect("choose p1")
                        .clone();

                    if *has_p2 {
                        p2 = available_parents.choose(&mut rng)
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
                let in_pack = muthistorypack.get_ancestors(&key).expect("get ancestors").unwrap();
                if in_pack != chains[&key] {
                    return false;
                }
            }

            true
        }

        fn test_get_node_info(insert: HashMap<Key, NodeInfo>, notinsert: Vec<Key>) -> bool {
            let tempdir = tempdir().unwrap();
            let muthistorypack =
                MutableHistoryPack::new(tempdir.path(), HistoryPackVersion::One).unwrap();

            for (key, info) in insert.iter() {
                muthistorypack.add(&key, &info).unwrap();
            }

            for (key, info) in insert.iter() {
                if *info != muthistorypack.get_node_info(key).unwrap().unwrap() {
                    return false;
                }
            }

            for key in notinsert.iter() {
                if muthistorypack.get_node_info(key).unwrap().is_some() {
                    return false;
                }
            }

            true
        }

        fn test_get_missing(insert: HashMap<Key, NodeInfo>, notinsert: Vec<Key>) -> bool {
            let tempdir = tempdir().unwrap();
            let muthistorypack =
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
