/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::{
    convert::TryInto,
    fs::File,
    io::{ErrorKind, Read, Write},
    path::{Path, PathBuf},
    str,
};

use anyhow::{ensure, Result};
use bytes::{Bytes, BytesMut};
use crypto::{digest::Digest, sha2::Sha256 as CryptoSha256};
use parking_lot::RwLock;
use serde_derive::{Deserialize, Serialize};

use indexedlog::log::IndexOutput;
use mincode::{deserialize, serialize};
use types::{HgId, Key, Sha256};
use util::path::create_dir;

use crate::{
    datastore::{strip_metadata, DataStore, Delta, Metadata, MutableDeltaStore},
    indexedlogutil::{Store, StoreOpenOptions},
    localstore::LocalStore,
    util::{get_lfs_blobs_path, get_lfs_pointers_path},
};

/// The `LfsPointersStore` holds the mapping between a `HgId` and the content hash (sha256) of the LFS blob.
struct LfsPointersStore(Store);

/// The `LfsBlobsStore` holds the actual blobs. Lookup is done via the content hash (sha256) of the
/// blob.
struct LfsBlobsStore(PathBuf, bool);

struct LfsStoreInner {
    pointers: LfsPointersStore,
    blobs: LfsBlobsStore,
}

/// Main LFS store to be used within the `ContentStore`.
///
/// The on-disk layout of the LFS store is 2 parts:
///  - A pointers store that holds a mapping between a `HgId` and the content hash of a blob. This
///    store is under the top-level directory "pointers".
///  - A blob store that holds the actual data. This store is under the top-level directory
///    "objects". Under that directory, the string representation of its content hash is used to
///    find the file storing the data: <2-digits hex>/<62-digits hex>
pub struct LfsStore {
    inner: RwLock<LfsStoreInner>,
}

/// On-disk format of an LFS pointer. This is directly serialized with the mincode encoding, and
/// thus changes to this structure must be done in a backward and forward compatible fashion.
#[derive(Serialize, Deserialize)]
struct LfsPointersEntry {
    hgid: HgId,
    size: u64,
    is_binary: bool,
    copy_from: Option<Key>,
    content_hash: ContentHash,
}

/// Kind of content hash stored in the LFS pointer. Adding new types is acceptable, re-ordering or
/// removal is forbidden.
#[derive(Serialize, Deserialize)]
enum ContentHash {
    Sha256(Sha256),
}

impl ContentHash {
    fn sha256(data: &Bytes) -> Result<Self> {
        let mut hash = CryptoSha256::new();
        hash.input(data);

        let mut bytes = [0; Sha256::len()];
        hash.result(&mut bytes);
        Ok(ContentHash::Sha256(Sha256::from_slice(&bytes)?))
    }
}

impl LfsPointersStore {
    fn open_options() -> StoreOpenOptions {
        StoreOpenOptions::new()
            .max_log_count(4)
            .max_bytes_per_log(10_000_000)
            .index("node", |_| {
                vec![IndexOutput::Reference(0..HgId::len() as u64)]
            })
    }

    /// Create a local `LfsPointersStore`.
    fn local(path: &Path) -> Result<Self> {
        let path = get_lfs_pointers_path(path)?;
        Ok(Self(LfsPointersStore::open_options().local(path)?))
    }

    /// Create a shared `LfsPointersStore`.
    fn shared(path: &Path) -> Result<Self> {
        let path = get_lfs_pointers_path(path)?;
        Ok(Self(LfsPointersStore::open_options().shared(path)?))
    }

    /// Read an entry from the slice and deserialize it.
    fn get_from_slice(data: &[u8]) -> Result<LfsPointersEntry> {
        Ok(deserialize(data)?)
    }

    fn get(&self, key: &Key) -> Result<Option<LfsPointersEntry>> {
        let mut log_entry = self.0.lookup(key.hgid)?;
        let buf = match log_entry.nth(0) {
            None => return Ok(None),
            Some(buf) => buf?,
        };

        Self::get_from_slice(buf).map(Some)
    }

    fn add(&mut self, entry: LfsPointersEntry) -> Result<()> {
        Ok(self.0.append(serialize(&entry)?)?)
    }
}

impl LfsBlobsStore {
    fn local(path: &Path) -> Result<Self> {
        Ok(Self(get_lfs_blobs_path(path)?, true))
    }

    fn shared(path: &Path) -> Result<Self> {
        Ok(Self(get_lfs_blobs_path(path)?, false))
    }

    fn path(&self, hash: &Sha256) -> PathBuf {
        let mut path = self.0.to_path_buf();
        let mut hex = hash.to_hex();

        let second = hex.split_off(2);
        path.push(hex);
        path.push(second);

        path
    }

