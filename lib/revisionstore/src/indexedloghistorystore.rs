// Copyright 2019 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::{
    io::{Cursor, Write},
    path::{Path, PathBuf},
    sync::{Arc, RwLock},
};

use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};
use crypto::{digest::Digest, sha1::Sha1};
use failure::Fallible;

use indexedlog::{
    log::IndexOutput,
    rotate::{OpenOptions, RotateLog},
};
use types::{
    node::{ReadNodeExt, WriteNodeExt},
    Key, Node, NodeInfo, RepoPath, RepoPathBuf,
};

use crate::{
    ancestors::{AncestorIterator, AncestorTraversal},
    historystore::{Ancestors, HistoryStore, MutableHistoryStore},
    localstore::LocalStore,
    repack::ToKeys,
    sliceext::SliceExt,
};

struct IndexedLogHistoryStoreInner {
    log: RotateLog,
}

pub struct IndexedLogHistoryStore {
    inner: Arc<RwLock<IndexedLogHistoryStoreInner>>,
}

struct Entry {
    key: Key,

    p1: Node,
    p2: Node,
    linknode: Node,
    copy_from: Option<RepoPathBuf>,
}

impl Entry {
    pub fn new(key: &Key, info: &NodeInfo) -> Self {
        // Loops in the graph aren't allowed. Since this is a logic error in the code, let's
        // assert.
        assert_ne!(key.node, info.parents[0].node);
        assert_ne!(key.node, info.parents[1].node);

        let copy_from = if info.parents[0].path != key.path {
            Some(info.parents[0].path.to_owned())
        } else {
            None
        };

        Entry {
            key: key.clone(),
            p1: info.parents[0].node,
            p2: info.parents[1].node,
            linknode: info.linknode,
            copy_from,
        }
    }

    fn key_to_index_key(key: &Key) -> Vec<u8> {
        let mut hasher = Sha1::new();
        hasher.input(key.path.as_ref());
        let mut buf: [u8; 20] = Default::default();
        hasher.result(&mut buf);

        let mut index_key = Vec::with_capacity(Node::len() * 2);
        index_key.extend_from_slice(key.node.as_ref());
        index_key.extend_from_slice(&buf);

        index_key
    }

    /// Read an entry from the slice and deserialize it.
    ///
    /// The on-disk format of an entry is the following:
    /// - Node: <20 bytes>
    /// - Sha1(path) <20 bytes>
    /// - Path len: 2 unsigned bytes, big-endian
    /// - Path: <Path len> bytes
    /// - p1 node: <20 bytes>
    /// - p2 node: <20 bytes>
    /// - linknode: <20 bytes>
    /// Optionally:
    /// - copy from len: 2 unsigned bytes, big-endian
    /// - copy from: <copy from len> bytes
    fn from_slice(data: &[u8]) -> Fallible<Self> {
        let mut cur = Cursor::new(data);
        let node = cur.read_node()?;

        // Jump over the hashed path.
        cur.set_position(40);

        let path_len = cur.read_u16::<BigEndian>()? as u64;
        let path_slice =
            data.get_err(cur.position() as usize..(cur.position() + path_len) as usize)?;
        cur.set_position(cur.position() + path_len);
        let path = RepoPath::from_utf8(path_slice)?;

        let key = Key::new(path.to_owned(), node);

        let p1 = cur.read_node()?;
        let p2 = cur.read_node()?;
        let linknode = cur.read_node()?;

        let copy_from = if let Ok(copy_from_len) = cur.read_u16::<BigEndian>() {
            let copy_from_slice = data.get_err(
                cur.position() as usize..(cur.position() + copy_from_len as u64) as usize,
            )?;
            Some(RepoPath::from_utf8(copy_from_slice)?.to_owned())
        } else {
            None
        };

        Ok(Entry {
            key,
            p1,
            p2,
            linknode,
            copy_from,
        })
    }

