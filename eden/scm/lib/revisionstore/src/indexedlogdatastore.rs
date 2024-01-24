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

use anyhow::bail;
use anyhow::ensure;
use anyhow::Result;
use byteorder::BigEndian;
use byteorder::ReadBytesExt;
use byteorder::WriteBytesExt;
use configmodel::convert::ByteCount;
use edenapi_types::FileEntry;
use edenapi_types::TreeEntry;
use indexedlog::log::IndexOutput;
use lz4_pyframe::compress;
use lz4_pyframe::decompress;
use minibytes::Bytes;
use once_cell::sync::OnceCell;
use parking_lot::RwLock;
use tracing::warn;
use types::hgid::ReadHgIdExt;
use types::HgId;
use types::Key;
use types::RepoPath;

use crate::datastore::Delta;
use crate::datastore::HgIdDataStore;
use crate::datastore::HgIdMutableDeltaStore;
use crate::datastore::Metadata;
use crate::datastore::StoreResult;
use crate::indexedlogutil::Store;
use crate::indexedlogutil::StoreOpenOptions;
use crate::indexedlogutil::StoreType;
use crate::localstore::ExtStoredPolicy;
use crate::localstore::LocalStore;
use crate::missing::MissingInjection;
use crate::repack::ToKeys;
use crate::sliceext::SliceExt;
use crate::types::StoreKey;

pub struct IndexedLogHgIdDataStoreConfig {
    pub max_log_count: Option<u8>,
    pub max_bytes_per_log: Option<ByteCount>,
    pub max_bytes: Option<ByteCount>,
}

pub struct IndexedLogHgIdDataStore {
    store: RwLock<Store>,
    extstored_policy: ExtStoredPolicy,
    missing: MissingInjection,
}

#[derive(Clone, Debug)]
pub struct Entry {
    key: Key,
    metadata: Metadata,

    content: OnceCell<Bytes>,
    compressed_content: Option<Bytes>,
}

impl std::cmp::PartialEq for Entry {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key
            && self.metadata == other.metadata
            && match (self.calculate_content(), other.calculate_content()) {
                (Ok(c1), Ok(c2)) if c1 == c2 => true,
                _ => false,
            }
    }
}

