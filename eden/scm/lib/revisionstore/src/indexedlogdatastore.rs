/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::{
    io::{Cursor, Write},
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{bail, ensure, Result};
use async_trait::async_trait;
use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};
use futures::{FutureExt, StreamExt};
use minibytes::Bytes;
use parking_lot::RwLock;
use tokio::task::spawn_blocking;

use configparser::{config::ConfigSet, convert::ByteCount};
use edenapi_types::{FileEntry, TreeEntry};
use indexedlog::log::IndexOutput;
use lz4_pyframe::{compress, decompress};
use types::{hgid::ReadHgIdExt, HgId, Key, RepoPath};

use crate::{
    datastore::{Delta, HgIdDataStore, HgIdMutableDeltaStore, Metadata, StoreResult},
    indexedlogutil::{Store, StoreOpenOptions},
    localstore::{ExtStoredPolicy, LocalStore},
    newstore::{
        FetchError, FetchStream, KeyStream, ReadStore, WriteResults, WriteStore, WriteStream,
    },
    repack::ToKeys,
    sliceext::SliceExt,
    types::StoreKey,
};

#[derive(Clone, Copy)]
pub enum IndexedLogDataStoreType {
    Local,
    Shared,
}

struct IndexedLogHgIdDataStoreInner {
    log: Store,
}

pub struct IndexedLogHgIdDataStore {
    inner: RwLock<IndexedLogHgIdDataStoreInner>,
    extstored_policy: ExtStoredPolicy,
}

#[derive(Clone, Debug)]
pub struct Entry {
    key: Key,
    metadata: Metadata,

    content: Option<Bytes>,
    compressed_content: Option<Bytes>,
}

impl std::cmp::PartialEq for Entry {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key
            && self.metadata == other.metadata
            && match (self.content_inner(), other.content_inner()) {
                (Ok(c1), Ok(c2)) if c1 == c2 => true,
                _ => false,
            }
    }
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
    /// - HgId <20 bytes>
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
    fn from_slice(data: &[u8]) -> Result<Self> {
        let mut cur = Cursor::new(data);
        let hgid = cur.read_hgid()?;

        let name_len = cur.read_u16::<BigEndian>()? as u64;
        let name_slice =
            data.get_err(cur.position() as usize..(cur.position() + name_len) as usize)?;
        cur.set_position(cur.position() + name_len);
        let filename = RepoPath::from_utf8(name_slice)?;

        let key = Key::new(filename.to_owned(), hgid);

        let metadata = Metadata::read(&mut cur)?;

        let compressed_len = cur.read_u64::<BigEndian>()?;
        let compressed =
            data.get_err(cur.position() as usize..(cur.position() + compressed_len) as usize)?;

        Ok(Entry {
            key,
            content: None,
            compressed_content: Some(Bytes::copy_from_slice(compressed)),
            metadata,
        })
    }

    /// Read an entry from the IndexedLog and deserialize it.
    pub fn from_log(key: &Key, log: &Store) -> Result<Option<Self>> {
        let mut log_entry = log.lookup(0, key.hgid.as_ref().to_vec())?;
        let buf = match log_entry.next() {
            None => return Ok(None),
            Some(buf) => buf?,
        };

        Entry::from_slice(buf).map(Some)
    }

    /// Write an entry to the IndexedLog. See [`from_log`] for the detail about the on-disk format.
    pub fn write_to_log(self, log: &mut Store) -> Result<()> {
        let mut buf = Vec::new();
        buf.write_all(self.key.hgid.as_ref())?;
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

    fn content_inner(&self) -> Result<Bytes> {
        if let Some(content) = self.content.as_ref() {
            return Ok(content.clone());
        }

        if let Some(compressed) = self.compressed_content.as_ref() {
            let raw = Bytes::from(decompress(&compressed)?);
            Ok(raw)
        } else {
            bail!("No content");
        }
    }

    pub fn content(&mut self) -> Result<Bytes> {
        self.content = Some(self.content_inner()?);
        // this unwrap is safe because we assign the field in the line above
        Ok(self.content.as_ref().unwrap().clone())
    }

    pub fn metadata(&self) -> &Metadata {
        &self.metadata
    }

    pub fn key(&self) -> &Key {
        &self.key
    }
}