    /// Read an entry from the `IndexedLog` and deserialize it.
    pub fn from_log(key: &Key, log: &RotateLog) -> Fallible<Option<Self>> {
        let index_key = Self::key_to_index_key(key);
        let mut log_entry = log.lookup(0, index_key)?;
        let buf = match log_entry.nth(0) {
            None => return Ok(None),
            Some(buf) => buf?,
        };

        Self::from_slice(buf).map(Some)
    }

    /// Write an entry to the `IndexedLog`. See [`from_slice`] for the detail about the on-disk
    /// format.
    pub fn write_to_log(self, log: &mut RotateLog) -> Fallible<()> {
        let mut buf = Vec::new();
        buf.write_all(Self::key_to_index_key(&self.key).as_ref())?;
        let path_slice = self.key.path.as_byte_slice();
        buf.write_u16::<BigEndian>(path_slice.len() as u16)?;
        buf.write_all(path_slice)?;
        buf.write_node(&self.p1)?;
        buf.write_node(&self.p2)?;
        buf.write_node(&self.linknode)?;

        if let Some(copy_from) = self.copy_from {
            let copy_from_slice = copy_from.as_byte_slice();
            buf.write_u16::<BigEndian>(copy_from_slice.len() as u16)?;
            buf.write_all(copy_from_slice)?;
        }

        Ok(log.append(buf)?)
    }

    pub fn node_info(&self) -> NodeInfo {
        let p1path = if let Some(copy_from) = &self.copy_from {
            copy_from.clone()
        } else {
            self.key.path.clone()
        };

        NodeInfo {
            parents: [
                Key::new(p1path, self.p1),
                Key::new(self.key.path.clone(), self.p2),
            ],
            linknode: self.linknode,
        }
    }
}

impl IndexedLogHistoryStore {
    /// Create or open an `IndexedLogHistoryStore`.
    pub fn new(path: impl AsRef<Path>) -> Fallible<Self> {
        let open_options = Self::default_open_options();
        let log = open_options.open(&path)?;
        Ok(IndexedLogHistoryStore {
            inner: Arc::new(RwLock::new(IndexedLogHistoryStoreInner { log })),
        })
    }

    /// Attempt to repair data at the given path.
    /// Return human-readable repair logs.
    pub fn repair(path: impl AsRef<Path>) -> Fallible<String> {
        let path = path.as_ref();
        let open_options = Self::default_open_options();
        Ok(open_options.repair(path)?)
    }

    /// Default configuration: 4 x 0.5GB.
    fn default_open_options() -> OpenOptions {
        OpenOptions::new()
            .max_log_count(4)
            .max_bytes_per_log(500 * 1000 * 1000)
            .create(true)
            .index("node_and_path", |_| {
                vec![IndexOutput::Reference(0..(Node::len() * 2) as u64)]
            })
    }
}

impl LocalStore for IndexedLogHistoryStore {
    fn from_path(path: &Path) -> Fallible<Self> {
        IndexedLogHistoryStore::new(path)
    }

    fn get_missing(&self, keys: &[Key]) -> Fallible<Vec<Key>> {
        let inner = self.inner.read().unwrap();
        Ok(keys
            .iter()
            .filter(|k| match Entry::from_log(k, &inner.log) {
                Ok(None) | Err(_) => true,
                Ok(Some(_)) => false,
            })
            .map(|k| k.clone())
            .collect())
    }
}

impl HistoryStore for IndexedLogHistoryStore {
    fn get_ancestors(&self, key: &Key) -> Fallible<Option<Ancestors>> {
        AncestorIterator::new(
            key,
            |k, _seen| self.get_node_info(k),
            AncestorTraversal::Partial,
        )
        .collect()
    }

    fn get_node_info(&self, key: &Key) -> Fallible<Option<NodeInfo>> {
        let inner = self.inner.read().unwrap();
        let entry = match Entry::from_log(key, &inner.log)? {
            None => return Ok(None),
            Some(entry) => entry,
        };
        Ok(Some(entry.node_info()))
    }
}

impl MutableHistoryStore for IndexedLogHistoryStore {
    fn add(&self, key: &Key, info: &NodeInfo) -> Fallible<()> {
        let mut inner = self.inner.write().unwrap();
        let entry = Entry::new(key, info);
        entry.write_to_log(&mut inner.log)
    }

