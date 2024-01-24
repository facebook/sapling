/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::io::Cursor;
use std::io::Read;
use std::io::Write;
use std::path::Path;

use anyhow::bail;
use anyhow::Result;
use byteorder::ReadBytesExt;
use byteorder::WriteBytesExt;
use configmodel::convert::ByteCount;
use configmodel::Config;
use configmodel::ConfigExt;
use edenapi_types::FileAuxData;
use indexedlog::log::IndexOutput;
use minibytes::Bytes;
use parking_lot::RwLock;
use types::hgid::ReadHgIdExt;
use types::HgId;
use vlqencoding::VLQDecode;
use vlqencoding::VLQEncode;

use crate::indexedlogutil::Store;
use crate::indexedlogutil::StoreOpenOptions;
use crate::indexedlogutil::StoreType;

/// See edenapi_types::FileAuxData and mononoke_types::ContentMetadataV2
pub(crate) type Entry = FileAuxData;

/// Serialize the Entry to Bytes.
///
/// The serialization format is as follows:
/// - HgId <20 bytes>
/// - Version <1 byte> (for compatibility)
/// - content_id <32 bytes>
/// - content sha1 <20 bytes>
/// - content sha256 <32 bytes>
/// - total_size <u64 VLQ, 1-9 bytes>
/// - presence byte for seeded blake3 <1 byte>
/// - content seeded blake3 <32 OR 0 bytes>
pub(crate) fn serialize(this: &FileAuxData, hgid: HgId) -> Result<Bytes> {
    let mut buf = Vec::new();
    buf.write_all(hgid.as_ref())?;
    buf.write_u8(0)?; // write version
    buf.write_all(this.content_id.as_ref())?;
    buf.write_all(this.sha1.as_ref())?;
    buf.write_all(this.sha256.as_ref())?;
    buf.write_vlq(this.total_size)?;
    match this.seeded_blake3 {
        Some(seeded_blake3) => {
            buf.write_u8(1)?; // A value of 1 indicates the blake3 hash is present
            buf.write_all(seeded_blake3.as_ref())?;
        }
        None => buf.write_u8(0)?, // A value of 0 indicates the blake3 hash is absent
    };
    Ok(buf.into())
}

fn deserialize(bytes: Bytes) -> Result<(HgId, FileAuxData)> {
    let data: &[u8] = bytes.as_ref();
    let mut cur = Cursor::new(data);

    let hgid = cur.read_hgid()?;

    let version = cur.read_u8()?;
    if version != 0 {
        bail!("unsupported auxstore entry version {}", version);
    }

    let mut content_id = [0u8; 32];
    cur.read_exact(&mut content_id)?;

    let mut sha1 = [0u8; 20];
    cur.read_exact(&mut sha1)?;

    let mut sha256 = [0u8; 32];
    cur.read_exact(&mut sha256)?;

    let total_size: u64 = cur.read_vlq()?;
    let remaining = cur.position() < bytes.len() as u64;
    let seeded_blake3 = if remaining && cur.read_u8()? == 1 {
        let mut seeded_blake3 = [0u8; 32];
        cur.read_exact(&mut seeded_blake3)?;
        Some(seeded_blake3.into())
    } else {
        None
    };

    Ok((
        hgid,
        FileAuxData {
            content_id: content_id.into(),
            sha1: sha1.into(),
            sha256: sha256.into(),
            total_size,
            seeded_blake3,
        },
    ))
}

pub struct AuxStore(RwLock<Store>);

impl AuxStore {
    pub fn new(path: impl AsRef<Path>, config: &dyn Config, store_type: StoreType) -> Result<Self> {
        // TODO(meyer): Eliminate "local" AuxStore - always treat it as shared / cache?
        let open_options = AuxStore::open_options(config)?;

        let log = match store_type {
            StoreType::Local => open_options.local(&path),
            StoreType::Shared => open_options.shared(&path),
        }?;

        Ok(AuxStore(RwLock::new(log)))
    }