impl IndexedLogHgIdDataStore {
    /// Create or open an `IndexedLogHgIdDataStore`.
    pub fn new(
        path: impl AsRef<Path>,
        extstored_policy: ExtStoredPolicy,
        config: &ConfigSet,
        store_type: IndexedLogDataStoreType,
    ) -> Result<Self> {
        let open_options = IndexedLogHgIdDataStore::open_options(config)?;

        let log = match store_type {
            IndexedLogDataStoreType::Local => open_options.local(&path),
            IndexedLogDataStoreType::Shared => open_options.shared(&path),
        }?;

        Ok(IndexedLogHgIdDataStore {
            inner: RwLock::new(IndexedLogHgIdDataStoreInner { log }),
            extstored_policy,
        })
    }

    fn open_options(config: &ConfigSet) -> Result<StoreOpenOptions> {
        // Default configuration: 4 x 2.5GB.
        let mut open_options = StoreOpenOptions::new()
            .max_log_count(4)
            .max_bytes_per_log(2500 * 1000 * 1000)
            .auto_sync_threshold(250 * 1024 * 1024)
            .create(true)
            .index("node", |_| {
                vec![IndexOutput::Reference(0..HgId::len() as u64)]
            });

        if let Some(max_log_count) = config.get_opt::<u8>("indexedlog", "data.max-log-count")? {
            open_options = open_options.max_log_count(max_log_count);
        }
        if let Some(max_bytes_per_log) =
            config.get_opt::<ByteCount>("indexedlog", "data.max-bytes-per-log")?
        {
            open_options = open_options.max_bytes_per_log(max_bytes_per_log.value());
        } else if let Some(max_bytes_per_log) =
            config.get_opt::<ByteCount>("remotefilelog", "cachelimit")?
        {
            let log_count: u64 = open_options.max_log_count.unwrap_or(1).max(1).into();
            open_options =
                open_options.max_bytes_per_log((max_bytes_per_log.value() / log_count).max(1));
        }
        Ok(open_options)
    }

    pub fn repair(
        path: PathBuf,
        config: &ConfigSet,
        store_type: IndexedLogDataStoreType,
    ) -> Result<String> {
        match store_type {
            IndexedLogDataStoreType::Local => {
                IndexedLogHgIdDataStore::open_options(config)?.repair_local(path)
            }
            IndexedLogDataStoreType::Shared => {
                IndexedLogHgIdDataStore::open_options(config)?.repair_shared(path)
            }
        }
    }
}

impl std::convert::From<TreeEntry> for Entry {
    fn from(v: TreeEntry) -> Self {
        Entry::new(
            v.key().clone(),
            v.data_unchecked().unwrap().into(),
            Metadata::default(),
        )
    }
}

impl std::convert::From<FileEntry> for Entry {
    fn from(v: FileEntry) -> Self {
        Entry::new(
            v.key().clone(),
            v.data_unchecked().into(),
            v.metadata().clone(),
        )
    }
}

#[async_trait]
impl ReadStore<Key, Entry> for IndexedLogHgIdDataStore {
    async fn fetch_stream(self: Arc<Self>, keys: KeyStream<Key>) -> FetchStream<Key, Entry> {
        Box::pin(keys.then(move |key| {
            let self_ = self.clone();
            let key_ = key.clone();
            spawn_blocking(move || {
                let inner = self_.inner.read();
                match Entry::from_log(&key, &inner.log) {
                    Ok(None) => Err(FetchError::not_found(key.clone())),
                    Ok(Some(entry)) => Ok(entry),
                    Err(e) => Err(FetchError::with_key(key.clone(), e)),
                }
            })
            .map(move |spawn_res| {
                match spawn_res {
                    Ok(Ok(entry)) => Ok(entry),
                    Ok(Err(e)) => Err(e),
                    Err(e) => Err(FetchError::with_key(key_, e)),
                }
            })
        }))
    }
}

#[async_trait]
impl WriteStore<Key, Entry> for IndexedLogHgIdDataStore {
    async fn write_stream(self: Arc<Self>, values: WriteStream<Entry>) -> WriteResults<Key> {
        Box::pin(values.then(move |value| {
            let self_ = self.clone();
            let key = value.key.clone();
            spawn_blocking(move || {
                let mut inner = self_.inner.write();
                let key = value.key.clone();
                match value.write_to_log(&mut inner.log) {
                    Ok(()) => Ok(key),
                    Err(e) => Err((Some(key), e)),
                }
            })
            .map(move |spawn_res| {
                match spawn_res {
                    Ok(Ok(entry)) => Ok(entry),
                    Ok(Err(e)) => Err(e),
                    Err(e) => Err((Some(key), e.into())),
                }
            })
        }))
    }
}

