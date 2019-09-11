// Copyright 2019 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::{
    fs::remove_dir_all,
    io::{Cursor, Write},
    path::{Path, PathBuf},
    sync::{Arc, RwLock},
};

use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};
use bytes::Bytes;
use failure::{bail, ensure, format_err, Fallible};

use indexedlog::{
    log::IndexOutput,
    rotate::{OpenOptions, RotateLog},
};
use lz4_pyframe::{compress, decompress};
use types::{node::ReadNodeExt, Key, Node, RepoPath};

use crate::{
    datastore::{DataStore, Delta, Metadata, MutableDeltaStore},
    error::KeyError,
    localstore::LocalStore,
    repack::ToKeys,
    sliceext::SliceExt,
};

struct IndexedLogDataStoreInner {
    log: RotateLog,
}

pub struct IndexedLogDataStore {
    inner: Arc<RwLock<IndexedLogDataStoreInner>>,
}

struct Entry {
    key: Key,
    metadata: Metadata,

    content: Option<Bytes>,
    compressed_content: Option<Bytes>,
}

impl Entry {
    pub fn new(key: Key, content: Bytes, metadata: Metadata) -> Self {
        Entry {
            key,
            content: Some(content),
            metadata,
            compressed_content: None,
        }
    }

    /// Read an entry from the slice and deserialize it.
    ///
    /// The on-disk format of an entry is the following:
    /// - Node <20 bytes>
    /// - Path len: 2 unsigned bytes, big-endian
    /// - Path: <Path len> bytes
    /// - Metadata: metadata-list
    /// - Content len: 8 unsigned bytes, big-endian
    /// - Content: <Content len> bytes, lz4 compressed
    ///
    /// The metadata-list is a list of Metadata, encode with:
    /// - Flag: 1 byte,
    /// - Len: 2 unsigned bytes, big-endian
    /// - Value: <Len> bytes, big-endian
    fn from_slice(data: &[u8]) -> Fallible<Self> {
        let mut cur = Cursor::new(data);
        let node = cur.read_node()?;

        let name_len = cur.read_u16::<BigEndian>()? as u64;
        let name_slice =
            data.get_err(cur.position() as usize..(cur.position() + name_len) as usize)?;
        cur.set_position(cur.position() + name_len);
        let filename = RepoPath::from_utf8(name_slice)?;

        let key = Key::new(filename.to_owned(), node);

        let metadata = Metadata::read(&mut cur)?;

        let compressed_len = cur.read_u64::<BigEndian>()?;
        let compressed =
            data.get_err(cur.position() as usize..(cur.position() + compressed_len) as usize)?;

        Ok(Entry {
            key,
            content: None,
            compressed_content: Some(compressed.into()),
            metadata,
        })
    }

    /// Read an entry from the IndexedLog and deserialize it.
    pub fn from_log(key: &Key, log: &RotateLog) -> Fallible<Self> {
        let mut log_entry = log.lookup(0, key.node.as_ref())?;
        let buf = log_entry
            .nth(0)
            .ok_or_else(|| KeyError::new(format_err!("Key {} not found", key)))??;

        Entry::from_slice(buf)
    }

    /// Write an entry to the IndexedLog. See [`from_log`] for the detail about the on-disk format.
    pub fn write_to_log(self, log: &mut RotateLog) -> Fallible<()> {
        let mut buf = Vec::new();
        buf.write_all(self.key.node.as_ref())?;
        let path_slice = self.key.path.as_byte_slice();
        buf.write_u16::<BigEndian>(path_slice.len() as u16)?;
        buf.write_all(path_slice)?;
        self.metadata.write(&mut buf)?;

        let compressed = if let Some(compressed) = self.compressed_content {
            compressed
        } else {
            if let Some(raw) = self.content {
                compress(&raw)?.into()
            } else {
                bail!("No content");
            }
        };

        buf.write_u64::<BigEndian>(compressed.len() as u64)?;
        buf.write_all(&compressed)?;

        Ok(log.append(buf)?)
    }

    pub fn content(&mut self) -> Fallible<Bytes> {
        if let Some(content) = self.content.as_ref() {
            return Ok(content.clone());
        }

        if let Some(compressed) = self.compressed_content.as_ref() {
            let raw = Bytes::from(decompress(&compressed)?);
            self.content = Some(raw.clone());
            Ok(raw)
        } else {
            bail!("No content");
        }
    }

    pub fn metadata(&self) -> &Metadata {
        &self.metadata
    }
}

impl IndexedLogDataStore {
    /// Create or open an `IndexedLogDataStore`.
    ///
    /// It is configured to use 10 logs of 1GB each. On data corruption, the entire
    /// `IndexedLogDataStore` is being recreated, losing all data that was previously stored in
    /// it.
    pub fn new(path: impl AsRef<Path>) -> Fallible<Self> {
        let open_options = OpenOptions::new()
            .max_log_count(10)
            .max_bytes_per_log(1 * 1024 * 1024 * 1024)
            .create(true)
            .index("node", |_| {
                vec![IndexOutput::Reference(0..Node::len() as u64)]
            });

        let log = match open_options.clone().open(&path) {
            Ok(log) => log,
            Err(_) => {
                remove_dir_all(&path)?;
                open_options.open(&path)?
            }
        };
        Ok(IndexedLogDataStore {
            inner: Arc::new(RwLock::new(IndexedLogDataStoreInner { log })),
        })
    }
}

impl MutableDeltaStore for IndexedLogDataStore {
    fn add(&mut self, delta: &Delta, metadata: &Metadata) -> Fallible<()> {
        ensure!(delta.base.is_none(), "Deltas aren't supported.");

        let entry = Entry::new(delta.key.clone(), delta.data.clone(), metadata.clone());
        let mut inner = self.inner.write().unwrap();
        entry.write_to_log(&mut inner.log)
    }