    /// Read the blob matching the content hash.
    ///
    /// XXX: The blob hash is not validated.
    fn get(&self, hash: &Sha256) -> Result<Option<Bytes>> {
        let path = self.path(hash);

        let mut file = match File::open(path) {
            Ok(file) => file,
            Err(e) => {
                if e.kind() == ErrorKind::NotFound {
                    return Ok(None);
                } else {
                    return Err(e.into());
                }
            }
        };

        let mut buf = Vec::new();
        file.read_to_end(&mut buf)?;
        Ok(Some(buf.into()))
    }

    /// Test whether the blob store contains the hash. A file of the correct name is for now
    /// indicating that it exists.
    fn contains(&self, hash: &Sha256) -> bool {
        let path = self.path(hash);
        path.is_file()
    }

    /// Add the blob to the store.
    fn add(&mut self, hash: &Sha256, blob: Bytes) -> Result<()> {
        let path = self.path(hash);
        create_dir(path.parent().unwrap())?;

        let mut file = File::create(path)?;
        file.write(&blob)?;

        if self.1 {
            file.sync_all()?;
        }

        Ok(())
    }
}

impl LfsStore {
    fn new(pointers: LfsPointersStore, blobs: LfsBlobsStore) -> Result<Self> {
        Ok(Self {
            inner: RwLock::new(LfsStoreInner { pointers, blobs }),
        })
    }

    /// Create a new local `LfsStore`.
    ///
    /// Local stores will `fsync(2)` data to disk, and will never rotate data out of the store.
    pub fn local(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let pointers = LfsPointersStore::local(path)?;
        let blobs = LfsBlobsStore::local(path)?;
        LfsStore::new(pointers, blobs)
    }

    /// Create a new shared `LfsStore`.
    pub fn shared(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let pointers = LfsPointersStore::shared(path)?;
        let blobs = LfsBlobsStore::shared(path)?;
        LfsStore::new(pointers, blobs)
    }
}

impl LocalStore for LfsStore {
    fn get_missing(&self, keys: &[Key]) -> Result<Vec<Key>> {
        let inner = self.inner.read();
        Ok(keys
            .iter()
            .filter(|k| match inner.pointers.get(k) {
                Ok(None) | Err(_) => true,
                Ok(Some(entry)) => match entry.content_hash {
                    ContentHash::Sha256(hash) => !inner.blobs.contains(&hash),
                },
            })
            .map(|k| k.clone())
            .collect())
    }
}

/// When a file was copied, Mercurial expects the blob that the store returns to contain this copy
/// information
fn rebuild_metadata(data: Bytes, entry: &LfsPointersEntry) -> Bytes {
    if let Some(copy_from) = &entry.copy_from {
        let mut ret = BytesMut::new();

        ret.extend_from_slice(&b"\x01\n"[..]);
        ret.extend_from_slice(&b"copy: "[..]);
        ret.extend_from_slice(copy_from.path.as_ref());
        ret.extend_from_slice(&b"\n"[..]);
        ret.extend_from_slice(&b"copyrev: "[..]);
        ret.extend_from_slice(copy_from.hgid.to_hex().as_bytes());
        ret.extend_from_slice(&b"\n"[..]);
        ret.extend_from_slice(&b"\x01\n"[..]);
        ret.extend_from_slice(data.as_ref());
        ret.freeze()
    } else {
        if data.as_ref().starts_with(b"\x01\n") {
            let mut ret = BytesMut::new();
            ret.extend_from_slice(&b"\x01\n\x01\n"[..]);
            ret.extend_from_slice(data.as_ref());
            ret.freeze()
        } else {
            data
        }
    }
}

impl DataStore for LfsStore {
    fn get(&self, _key: &Key) -> Result<Option<Vec<u8>>> {
        unreachable!()
    }

    fn get_delta(&self, key: &Key) -> Result<Option<Delta>> {
        let inner = self.inner.read();

        let entry = inner.pointers.get(key)?;
        if let Some(entry) = entry {
            let content = match entry.content_hash {
                ContentHash::Sha256(hash) => inner.blobs.get(&hash)?,
            };
            if let Some(content) = content {
                let content = rebuild_metadata(content, &entry);
                return Ok(Some(Delta {
                    data: content,
                    base: None,
                    key: key.clone(),
                }));
            }
        }

        Ok(None)
    }

    fn get_delta_chain(&self, key: &Key) -> Result<Option<Vec<Delta>>> {
        Ok(self.get_delta(key)?.map(|delta| vec![delta]))
    }

    fn get_meta(&self, key: &Key) -> Result<Option<Metadata>> {
        let inner = self.inner.read();

        let entry = inner.pointers.get(key)?;
        if let Some(entry) = entry {
            // XXX: add LFS flag?
            Ok(Some(Metadata {
                size: Some(entry.size.try_into()?),
                flags: None,
            }))
        } else {
            Ok(None)
        }
    }
}

