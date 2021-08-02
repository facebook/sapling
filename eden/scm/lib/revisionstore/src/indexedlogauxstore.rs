/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::{
    io::{Cursor, Read, Write},
    path::Path,
};

use anyhow::{bail, Result};
use byteorder::{ReadBytesExt, WriteBytesExt};
use minibytes::Bytes;
use parking_lot::{RwLock, RwLockReadGuard, RwLockWriteGuard};

use configparser::{config::ConfigSet, convert::ByteCount};
use edenapi_types::{ContentId, FileAuxData, Sha1, Sha256};
use indexedlog::log::IndexOutput;
use types::{hgid::ReadHgIdExt, HgId};
use vlqencoding::{VLQDecode, VLQEncode};

use crate::indexedlogutil::{Store, StoreOpenOptions, StoreType};

/// See edenapi_types::FileAuxData and mononoke_types::ContentMetadata
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct Entry {
    pub(crate) total_size: u64,
    pub(crate) content_id: ContentId,
    pub(crate) content_sha1: Sha1,
    pub(crate) content_sha256: Sha256,
}

impl From<FileAuxData> for Entry {
    fn from(v: FileAuxData) -> Self {
        Entry {
            total_size: v.total_size,
            content_id: v.content_id,
            content_sha1: v.sha1,
            content_sha256: v.sha256,
        }
    }
}

impl Entry {
    pub fn total_size(&self) -> u64 {
        self.total_size
    }

    pub fn content_id(&self) -> ContentId {
        self.content_id
    }

    pub fn content_sha1(&self) -> Sha1 {
        self.content_sha1
    }

    pub fn content_sha256(&self) -> Sha256 {
        self.content_sha256
    }

    /// Serialize the Entry to Bytes.
    ///
    /// The serialization format is as follows:
    /// - HgId <20 bytes>
    /// - Version <1 byte> (for compatibility)
    /// - content_id <32 bytes>
    /// - content sha1 <20 bytes>
    /// - content sha256 <32 bytes>
    /// - total_size <u64 VLQ, 1-9 bytes>
    fn serialize(&self, hgid: HgId) -> Result<Bytes> {
        let mut buf = Vec::new();
        buf.write_all(hgid.as_ref())?;
        buf.write_u8(0)?; // write version
        buf.write_all(self.content_id.as_ref())?;
        buf.write_all(self.content_sha1.as_ref())?;
        buf.write_all(self.content_sha256.as_ref())?;
        buf.write_vlq(self.total_size)?;
        Ok(buf.into())
    }

    fn deserialize(bytes: Bytes) -> Result<(HgId, Self)> {
        let data: &[u8] = bytes.as_ref();
        let mut cur = Cursor::new(data);

        let hgid = cur.read_hgid()?;

        let version = cur.read_u8()?;
        if version != 0 {
            bail!("unsupported auxstore entry version {}", version);
        }

        let mut content_id = [0u8; 32];
        cur.read_exact(&mut content_id)?;

        let mut content_sha1 = [0u8; 20];
        cur.read_exact(&mut content_sha1)?;

        let mut content_sha256 = [0u8; 32];
        cur.read_exact(&mut content_sha256)?;

        let total_size: u64 = cur.read_vlq()?;

        Ok((
            hgid,
            Entry {
                content_id: content_id.into(),
                content_sha1: content_sha1.into(),
                content_sha256: content_sha256.into(),
                total_size,
            },
        ))
    }
}

pub struct AuxStoreInner(Store);

impl AuxStoreInner {
    pub fn get(&self, hgid: HgId) -> Result<Option<Entry>> {
        let mut entries = self.0.lookup(0, &hgid)?;

        let slice = match entries.next() {
            None => return Ok(None),
            Some(slice) => slice?,
        };
        let bytes = self.0.slice_to_bytes(slice);

        Entry::deserialize(bytes).map(|(_hgid, entry)| Some(entry))
    }

    pub fn put(&mut self, hgid: HgId, entry: &Entry) -> Result<()> {
        self.0.append(&entry.serialize(hgid)?)?;
        Ok(())
    }