    fn flush(&self) -> Fallible<Option<PathBuf>> {
        self.inner.write().unwrap().log.flush()?;
        Ok(None)
    }
}

impl ToKeys for IndexedLogHistoryStore {
    fn to_keys(&self) -> Vec<Fallible<Key>> {
        self.inner
            .read()
            .unwrap()
            .log
            .iter()
            .map(|entry| Entry::from_slice(entry?))
            .map(|entry| Ok(entry?.key))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::fs::remove_file;

    use rand::SeedableRng;
    use rand_chacha::ChaChaRng;
    use tempfile::TempDir;

    use types::testutil::*;

    use crate::historypack::tests::get_nodes;

    #[test]
    fn test_empty() -> Fallible<()> {
        let tempdir = TempDir::new()?;
        let log = IndexedLogHistoryStore::new(&tempdir)?;
        log.flush()?;
        Ok(())
    }

    #[test]
    fn test_add() -> Fallible<()> {
        let tempdir = TempDir::new()?;
        let log = IndexedLogHistoryStore::new(&tempdir)?;
        let k = key("a", "1");
        let nodeinfo = NodeInfo {
            parents: [key("a", "2"), null_key("a")],
            linknode: node("3"),
        };

        log.add(&k, &nodeinfo)?;
        log.flush()?;
        Ok(())
    }

    #[test]
    fn test_add_get_node_info() -> Fallible<()> {
        let tempdir = TempDir::new()?;
        let log = IndexedLogHistoryStore::new(&tempdir)?;
        let k = key("a", "1");
        let nodeinfo = NodeInfo {
            parents: [key("a", "2"), null_key("a")],
            linknode: node("3"),
        };
        log.add(&k, &nodeinfo)?;
        log.flush()?;

        let log = IndexedLogHistoryStore::new(&tempdir)?;
        let read_nodeinfo = log.get_node_info(&k)?;
        assert_eq!(Some(nodeinfo), read_nodeinfo);
        Ok(())
    }

    #[test]
    fn test_add_get_ancestors() -> Fallible<()> {
        let tempdir = TempDir::new()?;
        let log = IndexedLogHistoryStore::new(&tempdir)?;
        let mut rng = ChaChaRng::from_seed([0u8; 32]);

        let (nodes, ancestors) = get_nodes(&mut rng);
        for (key, info) in nodes.iter() {
            log.add(&key, &info)?;
        }

        for (key, _) in nodes.iter() {
            log.get_node_info(&key)?;
            let response = log.get_ancestors(&key)?;
            assert_eq!(response.as_ref(), ancestors.get(&key));
        }
        Ok(())
    }

    #[test]
    fn test_corrupted() -> Fallible<()> {
        let tempdir = TempDir::new()?;
        let log = IndexedLogHistoryStore::new(&tempdir)?;
        let mut rng = ChaChaRng::from_seed([0u8; 32]);

        let (nodes, _) = get_nodes(&mut rng);
        for (key, info) in nodes.iter() {
            log.add(&key, &info)?;
        }
        log.flush()?;
        drop(log);

        // Corrupt the log by removing the "log" file.
        let mut rotate_log_path = tempdir.path().to_path_buf();
        rotate_log_path.push("0");
        rotate_log_path.push("log");
        remove_file(rotate_log_path)?;

        let log = IndexedLogHistoryStore::new(&tempdir)?;
        for (key, info) in nodes.iter() {
            log.add(&key, &info)?;
        }
        log.flush()?;

        assert_eq!(log.to_keys().len(), nodes.iter().count());
        Ok(())
    }

    #[test]
    fn test_iter() -> Fallible<()> {
        let tempdir = TempDir::new()?;
        let log = IndexedLogHistoryStore::new(&tempdir)?;
        let k = key("a", "1");
        let nodeinfo = NodeInfo {
            parents: [key("a", "2"), null_key("a")],
            linknode: node("3"),
        };
        log.add(&k, &nodeinfo)?;

        assert!(log.to_keys().into_iter().all(|e| e.unwrap() == k));
        Ok(())
    }
}