impl MutableDeltaStore for LfsStore {
    fn add(&self, delta: &Delta, _metadata: &Metadata) -> Result<()> {
        ensure!(delta.base.is_none(), "Deltas aren't supported.");

        let (data, copy_from) = strip_metadata(&delta.data)?;
        let content_hash = ContentHash::sha256(&data)?;

        let mut inner = self.inner.write();

        match content_hash {
            ContentHash::Sha256(sha256) => inner.blobs.add(&sha256, data.clone())?,
        };

        let entry = LfsPointersEntry {
            hgid: delta.key.hgid.clone(),
            size: data.len().try_into()?,
            is_binary: data.as_ref().contains(&b'\0'),
            copy_from,
            content_hash,
        };
        inner.pointers.add(entry)
    }

    fn flush(&self) -> Result<Option<PathBuf>> {
        self.inner.write().pointers.0.flush()?;
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use quickcheck::quickcheck;
    use tempfile::TempDir;

    use types::testutil::*;

    #[test]
    fn test_new_shared() -> Result<()> {
        let dir = TempDir::new()?;
        let _ = LfsStore::shared(&dir)?;

        let mut lfs_dir = dir.as_ref().to_owned();
        lfs_dir.push("lfs");
        lfs_dir.push("objects");
        assert!(lfs_dir.is_dir());

        Ok(())
    }

    #[test]
    fn test_new_local() -> Result<()> {
        let dir = TempDir::new()?;
        let _ = LfsStore::local(&dir)?;

        let mut lfs_dir = dir.as_ref().to_owned();
        lfs_dir.push("lfs");
        lfs_dir.push("objects");
        assert!(lfs_dir.is_dir());

        Ok(())
    }

    #[test]
    fn test_add() -> Result<()> {
        let dir = TempDir::new()?;
        let store = LfsStore::shared(&dir)?;

        let k1 = key("a", "2");
        let delta = Delta {
            data: Bytes::from(&[1, 2, 3, 4][..]),
            base: None,
            key: k1.clone(),
        };

        store.add(&delta, &Default::default())?;

        let mut lfs_dir = dir.as_ref().to_owned();
        lfs_dir.push("lfs");
        lfs_dir.push("objects");

        lfs_dir.push("9f");
        assert!(lfs_dir.is_dir());

        lfs_dir.push("64a747e1b97f131fabb6b447296c9b6f0201e79fb3c5356e6c77e89b6a806a");
        assert!(lfs_dir.is_file());

        let mut content = Vec::new();
        File::open(&lfs_dir)?.read_to_end(&mut content)?;

        assert_eq!(Bytes::from(content), delta.data);

        Ok(())
    }

    #[test]
    fn test_add_get_missing() -> Result<()> {
        let dir = TempDir::new()?;
        let store = LfsStore::shared(&dir)?;

        let k1 = key("a", "2");
        let delta = Delta {
            data: Bytes::from(&[1, 2, 3, 4][..]),
            base: None,
            key: k1.clone(),
        };

        assert_eq!(store.get_missing(&[k1.clone()])?, vec![k1.clone()]);
        store.add(&delta, &Default::default())?;
        assert_eq!(store.get_missing(&[k1.clone()])?, vec![]);

        Ok(())
    }

    #[test]
    fn test_add_get() -> Result<()> {
        let dir = TempDir::new()?;
        let store = LfsStore::shared(&dir)?;

        let k1 = key("a", "2");
        let delta = Delta {
            data: Bytes::from(&[1, 2, 3, 4][..]),
            base: None,
            key: k1.clone(),
        };

        store.add(&delta, &Default::default())?;
        let get_delta = store.get_delta(&k1)?;
        assert_eq!(Some(delta), get_delta);

        Ok(())
    }

    quickcheck! {
        fn metadata_strip_rebuild(data: Vec<u8>, copy_from: Option<Key>) -> Result<bool> {
            let data = Bytes::from(data);
            let pointer = LfsPointersEntry {
                hgid: hgid("1234"),
                size: data.len().try_into()?,
                is_binary: true,
                copy_from: copy_from.clone(),
                content_hash: ContentHash::sha256(&data)?,
            };

            let with_metadata = rebuild_metadata(data.clone(), &pointer);
            let (without, copy) = strip_metadata(&with_metadata)?;

            Ok(data == without && copy == copy_from)
        }
    }

    #[test]
    fn test_add_get_copyfrom() -> Result<()> {
        let dir = TempDir::new()?;
        let store = LfsStore::shared(&dir)?;

        let k1 = key("a", "2");
        let delta = Delta {
            data: Bytes::copy_from_slice(
                format!(
                    "\x01\ncopy: {}\ncopyrev: {}\n\x01\nthis is a blob",
                    k1.path, k1.hgid
                )
                .as_bytes(),
            ),
            base: None,
            key: k1.clone(),
        };

        store.add(&delta, &Default::default())?;
        let get_delta = store.get_delta(&k1)?;
        assert_eq!(Some(delta), get_delta);

        Ok(())
    }
}
