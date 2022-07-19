/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::io::Cursor;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;

use anyhow::Result;
use byteorder::BigEndian;
use byteorder::ReadBytesExt;
use byteorder::WriteBytesExt;
use configparser::config::ConfigSet;
use configparser::convert::ByteCount;
use indexedlog::log::IndexOutput;
use minibytes::Bytes;
use parking_lot::RwLock;
use sha1::Digest;
use sha1::Sha1;
use types::hgid::ReadHgIdExt;
use types::hgid::WriteHgIdExt;
use types::HgId;
use types::Key;
use types::NodeInfo;
use types::RepoPath;
use types::RepoPathBuf;

use crate::historystore::HgIdHistoryStore;
use crate::historystore::HgIdMutableHistoryStore;
use crate::indexedlogutil::Store;
use crate::indexedlogutil::StoreOpenOptions;
use crate::indexedlogutil::StoreType;
use crate::localstore::LocalStore;
use crate::repack::ToKeys;
use crate::sliceext::SliceExt;
use crate::types::StoreKey;

pub struct IndexedLogHgIdHistoryStore {
    log: RwLock<Store>,
}

struct Entry {
    key: Key,

    p1: HgId,
    p2: HgId,
    linknode: HgId,
    copy_from: Option<RepoPathBuf>,
}

impl Entry {
    pub fn new(key: &Key, info: &NodeInfo) -> Self {
        // Loops in the graph aren't allowed. Since this is a logic error in the code, let's
        // assert.
        assert_ne!(key.hgid, info.parents[0].hgid);
        assert_ne!(key.hgid, info.parents[1].hgid);

        let copy_from = if info.parents[0].path != key.path {
            Some(info.parents[0].path.to_owned())
        } else {
            None
        };

        Entry {
            key: key.clone(),
            p1: info.parents[0].hgid,
            p2: info.parents[1].hgid,
            linknode: info.linknode,
            copy_from,
        }
    }

    fn key_to_index_key(key: &Key) -> Vec<u8> {
        let mut hasher = Sha1::new();
        let path_buf: &[u8] = key.path.as_ref();
        hasher.update(path_buf);
        let buf: [u8; 20] = hasher.finalize().into();

        let mut index_key = Vec::with_capacity(HgId::len() * 2);
        index_key.extend_from_slice(key.hgid.as_ref());
        index_key.extend_from_slice(&buf);

        index_key
    }