    pub fn flush(&mut self) -> Result<()> {
        self.0.flush()?;
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn hgids(&self) -> Result<Vec<HgId>> {
        let inner = &self.0;
        inner
            .iter()
            .map(|slice| {
                let bytes = inner.slice_to_bytes(slice?);
                Entry::deserialize(bytes).map(|(hgid, _entry)| hgid)
            })
            .collect()
    }
}

pub struct AuxStore(RwLock<AuxStoreInner>);

impl AuxStore {
    pub fn new(path: impl AsRef<Path>, config: &ConfigSet, store_type: StoreType) -> Result<Self> {
        // TODO(meyer): Eliminate "local" AuxStore - always treat it as shared / cache?
        let open_options = AuxStore::open_options(config)?;

        let log = match store_type {
            StoreType::Local => open_options.local(&path),
            StoreType::Shared => open_options.shared(&path),
        }?;

        Ok(AuxStore(RwLock::new(AuxStoreInner(log))))
    }

    pub fn read<'a>(&'a self) -> AuxStoreReadGuard<'a> {
        AuxStoreReadGuard(self.0.read())
    }

    pub fn write<'a>(&'a self) -> AuxStoreWriteGuard<'a> {
        AuxStoreWriteGuard(self.0.write())
    }

    fn open_options(config: &ConfigSet) -> Result<StoreOpenOptions> {
        // TODO(meyer): Decide exactly how we want to configure this store. This is all copied from indexedlogdatastore
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
}

pub struct AuxStoreReadGuard<'a>(RwLockReadGuard<'a, AuxStoreInner>);

impl AuxStoreReadGuard<'_> {
    pub fn get(&self, hgid: HgId) -> Result<Option<Entry>> {
        self.0.get(hgid)
    }

    #[cfg(test)]
    pub(crate) fn hgids(&self) -> Result<Vec<HgId>> {
        self.0.hgids()
    }
}

pub struct AuxStoreWriteGuard<'a>(RwLockWriteGuard<'a, AuxStoreInner>);