impl HgIdMutableDeltaStore for IndexedLogHgIdDataStore {
    fn add(&self, delta: &Delta, metadata: &Metadata) -> Result<()> {
        ensure!(delta.base.is_none(), "Deltas aren't supported.");

        let entry = Entry::new(delta.key.clone(), delta.data.clone(), metadata.clone());
        let mut inner = self.inner.write();
        entry.write_to_log(&mut inner.log)
    }

    fn flush(&self) -> Result<Option<Vec<PathBuf>>> {
        self.inner.write().log.flush()?;
        Ok(None)
    }
}

impl LocalStore for IndexedLogHgIdDataStore {
    fn get_missing(&self, keys: &[StoreKey]) -> Result<Vec<StoreKey>> {
        let inner = self.inner.read();
        Ok(keys
            .iter()
            .filter(|k| match k {
                StoreKey::HgId(k) => match Entry::from_log(k, &inner.log) {
                    Ok(None) | Err(_) => true,
                    Ok(Some(_)) => false,
                },
                StoreKey::Content(_, _) => true,
            })
            .cloned()
            .collect())
    }
}

impl HgIdDataStore for IndexedLogHgIdDataStore {
    fn get(&self, key: StoreKey) -> Result<StoreResult<Vec<u8>>> {
        let key = match key {
            StoreKey::HgId(key) => key,
            content => return Ok(StoreResult::NotFound(content)),
        };

        let inner = self.inner.read();
        let mut entry = match Entry::from_log(&key, &inner.log)? {
            None => return Ok(StoreResult::NotFound(StoreKey::HgId(key))),
            Some(entry) => entry,
        };

        if self.extstored_policy == ExtStoredPolicy::Ignore && entry.metadata().is_lfs() {
            Ok(StoreResult::NotFound(StoreKey::HgId(key)))
        } else {
            let content = entry.content()?;
            Ok(StoreResult::Found(content.as_ref().to_vec()))
        }
    }

    fn get_meta(&self, key: StoreKey) -> Result<StoreResult<Metadata>> {
        let key = match key {
            StoreKey::HgId(key) => key,
            content => return Ok(StoreResult::NotFound(content)),
        };

        let inner = self.inner.read();
        let entry = match Entry::from_log(&key, &inner.log)? {
            None => return Ok(StoreResult::NotFound(StoreKey::HgId(key))),
            Some(entry) => entry,
        };

        let metadata = entry.metadata();
        if self.extstored_policy == ExtStoredPolicy::Ignore && entry.metadata().is_lfs() {
            Ok(StoreResult::NotFound(StoreKey::HgId(key)))
        } else {
            Ok(StoreResult::Found(metadata.clone()))
        }
    }

    fn refresh(&self) -> Result<()> {
        Ok(())
    }
}

