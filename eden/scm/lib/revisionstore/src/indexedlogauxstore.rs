/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::io::Cursor;
use std::io::Read;
use std::io::Write;
use std::path::Path;

use anyhow::Result;
use anyhow::bail;
use byteorder::ReadBytesExt;
use byteorder::WriteBytesExt;
use configmodel::Config;
use configmodel::ConfigExt;
use configmodel::convert::ByteCount;
use indexedlog::log::IndexOutput;
use minibytes::Bytes;
use types::HgId;
use types::hgid::ReadHgIdExt;
use vlqencoding::VLQDecode;
use vlqencoding::VLQEncode;

use crate::indexedlogutil::Store;
use crate::indexedlogutil::StoreOpenOptions;
use crate::indexedlogutil::StoreType;
use crate::scmstore::FileAuxData;

/// See edenapi_types::FileAuxData and mononoke_types::ContentMetadataV2
pub(crate) type Entry = FileAuxData;

/// Serialize the Entry to Bytes.
///
/// The serialization format (v2) is as follows:
/// - HgId <20 bytes>
/// - Version <1 byte> (for compatibility)
/// - total_size <u64 VLQ, 1-9 bytes>
/// - content sha1 <20 bytes>
/// - content blake3 <32 bytes>
///
/// Note: (v1) was the same but containing also sha256 hash
///       (v0) also contained content_id hash and blake3 hash was optional,
///       also, size field was close to the end, just before the blake3
pub(crate) fn serialize(hgid: HgId, aux: &FileAuxData) -> Result<Bytes> {
    let mut buf = Vec::new();
    serialize_to(hgid, aux, &mut buf)?;
    Ok(buf.into())
}

fn serialize_to(hgid: HgId, aux: &FileAuxData, buf: &mut dyn Write) -> Result<()> {
    buf.write_all(hgid.as_ref())?;
    buf.write_u8(2)?; // write version
    buf.write_vlq(aux.total_size)?;
    buf.write_all(aux.sha1.as_ref())?;
    buf.write_all(aux.blake3.as_ref())?;
    if let Some(ref file_header_metadata) = aux.file_header_metadata {
        buf.write_u8(1)?; // write flag that it is present
        buf.write_vlq(file_header_metadata.len())?; // write size of file_header_metadata blob
        buf.write_all(file_header_metadata.as_ref())?;
    }
    Ok(())
}

fn deserialize(bytes: Bytes) -> Result<Option<(HgId, FileAuxData)>> {
    let data: &[u8] = bytes.as_ref();
    let mut cur = Cursor::new(data);

    let hgid = cur.read_hgid()?;

    let version = cur.read_u8()?;
    if version > 2 {
        bail!("unsupported auxstore entry version {}", version);
    }

    if version == 0 {
        let mut content_id = [0u8; 32];
        cur.read_exact(&mut content_id)?;

        let mut sha1 = [0u8; 20];
        cur.read_exact(&mut sha1)?;

        let mut sha256 = [0u8; 32];
        cur.read_exact(&mut sha256)?;

        let total_size: u64 = cur.read_vlq()?;
        let remaining = cur.position() < bytes.len() as u64;
        let blake3 = if remaining && cur.read_u8()? == 1 {
            let mut blake3 = [0u8; 32];
            cur.read_exact(&mut blake3)?;
            blake3.into()
        } else {
            // invalid auxstore entry (missing blake3), possibly old entry incompatible
            // with the current format, fallback to fetching from remote or local calculation
            return Ok(None);
        };

        Ok(Some((
            hgid,
            FileAuxData {
                total_size,
                sha1: sha1.into(),
                blake3,
                // TODO(liubovd) support serialization and deserialization of the new field
                file_header_metadata: None,
            },
        )))
    } else {
        let total_size: u64 = cur.read_vlq()?;

        let mut sha1 = [0u8; 20];
        cur.read_exact(&mut sha1)?;

        if version == 1 {
            // deprecated from version #2
            let mut sha256 = [0u8; 32];
            cur.read_exact(&mut sha256)?;
        }

        let mut blake3 = [0u8; 32];
        cur.read_exact(&mut blake3)?;

        let mut file_header_metadata = None;
        let remaining = cur.position() < bytes.len() as u64;
        if remaining && cur.read_u8()? == 1 {
            // read size
            let size: u64 = cur.read_vlq()?;
            // read file header metadata blob
            if cur.position() + size <= bytes.len() as u64 {
                let pos = cur.position() as usize;
                file_header_metadata = Some(bytes.slice(pos..pos + size as usize));
            } else {
                bail!("auxstore entry is truncated/corrupted");
            };
        }
        Ok(Some((
            hgid,
            FileAuxData {
                total_size,
                sha1: sha1.into(),
                blake3: blake3.into(),
                file_header_metadata,
            },
        )))
    }
}

