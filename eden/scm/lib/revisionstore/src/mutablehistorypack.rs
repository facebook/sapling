/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

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

use crate::error::EmptyMutablePack;
use crate::historyindex::{FileSectionLocation, HistoryIndex, NodeLocation};
use crate::historypack::{FileSectionHeader, HistoryEntry, HistoryPackVersion};
use crate::historystore::{HistoryStore, MutableHistoryStore};
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
        hgid_map: &HashMap<Key, NodeInfo>,
        section_offset: usize,
        nodes: &mut HashMap<&'a RepoPath, HashMap<Key, NodeLocation>>,
    ) -> Fallible<()> {
        let mut hgid_locations = HashMap::<Key, NodeLocation>::with_capacity(hgid_map.len());

        // Write section header
        FileSectionHeader {
            file_name: &file_name,
            count: hgid_map.len() as u32,
        }
        .write(writer)?;

        // Sort the nodes in topological order (ancestors first), as required by the histpack spec
        let hgid_map = topo_sort(hgid_map)?;

        // Write nodes
        for (key, node_info) in hgid_map.iter() {
            let p1 = &node_info.parents[0];
            let copyfrom = if !p1.hgid.is_null() && p1.path != key.path {
                Some(p1.path.as_ref())
            } else {
                None
            };

            let hgid_offset = section_offset + writer.len() as usize;
            HistoryEntry::write(
                writer,
                &key.hgid,
                &node_info.parents[0].hgid,
                &node_info.parents[1].hgid,
                &node_info.linknode,
                &copyfrom,
            )?;

            hgid_locations.insert(
                (*key).clone(),
                NodeLocation {
                    offset: hgid_offset as u64,
                },
            );
        }

        nodes.insert(file_name, hgid_locations);
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
        assert_ne!(key.hgid, info.parents[0].hgid);
        assert_ne!(key.hgid, info.parents[1].hgid);

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

        old_inner.close_pack()
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
            let hgid_map = self.mem_index.get(file_name).unwrap();
            self.write_section(
                &mut section_buf,
                file_name,
                hgid_map,
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

fn topo_sort(hgid_map: &HashMap<Key, NodeInfo>) -> Fallible<Vec<(&Key, &NodeInfo)>> {
    // Sorts the given keys into newest-first topological order
    let mut roots = Vec::<&Key>::new();

    // Child map will be used to perform an oldest-first walk later.
    let mut child_map = HashMap::<&Key, HashSet<&Key>>::with_capacity(hgid_map.len());
    // Parent count will be used to keep track of when all a commit's parents have been processed.
    let mut parent_counts = HashMap::with_capacity(hgid_map.len());

    for (key, info) in hgid_map.iter() {
        let mut parent_count = 0;
        for parent in &info.parents {
            // Only record the relationship if the parent is also in the provided hgid_map.
            // This also filters out null parents.
            if hgid_map.contains_key(parent) {
                let children = child_map.entry(parent).or_default();
                if !children.contains(key) {
                    children.insert(key);
                    parent_count += 1;
                }
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
        results.push((key, hgid_map.get(key).unwrap()));

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

    assert_eq!(results.len(), hgid_map.len());
    Ok(results)
}

impl HistoryStore for MutableHistoryPack {
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

    use types::{hgid::HgId, testutil::key};

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
        let null_key = Key::new(RepoPathBuf::new(), HgId::null_id().clone());

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

                let key = Key::new(RepoPathBuf::new(), HgId::random(&mut rng));
                let info = NodeInfo {
                    parents: [p1, p2],
                    linknode: HgId::random(&mut rng),
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
        assert_eq!(muthistorypack.flush().unwrap(), None);
        drop(muthistorypack);
        assert_eq!(fs::read_dir(tempdir.path()).unwrap().count(), 0);
    }

    quickcheck! {
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