    fn open_options(config: &dyn Config) -> Result<StoreOpenOptions> {
        // If you update defaults/logic here, please update the "cache" help topic
        // calculations in help.py.

        let mut open_options = StoreOpenOptions::new()
            .max_log_count(4)
            .max_bytes_per_log(250 * 1000 * 1000 / 4)
            .auto_sync_threshold(10 * 1024 * 1024)
            .create(true)
            .index("node", |_| {
                vec![IndexOutput::Reference(0..HgId::len() as u64)]
            });

        if let Some(max_log_count) = config.get_opt::<u8>("indexedlog", "aux.max-log-count")? {
            open_options = open_options.max_log_count(max_log_count);
        }
        if let Some(max_bytes_per_log) =
            config.get_opt::<ByteCount>("indexedlog", "aux.max-bytes-per-log")?
        {
            open_options = open_options.max_bytes_per_log(max_bytes_per_log.value());
        } else if let Some(max_bytes_per_log) =
            config.get_opt::<ByteCount>("remotefilelog", "auxlimit")?
        {
            let log_count: u64 = open_options.max_log_count.unwrap_or(1).max(1).into();
            open_options =
                open_options.max_bytes_per_log((max_bytes_per_log.value() / log_count).max(1));
        }
        Ok(open_options)
    }

    pub fn get(&self, hgid: HgId) -> Result<Option<FileAuxData>> {
        let log = self.0.read();
        let mut entries = log.lookup(0, hgid)?;

        let slice = match entries.next() {
            None => return Ok(None),
            Some(slice) => slice?,
        };
        let bytes = log.slice_to_bytes(slice);
        drop(log);

        deserialize(bytes).map(|(_hgid, entry)| Some(entry))
    }

    pub fn put(&self, hgid: HgId, entry: &Entry) -> Result<()> {
        let serialized = serialize(entry, hgid)?;
        self.0.write().append(&serialized)
    }

    pub fn flush(&self) -> Result<()> {
        self.0.write().flush()
    }