    fn flush(&mut self) -> Fallible<Option<PathBuf>> {
        self.inner.write().unwrap().log.flush()?;
        Ok(None)
    }
}

impl LocalStore for IndexedLogDataStore {
    fn from_path(path: &Path) -> Fallible<Self> {
        IndexedLogDataStore::new(path)
    }

    fn get_missing(&self, keys: &[Key]) -> Fallible<Vec<Key>> {
        let inner = self.inner.read().unwrap();
        Ok(keys
            .iter()
            .filter(|k| Entry::from_log(k, &inner.log).is_err())
            .map(|k| k.clone())
            .collect())
    }
}

impl DataStore for IndexedLogDataStore {
    fn get(&self, _key: &Key) -> Fallible<Vec<u8>> {
        unreachable!()
    }

    fn get_delta(&self, key: &Key) -> Fallible<Delta> {
        let inner = self.inner.read().unwrap();
        let mut entry = Entry::from_log(&key, &inner.log)?;
        let content = entry.content()?;
        return Ok(Delta {
            data: content,
            base: None,
            key: key.clone(),
        });
    }

    fn get_delta_chain(&self, key: &Key) -> Fallible<Vec<Delta>> {
        let delta = self.get_delta(key)?;
        return Ok(vec![delta]);
    }

    fn get_meta(&self, key: &Key) -> Fallible<Metadata> {
        let inner = self.inner.read().unwrap();
        Ok(Entry::from_log(&key, &inner.log)?.metadata().clone())
    }
}

impl ToKeys for IndexedLogDataStore {
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

    use bytes::Bytes;
    use tempfile::TempDir;

    use types::testutil::*;

    #[test]
    fn test_empty() {
        let tempdir = TempDir::new().unwrap();
        let mut log = IndexedLogDataStore::new(&tempdir).unwrap();
        log.flush().unwrap();
    }

    #[test]
    fn test_add() {
        let tempdir = TempDir::new().unwrap();
        let mut log = IndexedLogDataStore::new(&tempdir).unwrap();

        let delta = Delta {
            data: Bytes::from(&[1, 2, 3, 4][..]),
            base: None,
            key: key("a", "1"),
        };
        let metadata = Default::default();

        log.add(&delta, &metadata).unwrap();
        log.flush().unwrap();
    }

    #[test]
    fn test_add_get() {
        let tempdir = TempDir::new().unwrap();
        let mut log = IndexedLogDataStore::new(&tempdir).unwrap();

        let delta = Delta {
            data: Bytes::from(&[1, 2, 3, 4][..]),
            base: None,
            key: key("a", "1"),
        };
        let metadata = Default::default();

        log.add(&delta, &metadata).unwrap();
        log.flush().unwrap();

        let log = IndexedLogDataStore::new(&tempdir).unwrap();
        let read_delta = log.get_delta(&delta.key).unwrap();
        assert_eq!(delta, read_delta);
    }

    #[test]
    fn test_lookup_failure() {
        let tempdir = TempDir::new().unwrap();
        let log = IndexedLogDataStore::new(&tempdir).unwrap();

        let key = key("a", "1");
        let err = log.get_delta(&key);

        if let Err(err) = err {
            assert!(err.downcast_ref::<KeyError>().is_some());
        } else {
            panic!("Lookup didn't fail");
        }
    }

    #[test]
    fn test_add_chain() -> Fallible<()> {
        let tempdir = TempDir::new()?;
        let mut log = IndexedLogDataStore::new(&tempdir)?;

        let delta = Delta {
            data: Bytes::from(&[1, 2, 3, 4][..]),
            base: Some(key("a", "1")),
            key: key("a", "2"),
        };
        let metadata = Default::default();

        assert!(log.add(&delta, &metadata).is_err());
        Ok(())
    }

    #[test]
    fn test_iter() -> Fallible<()> {
        let tempdir = TempDir::new()?;
        let mut log = IndexedLogDataStore::new(&tempdir)?;

        let k = key("a", "2");
        let delta = Delta {
            data: Bytes::from(&[1, 2, 3, 4][..]),
            base: None,
            key: k.clone(),
        };
        let metadata = Default::default();

        log.add(&delta, &metadata)?;
        assert!(log.to_keys().into_iter().all(|e| e.unwrap() == k));
        Ok(())
    }

    #[test]
    fn test_corrupted() -> Fallible<()> {
        let tempdir = TempDir::new()?;
        let mut log = IndexedLogDataStore::new(&tempdir)?;

        let k = key("a", "2");
        let delta = Delta {
            data: Bytes::from(&[1, 2, 3, 4][..]),
            base: None,
            key: k.clone(),
        };
        let metadata = Default::default();

        log.add(&delta, &metadata)?;
        log.flush()?;
        drop(log);

        // Corrupt the log by removing the "log" file.
        let mut rotate_log_path = tempdir.path().to_path_buf();
        rotate_log_path.push("0");
        rotate_log_path.push("log");
        remove_file(rotate_log_path)?;

        let mut log = IndexedLogDataStore::new(&tempdir)?;
        let k = key("a", "3");
        let delta = Delta {
            data: Bytes::from(&[1, 2, 3, 4][..]),
            base: None,
            key: k.clone(),
        };
        let metadata = Default::default();
        log.add(&delta, &metadata)?;
        log.flush()?;

        // There should be only one key in the store.
        assert_eq!(log.to_keys().into_iter().count(), 1);
        Ok(())
    }
}