pub struct AuxStore(Store);

impl AuxStore {
    pub fn new(path: impl AsRef<Path>, config: &dyn Config, store_type: StoreType) -> Result<Self> {
        // TODO(meyer): Eliminate "local" AuxStore - always treat it as shared / cache?
        let open_options = AuxStore::open_options(config)?;

        let log = match store_type {
            StoreType::Permanent => open_options.permanent(&path),
            StoreType::Rotated => open_options.rotated(&path),
        }?;

        Ok(AuxStore(log))
    }

    fn open_options(config: &dyn Config) -> Result<StoreOpenOptions> {
        // If you update defaults/logic here, please update the "cache" help topic
        // calculations in help.py.

        let mut open_options = StoreOpenOptions::new(config)
            .max_log_count(4)
            .max_bytes_per_log(250 * 1000 * 1000 / 4)
            .auto_sync_threshold(1024 * 1024)
            .load_specific_config(config, "aux")
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

    pub fn get(&self, hgid: &HgId) -> Result<Option<FileAuxData>> {
        let log = self.0.read();
        let mut entries = log.lookup(0, hgid)?;

        let slice = match entries.next() {
            None => return Ok(None),
            Some(slice) => slice?,
        };
        let bytes = log.slice_to_bytes(slice);
        drop(log);

        deserialize(bytes).map(|value| value.map(|(_hgid, entry)| entry))
    }

    pub fn contains(&self, hgid: HgId) -> Result<bool> {
        let log = self.0.read();
        Ok(!log.lookup(0, hgid)?.is_empty()?)
    }

    pub fn put(&self, hgid: HgId, entry: &Entry) -> Result<()> {
        let serialized = serialize(hgid, entry)?;
        self.0.append(&serialized)
    }

    pub fn put_batch(&self, items: Vec<(HgId, Entry)>) -> Result<()> {
        self.0.append_batch(
            items,
            serialize_to,
            // aux data (particularly when fetching trees) can be inserted over and over - set
            // read_before_write=true to avoid the insert if the data is already present.
            true,
        )
    }

    pub fn flush(&self) -> Result<()> {
        self.0.flush()
    }

    #[cfg(test)]
    pub(crate) fn hgids(&self) -> Result<Vec<HgId>> {
        let log = self.0.read();
        Ok(log
            .iter()
            .map(|slice| {
                let bytes = log.slice_to_bytes(slice?);
                deserialize(bytes)
            })
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .filter_map(|v| v.map(|(hgid, _entry)| hgid))
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::str::FromStr;
    use std::sync::Arc;

    use edenapi_types::ContentId;
    use edenapi_types::Sha1;
    use edenapi_types::Sha256;
    use fs_err::remove_file;
    use storemodel::SerializationFormat;
    use tempfile::TempDir;
    use types::Blake3;
    use types::FetchContext;
    use types::testutil::*;

    use super::*;
    use crate::HgIdMutableDeltaStore;
    use crate::indexedlogdatastore::IndexedLogHgIdDataStore;
    use crate::indexedlogdatastore::IndexedLogHgIdDataStoreConfig;
    use crate::scmstore::FileAttributes;
    use crate::scmstore::FileStore;
    use crate::testutil::*;

    fn single_byte_sha1(fst: u8) -> Sha1 {
        let mut x: [u8; Sha1::len()] = Default::default();
        x[0] = fst;
        Sha1::from(x)
    }

    #[test]
    fn test_empty() -> Result<()> {
        let tempdir = TempDir::new()?;
        let store = AuxStore::new(&tempdir, &empty_config(), StoreType::Rotated)?;
        store.flush()?;
        Ok(())
    }

    #[test]
    fn test_add_get() -> Result<()> {
        let tempdir = TempDir::new().unwrap();
        let store = AuxStore::new(&tempdir, &empty_config(), StoreType::Rotated)?;

        let entry = Entry {
            total_size: 1,
            sha1: single_byte_sha1(1),
            ..Default::default()
        };

        let k = key("a", "1");

        store.put(k.hgid, &entry)?;
        store.flush()?;

        let found = store.get(&k.hgid)?;
        assert_eq!(Some(entry), found);
        Ok(())
    }

    #[test]
    fn test_lookup_failure() -> Result<()> {
        let tempdir = TempDir::new().unwrap();
        let store = AuxStore::new(&tempdir, &empty_config(), StoreType::Rotated)?;

        let entry = Entry {
            total_size: 1,
            sha1: single_byte_sha1(1),
            ..Default::default()
        };

        let k = key("a", "1");

        store.put(k.hgid, &entry)?;
        store.flush()?;

        let k2 = key("b", "2");

        let found = store.get(&k2.hgid)?;
        assert_eq!(None, found);
        Ok(())
    }

    #[test]
    fn test_corrupted() -> Result<()> {
        let tempdir = TempDir::new()?;
        let store = AuxStore::new(&tempdir, &empty_config(), StoreType::Rotated)?;

        let k = key("a", "2");
        let entry = Entry {
            total_size: 2,
            sha1: single_byte_sha1(2),
            ..Default::default()
        };

        store.put(k.hgid, &entry)?;
        store.flush()?;
        drop(store);

        // Corrupt the log by removing the "log" file.
        let mut rotate_log_path = tempdir.path().to_path_buf();
        rotate_log_path.push("0");
        rotate_log_path.push("log");
        remove_file(rotate_log_path)?;

        let store = AuxStore::new(&tempdir, &empty_config(), StoreType::Rotated)?;

        let k = key("a", "3");
        let entry = Entry {
            total_size: 3,
            sha1: single_byte_sha1(3),
            ..Default::default()
        };

        store.put(k.hgid, &entry)?;
        store.flush()?;

        // There should be only one key in the store.
        assert_eq!(store.hgids().into_iter().count(), 1);
        Ok(())
    }

    #[test]
    fn test_scmstore_read() -> Result<()> {
        let tmp = TempDir::new()?;
        let aux = Arc::new(AuxStore::new(&tmp, &empty_config(), StoreType::Rotated)?);

        let entry = Entry {
            total_size: 1,
            sha1: single_byte_sha1(1),
            ..Default::default()
        };

        let k = key("a", "1");

        aux.put(k.hgid, &entry)?;
        aux.flush()?;

        // Set up local-only FileStore
        let mut store = FileStore::empty();
        store.aux_cache = Some(aux);

        // Attempt fetch.
        let fetched = store
            .fetch(
                FetchContext::default(),
                std::iter::once(k),
                FileAttributes::AUX,
            )
            .single()?
            .expect("key not found");
        assert_eq!(entry, fetched.aux_data().expect("no aux data found"));
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
            btrfs_compression: false,
        };
        let content = Arc::new(IndexedLogHgIdDataStore::new(
            &BTreeMap::<&str, &str>::new(),
            &tmp,
            &config,
            StoreType::Rotated,
            SerializationFormat::Hg,
        )?);

        content.add(&d, &meta).unwrap();
        content.flush().unwrap();

        let tmp = TempDir::new()?;
        let aux = Arc::new(AuxStore::new(&tmp, &empty_config(), StoreType::Rotated)?);

        // Set up local-only FileStore
        let mut store = FileStore::empty();
        store.indexedlog_local = Some(content);
        store.aux_cache = Some(aux.clone());
        store.compute_aux_data = true;

        let expected = Entry {
            total_size: 4,
            sha1: Sha1::from_str("7110eda4d09e062aa5e4a390b0a572ac0d2c0220")?,
            blake3: Blake3::from_str(
                "2078b4229b5353de0268efc7f64b68f3c99fb8829e9c052117b4e1e090b2603a",
            )?,
            file_header_metadata: Some(Default::default()),
        };
        // Attempt fetch.
        let fetched = store
            .fetch(
                FetchContext::default(),
                std::iter::once(k.clone()),
                FileAttributes::AUX,
            )
            .single()?
            .expect("key not found");
        assert_eq!(expected, fetched.aux_data().expect("no aux data found"));

        // Verify we can read it directly too
        let found = aux.get(&k.hgid)?;
        assert_eq!(Some(expected), found);
        Ok(())
    }

    #[test]
    /// Test that we can deserialize old non-BLAKE3 entries stored in cache as "missing" rather than fail.
    fn test_deserialize_non_blake3_entry() -> Result<()> {
        let mut buf = Vec::new();
        buf.write_all(
            key("a", "def6f29d7b61f9cb70b2f14f79cd5c43c38e21b2")
                .hgid
                .as_ref(),
        )?;
        buf.write_u8(0)?; // write version
        buf.write_all(
            ContentId::from_str(
                "aa6ab85da77ca480b7624172fe44aa9906b6c3f00f06ff23c3e5f60bfd0c414e",
            )?
            .as_ref(),
        )?;
        buf.write_all(Sha1::from_str("7110eda4d09e062aa5e4a390b0a572ac0d2c0220")?.as_ref())?;
        buf.write_all(
            Sha256::from_str("03ac674216f3e15c761ee1a5e255f067953623c8b388b4459e13f978d7c846f4")?
                .as_ref(),
        )?;
        buf.write_vlq(4)?;

        // Validate that we can deserialize the entry even when the Blake3 hash has not been written to it.
        assert_eq!(
            deserialize(buf.into()).expect("Failed to deserialize non-Blake3 entry"),
            None
        );

        Ok(())
    }

    #[test]
    /// Test that we can deserialize old entries stored in the cache (older format with version 0)
    fn test_deserialize_old_format_entry_v0() -> Result<()> {
        let mut buf = Vec::new();
        buf.write_all(
            key("a", "def6f29d7b61f9cb70b2f14f79cd5c43c38e21b2")
                .hgid
                .as_ref(),
        )?;
        buf.write_u8(0)?; // write version
        buf.write_all(
            ContentId::from_str(
                "aa6ab85da77ca480b7624172fe44aa9906b6c3f00f06ff23c3e5f60bfd0c414e",
            )?
            .as_ref(),
        )?;
        buf.write_all(Sha1::from_str("7110eda4d09e062aa5e4a390b0a572ac0d2c0220")?.as_ref())?;
        buf.write_all(
            Sha256::from_str("03ac674216f3e15c761ee1a5e255f067953623c8b388b4459e13f978d7c846f4")?
                .as_ref(),
        )?;
        buf.write_vlq(4)?;
        buf.write_u8(1)?; // A value of 1 indicates the blake3 hash is present
        buf.write_all(
            Blake3::from_str("2078b4229b5353de0268efc7f64b68f3c99fb8829e9c052117b4e1e090b2603a")?
                .as_ref(),
        )?;

        assert!(
            deserialize(buf.into())
                .expect("Failed to deserialize old format entry")
                .is_some(),
        );

        Ok(())
    }

    #[test]
    /// Test that we can deserialize old entries stored in the cache (older format with version 1)
    fn test_deserialize_old_format_entry_v1() -> Result<()> {
        let mut buf = Vec::new();
        buf.write_all(
            key("a", "def6f29d7b61f9cb70b2f14f79cd5c43c38e21b2")
                .hgid
                .as_ref(),
        )?;
        buf.write_u8(1)?; // write version
        buf.write_vlq(4)?;
        buf.write_all(Sha1::from_str("7110eda4d09e062aa5e4a390b0a572ac0d2c0220")?.as_ref())?;
        buf.write_all(
            Sha256::from_str("03ac674216f3e15c761ee1a5e255f067953623c8b388b4459e13f978d7c846f4")?
                .as_ref(),
        )?;
        buf.write_all(
            Blake3::from_str("2078b4229b5353de0268efc7f64b68f3c99fb8829e9c052117b4e1e090b2603a")?
                .as_ref(),
        )?;

        assert!(
            deserialize(buf.into())
                .expect("Failed to deserialize old format entry")
                .is_some(),
        );

        Ok(())
    }

    #[test]
    /// Test that we can deserialize correctly the v2 format
    /// This test also covers the case where the file header metadata wasn't written at all (so adding it is forward and backward compatible)
    fn test_deserialize_entry_v2() -> Result<()> {
        let mut buf = Vec::new();
        buf.write_all(
            key("a", "def6f29d7b61f9cb70b2f14f79cd5c43c38e21b2")
                .hgid
                .as_ref(),
        )?;
        buf.write_u8(2)?; // write version
        buf.write_vlq(4)?;
        buf.write_all(Sha1::from_str("7110eda4d09e062aa5e4a390b0a572ac0d2c0220")?.as_ref())?;
        buf.write_all(
            Blake3::from_str("2078b4229b5353de0268efc7f64b68f3c99fb8829e9c052117b4e1e090b2603a")?
                .as_ref(),
        )?;
        assert!(
            deserialize(buf.into())
                .expect("Failed to deserialize entry")
                .is_some(),
        );
        Ok(())
    }

    #[test]
    /// Test that we can deserialize and store values with file header metadata
    fn test_deserialize_with_file_header_metadata() -> Result<()> {
        let hg_id = HgId::from_hex(b"def6f29d7b61f9cb70b2f14f79cd5c43c38e21b2")?;
        let test_entry = Entry {
            total_size: 4,
            sha1: Sha1::from_str("7110eda4d09e062aa5e4a390b0a572ac0d2c0220")?,
            blake3: Blake3::from_str(
                "2078b4229b5353de0268efc7f64b68f3c99fb8829e9c052117b4e1e090b2603a",
            )?,
            file_header_metadata: Some("\x01\ncopy: aaa/bbb/ccc/ddd/test_file.php\ncopyrev: 79c2d9e37f2f90e2ee3cb05762224eea0b864e12\n\x01\n".into()),
        };
        let bytes = serialize(hg_id, &test_entry)?;
        let (hg_id1, test_entry1) = deserialize(bytes)?.expect("Failed to deserialize entry");
        assert_eq!(hg_id, hg_id1);
        assert_eq!(test_entry, test_entry1);
        Ok(())
    }

    #[test]
    /// Test that we can deserialize and store values with empty file header metadata
    fn test_deserialize_with_empty_file_header_metadata() -> Result<()> {
        let hg_id = HgId::from_hex(b"def6f29d7b61f9cb70b2f14f79cd5c43c38e21b2")?;
        let test_entry = Entry {
            total_size: 4,
            sha1: Sha1::from_str("7110eda4d09e062aa5e4a390b0a572ac0d2c0220")?,
            blake3: Blake3::from_str(
                "2078b4229b5353de0268efc7f64b68f3c99fb8829e9c052117b4e1e090b2603a",
            )?,
            file_header_metadata: None,
        };
        let bytes = serialize(hg_id, &test_entry)?;
        let (hg_id1, test_entry1) = deserialize(bytes)?.expect("Failed to deserialize entry");
        assert_eq!(hg_id, hg_id1);
        assert_eq!(test_entry, test_entry1);
        Ok(())
    }
}