    #[cfg(test)]
    pub(crate) fn hgids(&self) -> Result<Vec<HgId>> {
        let log = self.0.read();
        log.iter()
            .map(|slice| {
                let bytes = log.slice_to_bytes(slice?);
                deserialize(bytes).map(|(hgid, _entry)| hgid)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;
    use std::sync::Arc;

    use edenapi_types::Blake3;
    use edenapi_types::ContentId;
    use edenapi_types::Sha1;
    use edenapi_types::Sha256;
    use fs_err::remove_file;
    use tempfile::TempDir;
    use types::testutil::*;

    use super::*;
    use crate::indexedlogdatastore::IndexedLogHgIdDataStore;
    use crate::indexedlogdatastore::IndexedLogHgIdDataStoreConfig;
    use crate::scmstore::FetchMode;
    use crate::scmstore::FileAttributes;
    use crate::scmstore::FileStore;
    use crate::testutil::*;
    use crate::ExtStoredPolicy;
    use crate::HgIdMutableDeltaStore;

    fn single_byte_sha1(fst: u8) -> Sha1 {
        let mut x: [u8; Sha1::len()] = Default::default();
        x[0] = fst;
        Sha1::from(x)
    }

    #[test]
    fn test_empty() -> Result<()> {
        let tempdir = TempDir::new()?;
        let store = AuxStore::new(&tempdir, &empty_config(), StoreType::Shared)?;
        store.flush()?;
        Ok(())
    }

    #[test]
    fn test_add_get() -> Result<()> {
        let tempdir = TempDir::new().unwrap();
        let store = AuxStore::new(&tempdir, &empty_config(), StoreType::Shared)?;

        let mut entry = Entry::default();
        entry.total_size = 1;
        entry.sha1 = single_byte_sha1(1);

        let k = key("a", "1");

        store.put(k.hgid, &entry)?;
        store.flush()?;

        let found = store.get(k.hgid)?;
        assert_eq!(Some(entry), found);
        Ok(())
    }

    #[test]
    fn test_lookup_failure() -> Result<()> {
        let tempdir = TempDir::new().unwrap();
        let store = AuxStore::new(&tempdir, &empty_config(), StoreType::Shared)?;

        let mut entry = Entry::default();
        entry.total_size = 1;
        entry.sha1 = single_byte_sha1(1);

        let k = key("a", "1");

        store.put(k.hgid, &entry)?;
        store.flush()?;

        let k2 = key("b", "2");

        let found = store.get(k2.hgid)?;
        assert_eq!(None, found);
        Ok(())
    }

    #[test]
    fn test_corrupted() -> Result<()> {
        let tempdir = TempDir::new()?;
        let store = AuxStore::new(&tempdir, &empty_config(), StoreType::Shared)?;

        let k = key("a", "2");
        let mut entry = Entry::default();
        entry.total_size = 2;
        entry.sha1 = single_byte_sha1(2);

        store.put(k.hgid, &entry)?;
        store.flush()?;
        drop(store);

        // Corrupt the log by removing the "log" file.
        let mut rotate_log_path = tempdir.path().to_path_buf();
        rotate_log_path.push("0");
        rotate_log_path.push("log");
        remove_file(rotate_log_path)?;

        let store = AuxStore::new(&tempdir, &empty_config(), StoreType::Shared)?;

        let k = key("a", "3");
        let mut entry = Entry::default();
        entry.total_size = 3;
        entry.sha1 = single_byte_sha1(3);

        store.put(k.hgid, &entry)?;
        store.flush()?;

        // There should be only one key in the store.
        assert_eq!(store.hgids().into_iter().count(), 1);
        Ok(())
    }

    #[test]
    fn test_scmstore_read() -> Result<()> {
        let tmp = TempDir::new()?;
        let aux = Arc::new(AuxStore::new(&tmp, &empty_config(), StoreType::Shared)?);

        let mut entry = Entry::default();
        entry.total_size = 1;
        entry.sha1 = single_byte_sha1(1);

        let k = key("a", "1");

        aux.put(k.hgid, &entry)?;
        aux.flush()?;

        // Set up local-only FileStore
        let mut store = FileStore::empty();
        store.aux_local = Some(aux);

        // Attempt fetch.
        let fetched = store
            .fetch(
                std::iter::once(k),
                FileAttributes::AUX,
                FetchMode::AllowRemote,
            )
            .single()?
            .expect("key not found");
        assert_eq!(entry, fetched.aux_data().expect("no aux data found").into());
        Ok(())
    }

    #[test]
    fn test_scmstore_compute_read() -> Result<()> {
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
        let content = Arc::new(IndexedLogHgIdDataStore::new(
            &tmp,
            ExtStoredPolicy::Ignore,
            &config,
            StoreType::Shared,
        )?);

        content.add(&d, &meta).unwrap();
        content.flush().unwrap();

        let tmp = TempDir::new()?;
        let aux = Arc::new(AuxStore::new(&tmp, &empty_config(), StoreType::Shared)?);

        // Set up local-only FileStore
        let mut store = FileStore::empty();
        store.indexedlog_local = Some(content);
        store.aux_local = Some(aux.clone());

        let mut expected = Entry::default();
        expected.total_size = 4;
        expected.content_id = ContentId::from_str(
            "aa6ab85da77ca480b7624172fe44aa9906b6c3f00f06ff23c3e5f60bfd0c414e",
        )?;
        expected.sha1 = Sha1::from_str("7110eda4d09e062aa5e4a390b0a572ac0d2c0220")?;
        expected.sha256 =
            Sha256::from_str("03ac674216f3e15c761ee1a5e255f067953623c8b388b4459e13f978d7c846f4")?;
        expected.seeded_blake3 = Some(Blake3::from_str(
            "2078b4229b5353de0268efc7f64b68f3c99fb8829e9c052117b4e1e090b2603a",
        )?);
        // Attempt fetch.
        let fetched = store
            .fetch(
                std::iter::once(k.clone()),
                FileAttributes::AUX,
                FetchMode::AllowRemote,
            )
            .single()?
            .expect("key not found");
        assert_eq!(
            expected,
            fetched.aux_data().expect("no aux data found").into()
        );

        // Verify we can read it directly too
        let found = aux.get(k.hgid)?;
        assert_eq!(Some(expected), found);
        Ok(())
    }

    #[test]
    /// Test that we can deserialize non-BLAKE3 entries stored in cache.
    fn test_deserialize_non_blake3_entry() -> Result<()> {
        let k = key("a", "def6f29d7b61f9cb70b2f14f79cd5c43c38e21b2");
        let entry = Entry {
            total_size: 4,
            content_id: ContentId::from_str(
                "aa6ab85da77ca480b7624172fe44aa9906b6c3f00f06ff23c3e5f60bfd0c414e",
            )?,
            sha1: Sha1::from_str("7110eda4d09e062aa5e4a390b0a572ac0d2c0220")?,
            sha256: Sha256::from_str(
                "03ac674216f3e15c761ee1a5e255f067953623c8b388b4459e13f978d7c846f4",
            )?,
            seeded_blake3: Some(Blake3::from_str(
                "2078b4229b5353de0268efc7f64b68f3c99fb8829e9c052117b4e1e090b2603a",
            )?),
        };

        let mut buf = Vec::new();
        buf.write_all(k.hgid.as_ref())?;
        buf.write_u8(0)?; // write version
        buf.write_all(entry.content_id.as_ref())?;
        buf.write_all(entry.sha1.as_ref())?;
        buf.write_all(entry.sha256.as_ref())?;
        buf.write_vlq(entry.total_size)?;

        // Validate that we can deserialize the entry even when the Blake3 hash has not been written to it.
        deserialize(buf.into()).expect("Failed to deserialize non-Blake3 entry");
        Ok(())
    }
}
