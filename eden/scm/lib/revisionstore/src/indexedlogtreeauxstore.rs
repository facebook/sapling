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
use byteorder::BigEndian;
use byteorder::ReadBytesExt;
use byteorder::WriteBytesExt;
use configmodel::Config;
use edenapi_types::DirectoryMetadata as TreeAuxData;
use indexedlog::log::IndexOutput;
use minibytes::Bytes;
use types::Blake3;
use types::HgId;
use types::hgid::ReadHgIdExt;

use crate::StoreType;
use crate::indexedlogutil::Store;
use crate::indexedlogutil::StoreOpenOptions;

pub struct TreeAuxStore {
    store: Store,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    hgid: HgId,
    tree_aux_data: TreeAuxData,
}

/// Read an entry from the slice and deserialize it
///
/// The on-disk format of an entry is the following:
/// - HgId <20 bytes>
/// - Version number (for compatibility) <1 byte>
/// - Augmented manifest blake3 id <32 bytes>
/// - Augmented manifest size <8 unsigned bytes, big-endian>
fn deserialize(bytes: Bytes) -> Result<(HgId, TreeAuxData)> {
    let data: &[u8] = bytes.as_ref();
    let mut cur = Cursor::new(data);
    let hgid = cur.read_hgid()?;
    let version = cur.read_u8()?;
    if version > 0 {
        bail!("unsupported treeauxstore entry version {}", version);
    }
    let mut bytes = [0u8; Blake3::len()];
    cur.read_exact(&mut bytes)?;
    let augmented_manifest_id = Blake3::from_slice(&bytes)?;
    let augmented_manifest_size = cur.read_u64::<BigEndian>()?;
    Ok((
        hgid,
        TreeAuxData {
            augmented_manifest_id,
            augmented_manifest_size,
        },
    ))
}

/// Write an entry to a buffer
fn serialize(hgid: HgId, tree_aux_data: &TreeAuxData) -> Result<Vec<u8>> {
    let mut buf = Vec::with_capacity(HgId::len() + 1 + Blake3::len() + 8);
    serialize_to(hgid, tree_aux_data, &mut buf)?;
    Ok(buf.into())
}

fn serialize_to(hgid: HgId, tree_aux_data: &TreeAuxData, buf: &mut dyn Write) -> Result<()> {
    buf.write_all(hgid.as_ref())?;
    buf.write_u8(0)?; // version
    buf.write_all(tree_aux_data.augmented_manifest_id.as_ref())?;
    buf.write_u64::<BigEndian>(tree_aux_data.augmented_manifest_size)?;
    Ok(())
}

impl TreeAuxStore {
    pub fn new(config: &dyn Config, path: impl AsRef<Path>, store_type: StoreType) -> Result<Self> {
        let open_options = Self::open_options(config);

        let store = match store_type {
            StoreType::Permanent => open_options.permanent(&path),
            StoreType::Rotated => open_options.rotated(&path),
        }?;

        Ok(TreeAuxStore { store })
    }

    fn open_options(config: &dyn Config) -> StoreOpenOptions {
        StoreOpenOptions::new(config)
            .max_log_count(4)
            .max_bytes_per_log(150_000_000)
            .auto_sync_threshold(1_000_000)
            .load_specific_config(config, "treeaux")
            .create(true)
            .index("node", |_| {
                vec![IndexOutput::Reference(0..HgId::len() as u64)]
            })
    }

    pub fn contains(&self, hgid: HgId) -> Result<bool> {
        self.store.read().contains(0, hgid)
    }

    pub fn get(&self, hgid: &HgId) -> Result<Option<TreeAuxData>> {
        let locked_log = self.store.read();
        let mut log_entry = locked_log.lookup(0, hgid)?;
        let buf = match log_entry.next() {
            None => return Ok(None),
            Some(buf) => buf?,
        };
        let bytes = locked_log.slice_to_bytes(buf);
        drop(locked_log);
        let (_hgid, tree_aux_data) = deserialize(bytes)?;
        Ok(Some(tree_aux_data))
    }

    pub fn put(&self, hgid: HgId, tree_aux_data: &TreeAuxData) -> Result<()> {
        let bytes = serialize(hgid, tree_aux_data)?;
        self.store.write().append(bytes)
    }

    pub fn put_batch(&self, items: Vec<(HgId, TreeAuxData)>) -> Result<()> {
        self.store.append_batch(
            items,
            serialize_to,
            // aux data (particularly when fetching trees) can be inserted over and over - set
            // read_before_write=true to avoid the insert if the data is already present.
            true,
        )
    }

    pub fn flush(&self) -> Result<()> {
        self.store.flush()?;
        Ok(())
    }
}