impl ToKeys for IndexedLogHgIdDataStore {
    fn to_keys(&self) -> Vec<Result<Key>> {
        self.inner
            .read()
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

    use futures::stream;
    use minibytes::Bytes;
    use tempfile::TempDir;

    use async_runtime::{block_on_future as block_on, stream_to_iter as block_on_stream};
    use types::testutil::*;

    use crate::newstore::fallback::FallbackStore;

    #[test]
    fn test_empty() {
        let tempdir = TempDir::new().unwrap();
        let log = IndexedLogHgIdDataStore::new(
            &tempdir,
            ExtStoredPolicy::Use,
            &ConfigSet::new(),
            IndexedLogDataStoreType::Shared,
        )
        .unwrap();
        log.flush().unwrap();
    }

    #[test]
    fn test_add() {
        let tempdir = TempDir::new().unwrap();
        let log = IndexedLogHgIdDataStore::new(
            &tempdir,
            ExtStoredPolicy::Use,
            &ConfigSet::new(),
            IndexedLogDataStoreType::Shared,
        )
        .unwrap();

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
        let log = IndexedLogHgIdDataStore::new(
            &tempdir,
            ExtStoredPolicy::Use,
            &ConfigSet::new(),
            IndexedLogDataStoreType::Shared,
        )
        .unwrap();

        let delta = Delta {
            data: Bytes::from(&[1, 2, 3, 4][..]),
            base: None,
            key: key("a", "1"),
        };
        let metadata = Default::default();

        log.add(&delta, &metadata).unwrap();
        log.flush().unwrap();

        let log = IndexedLogHgIdDataStore::new(
            &tempdir,
            ExtStoredPolicy::Use,
            &ConfigSet::new(),
            IndexedLogDataStoreType::Shared,
        )
        .unwrap();
        let read_data = log.get(StoreKey::hgid(delta.key)).unwrap();
        assert_eq!(StoreResult::Found(delta.data.as_ref().to_vec()), read_data);
    }

    #[test]
    fn test_lookup_failure() {
        let tempdir = TempDir::new().unwrap();
        let log = IndexedLogHgIdDataStore::new(
            &tempdir,
            ExtStoredPolicy::Use,
            &ConfigSet::new(),
            IndexedLogDataStoreType::Shared,
        )
        .unwrap();

        let key = StoreKey::hgid(key("a", "1"));
        assert_eq!(log.get(key.clone()).unwrap(), StoreResult::NotFound(key));
    }

    #[test]
    fn test_add_chain() -> Result<()> {
        let tempdir = TempDir::new()?;
        let log = IndexedLogHgIdDataStore::new(
            &tempdir,
            ExtStoredPolicy::Use,
            &ConfigSet::new(),
            IndexedLogDataStoreType::Shared,
        )?;

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
    fn test_iter() -> Result<()> {
        let tempdir = TempDir::new()?;
        let log = IndexedLogHgIdDataStore::new(
            &tempdir,
            ExtStoredPolicy::Use,
            &ConfigSet::new(),
            IndexedLogDataStoreType::Shared,
        )?;

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
    fn test_corrupted() -> Result<()> {
        let tempdir = TempDir::new()?;
        let log = IndexedLogHgIdDataStore::new(
            &tempdir,
            ExtStoredPolicy::Use,
            &ConfigSet::new(),
            IndexedLogDataStoreType::Shared,
        )?;

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

        let log = IndexedLogHgIdDataStore::new(
            &tempdir,
            ExtStoredPolicy::Use,
            &ConfigSet::new(),
            IndexedLogDataStoreType::Shared,
        )?;
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

    #[test]
    fn test_extstored_ignore() -> Result<()> {
        let tempdir = TempDir::new().unwrap();
        let log = IndexedLogHgIdDataStore::new(
            &tempdir,
            ExtStoredPolicy::Ignore,
            &ConfigSet::new(),
            IndexedLogDataStoreType::Shared,
        )
        .unwrap();

        let delta = Delta {
            data: Bytes::from(&[1, 2, 3, 4][..]),
            base: None,
            key: key("a", "1"),
        };

        log.add(
            &delta,
            &Metadata {
                size: None,
                flags: Some(Metadata::LFS_FLAG),
            },
        )?;

        let k = StoreKey::hgid(delta.key);
        assert_eq!(log.get(k.clone())?, StoreResult::NotFound(k));

        Ok(())
    }

    #[test]
    fn test_extstored_use() -> Result<()> {
        let tempdir = TempDir::new().unwrap();
        let log = IndexedLogHgIdDataStore::new(
            &tempdir,
            ExtStoredPolicy::Use,
            &ConfigSet::new(),
            IndexedLogDataStoreType::Shared,
        )
        .unwrap();

        let delta = Delta {
            data: Bytes::from(&[1, 2, 3, 4][..]),
            base: None,
            key: key("a", "1"),
        };

        log.add(
            &delta,
            &Metadata {
                size: None,
                flags: Some(Metadata::LFS_FLAG),
            },
        )?;

        let k = StoreKey::hgid(delta.key);
        assert_eq!(
            log.get(k)?,
            StoreResult::Found(delta.data.as_ref().to_vec())
        );

        Ok(())
    }

    #[test]
    fn test_newstore_read() {
        let tempdir = TempDir::new().unwrap();
        let log = IndexedLogHgIdDataStore::new(
            &tempdir,
            ExtStoredPolicy::Use,
            &ConfigSet::new(),
            IndexedLogDataStoreType::Shared,
        )
        .unwrap();

        let delta = Delta {
            data: Bytes::from(&[1, 2, 3, 4][..]),
            base: None,
            key: key("a", "1"),
        };
        let metadata = Default::default();

        log.add(&delta, &metadata).unwrap();

        log.flush().unwrap();

        let log = Arc::new(log);

        let mut fetched: Vec<_> = block_on_stream(block_on(
            log.fetch_stream(Box::pin(stream::iter(vec![key("a", "1")]))),
        ))
        .collect();

        assert_eq!(fetched.len(), 1);
        assert_eq!(
            fetched
                .get_mut(0)
                .unwrap()
                .as_mut()
                .unwrap()
                .content()
                .unwrap(),
            Bytes::from(&[1, 2, 3, 4][..])
        );
    }

    #[test]
    fn test_newstore_fallback() {
        let tempdir = TempDir::new().unwrap();
        let log1 = IndexedLogHgIdDataStore::new(
            &tempdir,
            ExtStoredPolicy::Use,
            &ConfigSet::new(),
            IndexedLogDataStoreType::Shared,
        )
        .unwrap();
        let log2 = IndexedLogHgIdDataStore::new(
            &tempdir,
            ExtStoredPolicy::Use,
            &ConfigSet::new(),
            IndexedLogDataStoreType::Shared,
        )
        .unwrap();

        let delta1 = Delta {
            data: Bytes::from(&[1, 2, 3, 4][..]),
            base: None,
            key: key("a", "1"),
        };
        let delta2 = Delta {
            data: Bytes::from(&[2, 3, 4, 5][..]),
            base: None,
            key: key("b", "2"),
        };
        let metadata = Default::default();

        log1.add(&delta1, &metadata).unwrap();
        log1.flush().unwrap();

        log2.add(&delta2, &metadata).unwrap();
        log2.flush().unwrap();

        let log1 = Arc::new(log1);
        let log2 = Arc::new(log2);

        let fallback = Arc::new(FallbackStore {
            preferred: log1.clone(),
            fallback: log2,
            write_store: log1,
            write: false,
        });

        let mut fetched: Vec<_> = block_on_stream(block_on(
            fallback.fetch_stream(Box::pin(stream::iter(vec![key("a", "1"), key("b", "2")]))),
        ))
        .collect();

        assert_eq!(fetched.len(), 2);
        assert_eq!(
            fetched
                .get_mut(0)
                .unwrap()
                .as_mut()
                .unwrap()
                .content()
                .unwrap(),
            Bytes::from(&[1, 2, 3, 4][..])
        );
        assert_eq!(
            fetched
                .get_mut(1)
                .unwrap()
                .as_mut()
                .unwrap()
                .content()
                .unwrap(),
            Bytes::from(&[2, 3, 4, 5][..])
        );
    }

    #[test]
    fn test_newstore_write_read() {
        let tempdir = TempDir::new().unwrap();
        let log = IndexedLogHgIdDataStore::new(
            &tempdir,
            ExtStoredPolicy::Use,
            &ConfigSet::new(),
            IndexedLogDataStoreType::Shared,
        )
        .unwrap();

        let entry_key = key("a", "1");
        let content = Bytes::from(&[1, 2, 3, 4][..]);
        let metadata = Default::default();
        let entry = Entry::new(entry_key.clone(), content, metadata);

        let log = Arc::new(log);

        let entries = vec![entry];

        let written: Vec<_> = block_on_stream(block_on(
            log.clone()
                .write_stream(Box::pin(stream::iter(entries.clone()))),
        ))
        .collect();

        assert_eq!(
            written
                .into_iter()
                .map(|r| r.expect("failed to write to test write store"))
                .collect::<Vec<_>>(),
            vec![entry_key.clone()]
        );

        // TODO(meyer): Add "flush" support to WriteStore trait
        log.flush().unwrap();

        let fetched: Vec<_> = block_on_stream(block_on(
            log.fetch_stream(Box::pin(stream::iter(vec![entry_key]))),
        ))
        .collect();

        assert_eq!(
            fetched
                .into_iter()
                .map(|r| r.expect("failed to fetch from test read store"))
                .collect::<Vec<_>>(),
            entries
        );
    }
}