    /// Read an entry from the slice and deserialize it.
    ///
    /// The on-disk format of an entry is the following:
    /// - HgId: <20 bytes>
    /// - Sha1(path) <20 bytes>
    /// - Path len: 2 unsigned bytes, big-endian
    /// - Path: <Path len> bytes
    /// - p1 hgid: <20 bytes>
    /// - p2 hgid: <20 bytes>
    /// - linknode: <20 bytes>
    /// Optionally:
    /// - copy from len: 2 unsigned bytes, big-endian
    /// - copy from: <copy from len> bytes
    fn from_slice(bytes: Bytes) -> Result<Self> {
        let data: &[u8] = bytes.as_ref();
        let mut cur = Cursor::new(data);
        let hgid = cur.read_hgid()?;

        // Jump over the hashed path.
        cur.set_position(40);

        let path_len = cur.read_u16::<BigEndian>()? as u64;
        let path_slice =
            data.get_err(cur.position() as usize..(cur.position() + path_len) as usize)?;
        cur.set_position(cur.position() + path_len);
        let path = RepoPath::from_utf8(path_slice)?;

        let key = Key::new(path.to_owned(), hgid);

        let p1 = cur.read_hgid()?;
        let p2 = cur.read_hgid()?;
        let linknode = cur.read_hgid()?;

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
    pub fn from_log(key: &Key, log: &RwLock<Store>) -> Result<Option<Self>> {
        let index_key = Self::key_to_index_key(key);

        let log = log.read();
        let mut log_entry = log.lookup(0, index_key)?;
        let buf = match log_entry.next() {
            None => return Ok(None),
            Some(buf) => buf?,
        };
        let buf = log.slice_to_bytes(buf);
        drop(log);
        Self::from_slice(buf).map(Some)
    }

    /// Write an entry to the `IndexedLog`. See [`from_slice`] for the detail about the on-disk
    /// format.
    pub fn write_to_log(self, log: &RwLock<Store>) -> Result<()> {
        let mut buf = Vec::new();
        buf.write_all(Self::key_to_index_key(&self.key).as_ref())?;
        let path_slice = self.key.path.as_byte_slice();
        buf.write_u16::<BigEndian>(path_slice.len() as u16)?;
        buf.write_all(path_slice)?;
        buf.write_hgid(&self.p1)?;
        buf.write_hgid(&self.p2)?;
        buf.write_hgid(&self.linknode)?;

        if let Some(copy_from) = self.copy_from {
            let copy_from_slice = copy_from.as_byte_slice();
            buf.write_u16::<BigEndian>(copy_from_slice.len() as u16)?;
            buf.write_all(copy_from_slice)?;
        }

        Ok(log.write().append(buf)?)
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

impl IndexedLogHgIdHistoryStore {
    /// Create or open an `IndexedLogHgIdHistoryStore`.
    pub fn new(path: impl AsRef<Path>, config: &ConfigSet, store_type: StoreType) -> Result<Self> {
        let open_options = Self::open_options(config)?;
        let log = match store_type {
            StoreType::Local => open_options.local(&path),
            StoreType::Shared => open_options.shared(&path),
        }?;
        Ok(IndexedLogHgIdHistoryStore {
            log: RwLock::new(log),
        })
    }

    fn open_options(config: &ConfigSet) -> Result<StoreOpenOptions> {
        let mut open_options = StoreOpenOptions::new()
            .max_log_count(4)
            .max_bytes_per_log(500 * 1000 * 1000)
            .auto_sync_threshold(10 * 1024 * 1024)
            .create(true)
            .index("node_and_path", |_| {
                vec![IndexOutput::Reference(0..(HgId::len() * 2) as u64)]
            });

        if let Some(max_bytes_per_log) =
            config.get_opt::<ByteCount>("indexedlog", "history.max-bytes-per-log")?
        {
            open_options = open_options.max_bytes_per_log(max_bytes_per_log.value());
        }
        if let Some(max_log_count) = config.get_opt::<u8>("indexedlog", "history.max-log-count")? {
            open_options = open_options.max_log_count(max_log_count);
        }
        Ok(open_options)
    }

    pub fn repair(path: PathBuf, config: &ConfigSet, store_type: StoreType) -> Result<String> {
        match store_type {
            StoreType::Local => {
                IndexedLogHgIdHistoryStore::open_options(config)?.repair_local(path)
            }
            StoreType::Shared => {
                IndexedLogHgIdHistoryStore::open_options(config)?.repair_shared(path)
            }
        }
    }
}

impl LocalStore for IndexedLogHgIdHistoryStore {
    fn get_missing(&self, keys: &[StoreKey]) -> Result<Vec<StoreKey>> {
        Ok(keys
            .iter()
            .filter(|k| match k {
                StoreKey::HgId(k) => match Entry::from_log(k, &self.log) {
                    Ok(None) | Err(_) => true,
                    Ok(Some(_)) => false,
                },
                StoreKey::Content(_, _) => true,
            })
            .cloned()
            .collect())
    }
}

impl HgIdHistoryStore for IndexedLogHgIdHistoryStore {
    fn get_node_info(&self, key: &Key) -> Result<Option<NodeInfo>> {
        let entry = match Entry::from_log(key, &self.log)? {
            None => return Ok(None),
            Some(entry) => entry,
        };
        Ok(Some(entry.node_info()))
    }

    fn refresh(&self) -> Result<()> {
        Ok(())
    }
}

impl HgIdMutableHistoryStore for IndexedLogHgIdHistoryStore {
    fn add(&self, key: &Key, info: &NodeInfo) -> Result<()> {
        let entry = Entry::new(key, info);
        entry.write_to_log(&self.log)
    }

    fn flush(&self) -> Result<Option<Vec<PathBuf>>> {
        self.log.write().flush()?;
        Ok(None)
    }
}

impl ToKeys for IndexedLogHgIdHistoryStore {
    fn to_keys(&self) -> Vec<Result<Key>> {
        let log = &self.log.read();
        log.iter()
            .map(|entry| {
                let bytes = log.slice_to_bytes(entry?);
                Entry::from_slice(bytes)
            })
            .map(|entry| Ok(entry?.key))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use std::fs::remove_file;

    use rand::SeedableRng;
    use rand_chacha::ChaChaRng;
    use tempfile::TempDir;
    use types::testutil::*;

    use super::*;
    use crate::historypack::tests::get_nodes;

    #[test]
    fn test_empty() -> Result<()> {
        let tempdir = TempDir::new()?;
        let log = IndexedLogHgIdHistoryStore::new(&tempdir, &ConfigSet::new(), StoreType::Shared)?;
        log.flush()?;
        Ok(())
    }

    #[test]
    fn test_add() -> Result<()> {
        let tempdir = TempDir::new()?;
        let log = IndexedLogHgIdHistoryStore::new(&tempdir, &ConfigSet::new(), StoreType::Shared)?;
        let k = key("a", "1");
        let nodeinfo = NodeInfo {
            parents: [key("a", "2"), null_key("a")],
            linknode: hgid("3"),
        };

        log.add(&k, &nodeinfo)?;
        log.flush()?;
        Ok(())
    }

    #[test]
    fn test_add_get_node_info() -> Result<()> {
        let tempdir = TempDir::new()?;
        let log = IndexedLogHgIdHistoryStore::new(&tempdir, &ConfigSet::new(), StoreType::Shared)?;
        let k = key("a", "1");
        let nodeinfo = NodeInfo {
            parents: [key("a", "2"), null_key("a")],
            linknode: hgid("3"),
        };
        log.add(&k, &nodeinfo)?;
        log.flush()?;

        let log = IndexedLogHgIdHistoryStore::new(&tempdir, &ConfigSet::new(), StoreType::Shared)?;
        let read_nodeinfo = log.get_node_info(&k)?;
        assert_eq!(Some(nodeinfo), read_nodeinfo);
        Ok(())
    }

    #[test]
    fn test_corrupted() -> Result<()> {
        let tempdir = TempDir::new()?;
        let log = IndexedLogHgIdHistoryStore::new(&tempdir, &ConfigSet::new(), StoreType::Shared)?;
        let mut rng = ChaChaRng::from_seed([0u8; 32]);

        let nodes = get_nodes(&mut rng);
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

        let log = IndexedLogHgIdHistoryStore::new(&tempdir, &ConfigSet::new(), StoreType::Shared)?;
        for (key, info) in nodes.iter() {
            log.add(&key, &info)?;
        }
        log.flush()?;

        assert_eq!(log.to_keys().len(), nodes.iter().count());
        Ok(())
    }

    #[test]
    fn test_iter() -> Result<()> {
        let tempdir = TempDir::new()?;
        let log = IndexedLogHgIdHistoryStore::new(&tempdir, &ConfigSet::new(), StoreType::Shared)?;
        let k = key("a", "1");
        let nodeinfo = NodeInfo {
            parents: [key("a", "2"), null_key("a")],
            linknode: hgid("3"),
        };
        log.add(&k, &nodeinfo)?;

        assert!(log.to_keys().into_iter().all(|e| e.unwrap() == k));
        Ok(())
    }
}