impl Entry {
    pub fn new(key: Key, content: Bytes, metadata: Metadata) -> Self {
        Entry {
            key,
            content: OnceCell::with_value(content),
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
    fn from_bytes(bytes: Bytes) -> Result<Self> {
        let data: &[u8] = bytes.as_ref();
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
        let bytes = bytes.slice_to_bytes(compressed);

        Ok(Entry {
            key,
            content: OnceCell::new(),
            compressed_content: Some(bytes),
            metadata,
        })
    }

    /// Read an entry from the IndexedLog and deserialize it.
    pub(crate) fn from_log(id: &[u8], log: &RwLock<Store>) -> Result<Option<Self>> {
        let locked_log = log.read();
        let mut log_entry = locked_log.lookup(0, id)?;
        let buf = match log_entry.next() {
            None => return Ok(None),
            Some(buf) => buf?,
        };

        let bytes = locked_log.slice_to_bytes(buf);
        drop(locked_log);
        Entry::from_bytes(bytes).map(Some)
    }

    /// Write an entry to the IndexedLog. See [`from_log`] for the detail about the on-disk format.
    pub fn write_to_log(self, log: &RwLock<Store>) -> Result<()> {
        let mut buf = Vec::new();
        buf.write_all(self.key.hgid.as_ref())?;
        let path_slice = self.key.path.as_byte_slice();
        buf.write_u16::<BigEndian>(path_slice.len() as u16)?;
        buf.write_all(path_slice)?;
        self.metadata.write(&mut buf)?;

        let compressed = if let Some(compressed) = self.compressed_content {
            compressed
        } else if let Some(raw) = self.content.get() {
            compress(&raw)?.into()
        } else {
            bail!("No content");
        };

        buf.write_u64::<BigEndian>(compressed.len() as u64)?;
        buf.write_all(&compressed)?;

        log.write().append(buf)
    }

    pub(crate) fn calculate_content(&self) -> Result<Bytes> {
        let content = self.content.get_or_try_init(|| {
            if let Some(compressed) = self.compressed_content.as_ref() {
                let raw = Bytes::from(decompress(compressed)?);
                Ok(raw)
            } else {
                bail!("No content");
            }
        })?;
        Ok(content.clone())
    }

    pub fn content(&self) -> Result<Bytes> {
        self.calculate_content()
    }

    pub fn metadata(&self) -> &Metadata {
        &self.metadata
    }

    pub fn key(&self) -> &Key {
        &self.key
    }

    /// Replaces the Entry's key in case caller looked up a different path.
    pub(crate) fn with_key(self, key: Key) -> Self {
        Entry {
            key,
            content: self.content,
            metadata: self.metadata,
            compressed_content: self.compressed_content,
        }
    }
}

impl IndexedLogHgIdDataStore {
    /// Create or open an `IndexedLogHgIdDataStore`.
    pub fn new(
        path: impl AsRef<Path>,
        extstored_policy: ExtStoredPolicy,
        config: &IndexedLogHgIdDataStoreConfig,
        store_type: StoreType,
    ) -> Result<Self> {
        let open_options = IndexedLogHgIdDataStore::open_options(config);

        let log = match store_type {
            StoreType::Local => open_options.local(&path),
            StoreType::Shared => open_options.shared(&path),
        }?;

        Ok(IndexedLogHgIdDataStore {
            store: RwLock::new(log),
            extstored_policy,
            missing: MissingInjection::new_from_env("MISSING_FILES"),
        })
    }

    fn open_options(config: &IndexedLogHgIdDataStoreConfig) -> StoreOpenOptions {
        // If you update defaults/logic here, please update the "cache" help topic
        // calculations in help.py.

        // Default configuration: 4 x 2.5GB.
        let mut open_options = StoreOpenOptions::new()
            .max_log_count(4)
            .max_bytes_per_log(2500 * 1000 * 1000)
            .auto_sync_threshold(50 * 1024 * 1024)
            .create(true)
            .index("node", |_| {
                vec![IndexOutput::Reference(0..HgId::len() as u64)]
            });

        if let Some(max_log_count) = config.max_log_count {
            open_options = open_options.max_log_count(max_log_count);
        }
        if let Some(max_bytes_per_log) = config.max_bytes_per_log {
            open_options = open_options.max_bytes_per_log(max_bytes_per_log.value());
        } else if let Some(max_bytes) = config.max_bytes {
            let log_count: u64 = open_options.max_log_count.unwrap_or(1).max(1).into();
            open_options = open_options.max_bytes_per_log((max_bytes.value() / log_count).max(1));
        }
        open_options
    }

    pub fn repair(
        path: PathBuf,
        config: &IndexedLogHgIdDataStoreConfig,
        store_type: StoreType,
    ) -> Result<String> {
        match store_type {
            StoreType::Local => IndexedLogHgIdDataStore::open_options(config).repair_local(path),
            StoreType::Shared => IndexedLogHgIdDataStore::open_options(config).repair_shared(path),
        }
    }

    /// Attempt to read an Entry from IndexedLog, replacing the stored path with the one from the provided Key
    pub(crate) fn get_entry(&self, key: Key) -> Result<Option<Entry>> {
        Ok(self.get_raw_entry(&key.hgid)?.map(|e| e.with_key(key)))
    }

    // TODO(meyer): Make IndexedLogHgIdDataStore "directly" lockable so we can lock and do a batch of operations (RwLock Guard pattern)
    /// Attempt to read an Entry from IndexedLog, without overwriting the Key (return Key path may not match the request Key path)
    pub(crate) fn get_raw_entry(&self, id: &HgId) -> Result<Option<Entry>> {
        Entry::from_log(id.as_ref(), &self.store)
    }

    /// Directly get the local content. Do not ask remote servers.
    pub(crate) fn get_local_content_direct(&self, id: &HgId) -> Result<Option<Bytes>> {
        let entry = match self.get_raw_entry(&id)? {
            None => return Ok(None),
            Some(v) => v,
        };
        if entry.metadata().is_lfs() {
            // Does not handle the LFS complexity here.
            // It seems this is not actually used in modern setup.
            return Ok(None);
        }
        let data = hgstore::strip_hg_file_metadata(&entry.calculate_content()?)?.0;
        Ok(Some(data))
    }

    /// Write an entry to the IndexedLog
    pub fn put_entry(&self, entry: Entry) -> Result<()> {
        entry.write_to_log(&self.store)
    }

    /// Flush the underlying IndexedLog
    pub fn flush_log(&self) -> Result<()> {
        self.store.write().flush()?;
        Ok(())
    }
}

// TODO(meyer): Remove these infallible conversions, replace with fallible or inherent in LazyFile.
impl From<TreeEntry> for Entry {
    fn from(v: TreeEntry) -> Self {
        Entry::new(
            v.key().clone(),
            // TODO(meyer): Why does this infallible conversion exist? Push the failure to consumer of TryFrom, at worst
            v.data_unchecked().unwrap().into(),
            Metadata::default(),
        )
    }
}

impl From<FileEntry> for Entry {
    fn from(v: FileEntry) -> Self {
        Entry::new(
            v.key().clone(),
            v.content()
                .expect("missing content")
                .data_unchecked()
                .clone()
                .into(),
            v.metadata().expect("missing content").clone(),
        )
    }
}

impl HgIdMutableDeltaStore for IndexedLogHgIdDataStore {
    fn add(&self, delta: &Delta, metadata: &Metadata) -> Result<()> {
        ensure!(delta.base.is_none(), "Deltas aren't supported.");

        let entry = Entry::new(delta.key.clone(), delta.data.clone(), metadata.clone());
        self.put_entry(entry)
    }

    fn flush(&self) -> Result<Option<Vec<PathBuf>>> {
        self.flush_log().map(|_| None)
    }
}

impl LocalStore for IndexedLogHgIdDataStore {
    fn get_missing(&self, keys: &[StoreKey]) -> Result<Vec<StoreKey>> {
        let missing: Vec<StoreKey> = keys
            .iter()
            .filter(|k| match k {
                StoreKey::HgId(k) => {
                    if self.missing.is_missing(&k.path) {
                        warn!("Force missing: {}", k.path);
                        return true;
                    }
                    match Entry::from_log(k.hgid.as_ref(), &self.store) {
                        Ok(None) | Err(_) => true,
                        Ok(Some(_)) => false,
                    }
                }
                StoreKey::Content(_, _) => true,
            })
            .cloned()
            .collect();
        Ok(missing)
    }
}

impl HgIdDataStore for IndexedLogHgIdDataStore {
    fn get(&self, key: StoreKey) -> Result<StoreResult<Vec<u8>>> {
        let key = match key {
            StoreKey::HgId(key) => key,
            content => return Ok(StoreResult::NotFound(content)),
        };

        let entry = match self.get_raw_entry(&key.hgid)? {
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

        let entry = match self.get_raw_entry(&key.hgid)? {
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
        self.flush_log()
    }
}

impl ToKeys for IndexedLogHgIdDataStore {
    fn to_keys(&self) -> Vec<Result<Key>> {
        let log = &self.store.read();
        log.iter()
            .map(|entry| {
                let bytes = log.slice_to_bytes(entry?);
                Entry::from_bytes(bytes)
            })
            .map(|entry| Ok(entry?.key))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use fs_err::remove_file;
    use minibytes::Bytes;
    use tempfile::TempDir;
    use types::testutil::*;

    use super::*;
    use crate::scmstore::FetchMode;
    use crate::scmstore::FileAttributes;
    use crate::scmstore::FileStore;
    use crate::testutil::*;

    #[test]
    fn test_empty() {
        let tempdir = TempDir::new().unwrap();
        let config = IndexedLogHgIdDataStoreConfig {
            max_log_count: None,
            max_bytes_per_log: None,
            max_bytes: None,
        };
        let log = IndexedLogHgIdDataStore::new(
            &tempdir,
            ExtStoredPolicy::Use,
            &config,
            StoreType::Shared,
        )
        .unwrap();
        log.flush().unwrap();
    }

    #[test]
    fn test_add() {
        let tempdir = TempDir::new().unwrap();
        let config = IndexedLogHgIdDataStoreConfig {
            max_log_count: None,
            max_bytes_per_log: None,
            max_bytes: None,
        };
        let log = IndexedLogHgIdDataStore::new(
            &tempdir,
            ExtStoredPolicy::Use,
            &config,
            StoreType::Shared,
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
        let config = IndexedLogHgIdDataStoreConfig {
            max_log_count: None,
            max_bytes_per_log: None,
            max_bytes: None,
        };
        let log = IndexedLogHgIdDataStore::new(
            &tempdir,
            ExtStoredPolicy::Use,
            &config,
            StoreType::Shared,
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

        let config = IndexedLogHgIdDataStoreConfig {
            max_log_count: None,
            max_bytes_per_log: None,
            max_bytes: None,
        };
        let log = IndexedLogHgIdDataStore::new(
            &tempdir,
            ExtStoredPolicy::Use,
            &config,
            StoreType::Shared,
        )
        .unwrap();
        let read_data = log.get(StoreKey::hgid(delta.key)).unwrap();
        assert_eq!(StoreResult::Found(delta.data.as_ref().to_vec()), read_data);
    }

    #[test]
    fn test_lookup_failure() {
        let tempdir = TempDir::new().unwrap();
        let config = IndexedLogHgIdDataStoreConfig {
            max_log_count: None,
            max_bytes_per_log: None,
            max_bytes: None,
        };
        let log = IndexedLogHgIdDataStore::new(
            &tempdir,
            ExtStoredPolicy::Use,
            &config,
            StoreType::Shared,
        )
        .unwrap();

        let key = StoreKey::hgid(key("a", "1"));
        assert_eq!(log.get(key.clone()).unwrap(), StoreResult::NotFound(key));
    }

    #[test]
    fn test_add_chain() -> Result<()> {
        let tempdir = TempDir::new()?;
        let config = IndexedLogHgIdDataStoreConfig {
            max_log_count: None,
            max_bytes_per_log: None,
            max_bytes: None,
        };
        let log = IndexedLogHgIdDataStore::new(
            &tempdir,
            ExtStoredPolicy::Use,
            &config,
            StoreType::Shared,
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
        let config = IndexedLogHgIdDataStoreConfig {
            max_log_count: None,
            max_bytes_per_log: None,
            max_bytes: None,
        };
        let log = IndexedLogHgIdDataStore::new(
            &tempdir,
            ExtStoredPolicy::Use,
            &config,
            StoreType::Shared,
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
        let config = IndexedLogHgIdDataStoreConfig {
            max_log_count: None,
            max_bytes_per_log: None,
            max_bytes: None,
        };
        let log = IndexedLogHgIdDataStore::new(
            &tempdir,
            ExtStoredPolicy::Use,
            &config,
            StoreType::Shared,
        )?;

        let k = key("a", "2");
        let delta = Delta {
            data: Bytes::from(&[1, 2, 3, 4][..]),
            base: None,
            key: k,
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

        let config = IndexedLogHgIdDataStoreConfig {
            max_log_count: None,
            max_bytes_per_log: None,
            max_bytes: None,
        };
        let log = IndexedLogHgIdDataStore::new(
            &tempdir,
            ExtStoredPolicy::Use,
            &config,
            StoreType::Shared,
        )?;
        let k = key("a", "3");
        let delta = Delta {
            data: Bytes::from(&[1, 2, 3, 4][..]),
            base: None,
            key: k,
        };
        let metadata = Default::default();
        log.add(&delta, &metadata)?;
        log.flush()?;

        // There should be only one key in the store.
        assert_eq!(log.to_keys().len(), 1);
        Ok(())
    }

    #[test]
    fn test_extstored_ignore() -> Result<()> {
        let tempdir = TempDir::new().unwrap();
        let config = IndexedLogHgIdDataStoreConfig {
            max_log_count: None,
            max_bytes_per_log: None,
            max_bytes: None,
        };
        let log = IndexedLogHgIdDataStore::new(
            &tempdir,
            ExtStoredPolicy::Ignore,
            &config,
            StoreType::Shared,
        )?;

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
        let config = IndexedLogHgIdDataStoreConfig {
            max_log_count: None,
            max_bytes_per_log: None,
            max_bytes: None,
        };
        let log = IndexedLogHgIdDataStore::new(
            &tempdir,
            ExtStoredPolicy::Use,
            &config,
            StoreType::Shared,
        )?;

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
    fn test_scmstore_read() -> Result<()> {
        let k = key("a", "def6f29d7b61f9cb70b2f14f79cd5c43c38e21b2");
        let d = delta("1234", None, k.clone());
        let meta = Default::default();

        // Setup local indexedlog
        let tmp = TempDir::new()?;
        let config = IndexedLogHgIdDataStoreConfig {
            max_log_count: None,
            max_bytes_per_log: None,
            max_bytes: None,
        };
        let local = Arc::new(IndexedLogHgIdDataStore::new(
            &tmp,
            ExtStoredPolicy::Ignore,
            &config,
            StoreType::Shared,
        )?);

        local.add(&d, &meta).unwrap();
        local.flush().unwrap();

        // Set up local-only FileStore
        let mut store = FileStore::empty();
        store.indexedlog_local = Some(local);

        // Attempt fetch.
        let mut fetched = store
            .fetch(
                std::iter::once(k),
                FileAttributes::CONTENT,
                FetchMode::AllowRemote,
            )
            .single()?
            .expect("key not found");
        assert_eq!(fetched.file_content()?.to_vec(), d.data.as_ref().to_vec());

        Ok(())
    }

    #[test]
    fn test_scmstore_write_read() -> Result<()> {
        let k = key("a", "def6f29d7b61f9cb70b2f14f79cd5c43c38e21b2");
        let d = delta("1234", None, k.clone());
        let meta = Default::default();

        // Setup local indexedlog
        let tmp = TempDir::new()?;
        let config = IndexedLogHgIdDataStoreConfig {
            max_log_count: None,
            max_bytes_per_log: None,
            max_bytes: None,
        };
        let local = Arc::new(IndexedLogHgIdDataStore::new(
            &tmp,
            ExtStoredPolicy::Ignore,
            &config,
            StoreType::Shared,
        )?);

        // Set up local-only FileStore
        let mut store = FileStore::empty();
        store.indexedlog_local = Some(local);

        // Write a file
        store.write_batch(std::iter::once((k.clone(), d.data.clone(), meta)))?;

        // Attempt fetch.
        let mut fetched = store
            .fetch(
                std::iter::once(k),
                FileAttributes::CONTENT,
                FetchMode::AllowRemote,
            )
            .single()?
            .expect("key not found");
        assert_eq!(fetched.file_content()?.to_vec(), d.data.as_ref().to_vec());

        Ok(())
    }

    #[test]
    fn test_scmstore_extstore_use() -> Result<()> {
        let tempdir = TempDir::new()?;
        let config = IndexedLogHgIdDataStoreConfig {
            max_log_count: None,
            max_bytes_per_log: None,
            max_bytes: None,
        };
        let log = IndexedLogHgIdDataStore::new(
            &tempdir,
            ExtStoredPolicy::Use,
            &config,
            StoreType::Shared,
        )?;

        let lfs_key = key("a", "1");
        let nonlfs_key = key("b", "2");
        let content = Bytes::from(&[1, 2, 3, 4][..]);
        let lfs_metadata = Metadata {
            size: None,
            flags: Some(Metadata::LFS_FLAG),
        };
        let nonlfs_metadata = Metadata {
            size: None,
            flags: None,
        };

        let lfs_entry = Entry::new(lfs_key.clone(), content.clone(), lfs_metadata);
        let nonlfs_entry = Entry::new(nonlfs_key.clone(), content.clone(), nonlfs_metadata);

        log.put_entry(lfs_entry)?;
        log.put_entry(nonlfs_entry)?;

        // Set up local-only FileStore
        let mut store = FileStore::empty();
        store.indexedlog_local = Some(Arc::new(log));
        store.extstored_policy = ExtStoredPolicy::Use;
        store.lfs_threshold_bytes = Some(123);

        let fetched = store.fetch(
            vec![lfs_key.clone(), nonlfs_key.clone()].into_iter(),
            FileAttributes::CONTENT,
            FetchMode::AllowRemote,
        );

        let (mut found, missing, _errors) = fetched.consume();
        assert_eq!(
            found
                .get_mut(&nonlfs_key)
                .expect("key not found")
                .file_content()?,
            content
        );

        // Note: We don't fully respect ExtStoredPolicy in scmstore. We try to resolve the pointer,
        // and if we can't we no longer return the serialized pointer. Thus, this fails with
        // "unknown metadata" trying to deserialize a malformed LFS pointer.
        assert!(format!("{:#?}", missing[&lfs_key][0]).contains("unknown metadata"));
        Ok(())
    }

    #[test]
    fn test_scmstore_extstore_ignore() -> Result<()> {
        let tempdir = TempDir::new()?;
        let config = IndexedLogHgIdDataStoreConfig {
            max_log_count: None,
            max_bytes_per_log: None,
            max_bytes: None,
        };
        let log = IndexedLogHgIdDataStore::new(
            &tempdir,
            ExtStoredPolicy::Ignore,
            &config,
            StoreType::Shared,
        )?;

        let lfs_key = key("a", "1");
        let nonlfs_key = key("b", "2");
        let content = Bytes::from(&[1, 2, 3, 4][..]);
        let lfs_metadata = Metadata {
            size: None,
            flags: Some(Metadata::LFS_FLAG),
        };
        let nonlfs_metadata = Metadata {
            size: None,
            flags: None,
        };

        let lfs_entry = Entry::new(lfs_key.clone(), content.clone(), lfs_metadata);
        let nonlfs_entry = Entry::new(nonlfs_key.clone(), content.clone(), nonlfs_metadata);

        log.put_entry(lfs_entry)?;
        log.put_entry(nonlfs_entry)?;

        // Set up local-only FileStore
        let mut store = FileStore::empty();
        store.indexedlog_local = Some(Arc::new(log));
        store.extstored_policy = ExtStoredPolicy::Ignore;
        store.lfs_threshold_bytes = Some(123);

        let fetched = store.fetch(
            vec![lfs_key.clone(), nonlfs_key.clone()].into_iter(),
            FileAttributes::CONTENT,
            FetchMode::AllowRemote,
        );

        let (mut found, missing, _errors) = fetched.consume();
        assert_eq!(
            found
                .get_mut(&nonlfs_key)
                .expect("key not found")
                .file_content()?,
            content
        );

        assert_eq!(missing[&lfs_key].len(), 1);
        Ok(())
    }
}