impl AuxStoreWriteGuard<'_> {
    /// Run a function with the write guard temporarily unlocked
    ///
    /// Used when calling recursively into contentstore during add
    pub fn unlocked<U>(&mut self, f: impl FnOnce() -> U) -> U {
        RwLockWriteGuard::unlocked(&mut self.0, f)
    }

    pub fn get(&self, hgid: HgId) -> Result<Option<Entry>> {
        self.0.get(hgid)
    }

    pub fn put(&mut self, hgid: HgId, entry: &Entry) -> Result<()> {
        self.0.put(hgid, entry)
    }

    pub fn flush(&mut self) -> Result<()> {
        self.0.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::fs::remove_file;
    use std::str::FromStr;
    use std::sync::Arc;

    use tempfile::TempDir;

    use types::testutil::*;

    use crate::{
        scmstore::{FileAttributes, FileStore},
        testutil::*,
        ExtStoredPolicy, HgIdMutableDeltaStore, IndexedLogHgIdDataStore,
    };

    fn single_byte_sha1(fst: u8) -> Sha1 {
        let mut x: [u8; Sha1::len()] = Default::default();
        x[0] = fst;
        Sha1::from(x)
    }

    #[test]
    fn test_empty() -> Result<()> {
        let tempdir = TempDir::new()?;
        let store = AuxStore::new(&tempdir, &ConfigSet::new(), StoreType::Shared)?;
        store.write().flush()?;
        Ok(())
    }

    #[test]
    fn test_add_get() -> Result<()> {
        let tempdir = TempDir::new().unwrap();
        let store = AuxStore::new(&tempdir, &ConfigSet::new(), StoreType::Shared)?;

        let mut entry = Entry::default();
        entry.total_size = 1;
        entry.content_sha1 = single_byte_sha1(1);

        let k = key("a", "1");

        store.write().put(k.hgid, &entry)?;
        store.write().flush()?;

        let found = store.read().get(k.hgid)?;
        assert_eq!(Some(entry), found);
        Ok(())
    }

    #[test]
    fn test_lookup_failure() -> Result<()> {
        let tempdir = TempDir::new().unwrap();
        let store = AuxStore::new(&tempdir, &ConfigSet::new(), StoreType::Shared)?;

        let mut entry = Entry::default();
        entry.total_size = 1;
        entry.content_sha1 = single_byte_sha1(1);

        let k = key("a", "1");

        store.write().put(k.hgid, &entry)?;
        store.write().flush()?;

        let k2 = key("b", "2");

        let found = store.read().get(k2.hgid)?;
        assert_eq!(None, found);
        Ok(())
    }

    #[test]
    fn test_corrupted() -> Result<()> {
        let tempdir = TempDir::new()?;
        let store = AuxStore::new(&tempdir, &ConfigSet::new(), StoreType::Shared)?;

        let k = key("a", "2");
        let mut entry = Entry::default();
        entry.total_size = 2;
        entry.content_sha1 = single_byte_sha1(2);

        store.write().put(k.hgid, &entry)?;
        store.write().flush()?;
        drop(store);

        // Corrupt the log by removing the "log" file.
        let mut rotate_log_path = tempdir.path().to_path_buf();
        rotate_log_path.push("0");
        rotate_log_path.push("log");
        remove_file(rotate_log_path)?;

        let store = AuxStore::new(&tempdir, &ConfigSet::new(), StoreType::Shared)?;

        let k = key("a", "3");
        let mut entry = Entry::default();
        entry.total_size = 3;
        entry.content_sha1 = single_byte_sha1(3);

        store.write().put(k.hgid, &entry)?;
        store.write().flush()?;

        // There should be only one key in the store.
        assert_eq!(store.read().hgids().into_iter().count(), 1);
        Ok(())
    }

    #[test]
    fn test_scmstore_read() -> Result<()> {
        let tmp = TempDir::new()?;
        let aux = Arc::new(AuxStore::new(&tmp, &ConfigSet::new(), StoreType::Shared)?);

        let mut entry = Entry::default();
        entry.total_size = 1;
        entry.content_sha1 = single_byte_sha1(1);

        let k = key("a", "1");

        aux.write().put(k.hgid, &entry)?;
        aux.write().flush()?;

        // Set up local-only FileStore
        let mut store = FileStore::empty();
        store.aux_local = Some(aux.clone());

        // Attempt fetch.
        let fetched = store
            .fetch(std::iter::once(k.clone()), FileAttributes::AUX)
            .single()?
            .expect("key not found");
        assert_eq!(Some(entry), fetched.aux_data().map(|a| a.into()));
        Ok(())
    }

    #[test]
    fn test_scmstore_compute_read() -> Result<()> {
        let k = key("a", "def6f29d7b61f9cb70b2f14f79cd5c43c38e21b2");
        let d = delta("1234", None, k.clone());
        let meta = Default::default();

        // Setup local indexedlog
        let tmp = TempDir::new()?;
        let content = Arc::new(IndexedLogHgIdDataStore::new(
            &tmp,
            ExtStoredPolicy::Ignore,
            &ConfigSet::new(),
            StoreType::Shared,
        )?);

        content.add(&d, &meta).unwrap();
        content.flush().unwrap();

        let tmp = TempDir::new()?;
        let aux = Arc::new(AuxStore::new(&tmp, &ConfigSet::new(), StoreType::Shared)?);

        // Set up local-only FileStore
        let mut store = FileStore::empty();
        store.cache_to_local_cache = true;
        store.indexedlog_local = Some(content.clone());
        store.aux_local = Some(aux.clone());

        let mut expected = Entry::default();
        expected.total_size = 4;
        expected.content_id = ContentId::from_str(
            "aa6ab85da77ca480b7624172fe44aa9906b6c3f00f06ff23c3e5f60bfd0c414e",
        )?;
        expected.content_sha1 = Sha1::from_str("7110eda4d09e062aa5e4a390b0a572ac0d2c0220")?;
        expected.content_sha256 =
            Sha256::from_str("03ac674216f3e15c761ee1a5e255f067953623c8b388b4459e13f978d7c846f4")?;

        // Attempt fetch.
        let fetched = store
            .fetch(std::iter::once(k.clone()), FileAttributes::AUX)
            .single()?
            .expect("key not found");
        assert_eq!(Some(expected), fetched.aux_data().map(|a| a.into()));

        // Verify we can read it directly too
        let found = aux.read().get(k.hgid)?;
        assert_eq!(Some(expected), found);
        Ok(())
    }
}
