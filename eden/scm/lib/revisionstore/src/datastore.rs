/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::{
    ops::Deref,
    path::PathBuf,
    str::{self, FromStr},
};

use anyhow::{bail, Result};
use bytes::Bytes;
use serde_derive::{Deserialize, Serialize};

use types::{HgId, Key, RepoPath};

pub use crate::Metadata;
use crate::{
    localstore::LocalStore,
    types::{ContentHash, StoreKey},
};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Delta {
    pub data: Bytes,
    pub base: Option<Key>,
    pub key: Key,
}

pub trait HgIdDataStore: LocalStore + Send + Sync {
    fn get(&self, key: &Key) -> Result<Option<Vec<u8>>>;
    fn get_delta(&self, key: &Key) -> Result<Option<Delta>>;
    fn get_delta_chain(&self, key: &Key) -> Result<Option<Vec<Delta>>>;
    fn get_meta(&self, key: &Key) -> Result<Option<Metadata>>;
}

/// The `RemoteDataStore` trait indicates that data can fetched over the network. Care must be
/// taken to avoid serially fetching data and instead data should be fetched in bulk via the
/// `prefetch` API.
pub trait RemoteDataStore: HgIdDataStore + Send + Sync {
    /// Attempt to bring the data corresponding to the passed in keys to a local store.
    ///
    /// When implemented on a pure remote store, like the `EdenApi`, the method will always fetch
    /// everything that was asked. On a higher level store, such as the `ContentStore`, this will
    /// avoid fetching data that is already present locally.
    fn prefetch(&self, keys: &[StoreKey]) -> Result<()>;

    /// Send all the blobs referenced by the keys to the remote store.
    fn upload(&self, keys: &[StoreKey]) -> Result<Vec<StoreKey>>;
}

pub trait HgIdMutableDeltaStore: HgIdDataStore + Send + Sync {
    fn add(&self, delta: &Delta, metadata: &Metadata) -> Result<()>;
    fn flush(&self) -> Result<Option<PathBuf>>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContentMetadata {
    pub size: usize,
    pub hash: ContentHash,
    pub is_binary: bool,
}

/// The `ContentDataStore` is intended for pure content only stores
///
/// Overtime, this new trait will replace the need for the `HgIdDataStore`, for now, only the LFS
/// store can implement it. Non content only stores could implement it, but the cost of the
/// `metadata` method will become linear over the blob size, reducing the benefit. A caching layer
/// will need to be put in place to avoid this.
pub trait ContentDataStore: Send + Sync {
    /// Read the blob from the store, the blob returned is the pure blob and will not contain any
    /// Mercurial copy_from header.
    fn blob(&self, key: &StoreKey) -> Result<Option<Bytes>>;

    /// Read the blob metadata from the store.
    fn metadata(&self, key: &StoreKey) -> Result<Option<ContentMetadata>>;
    // XXX: Add write operations.
}

/// Implement `HgIdDataStore` for all types that can be `Deref` into a `HgIdDataStore`. This includes all
/// the smart pointers like `Box`, `Rc`, `Arc`.
impl<T: HgIdDataStore + ?Sized, U: Deref<Target = T> + Send + Sync> HgIdDataStore for U {
    fn get(&self, key: &Key) -> Result<Option<Vec<u8>>> {
        T::get(self, key)
    }
    fn get_delta(&self, key: &Key) -> Result<Option<Delta>> {
        T::get_delta(self, key)
    }
    fn get_delta_chain(&self, key: &Key) -> Result<Option<Vec<Delta>>> {
        T::get_delta_chain(self, key)
    }
    fn get_meta(&self, key: &Key) -> Result<Option<Metadata>> {
        T::get_meta(self, key)
    }
}

/// Implement `RemoteDataStore` for all types that can be `Deref` into a `RemoteDataStore`. This
/// includes all the smart pointers like `Box`, `Rc`, `Arc`.
impl<T: RemoteDataStore + ?Sized, U: Deref<Target = T> + Send + Sync> RemoteDataStore for U {
    fn prefetch(&self, keys: &[StoreKey]) -> Result<()> {
        T::prefetch(self, keys)
    }

    fn upload(&self, keys: &[StoreKey]) -> Result<Vec<StoreKey>> {
        T::upload(self, keys)
    }
}

impl<T: HgIdMutableDeltaStore + ?Sized, U: Deref<Target = T> + Send + Sync> HgIdMutableDeltaStore
    for U
{
    fn add(&self, delta: &Delta, metadata: &Metadata) -> Result<()> {
        T::add(self, delta, metadata)
    }

    fn flush(&self) -> Result<Option<PathBuf>> {
        T::flush(self)
    }
}

/// Implement `ContentDataStore` for all types that can be `Deref` into a `ContentDataStore`.
impl<T: ContentDataStore + ?Sized, U: Deref<Target = T> + Send + Sync> ContentDataStore for U {
    fn blob(&self, key: &StoreKey) -> Result<Option<Bytes>> {
        T::blob(self, key)
    }

    fn metadata(&self, key: &StoreKey) -> Result<Option<ContentMetadata>> {
        T::metadata(self, key)
    }
}

/// Mercurial may embed the copy-from information into the blob itself, in which case, the `Delta`
/// would look like:
///
///   \1
///   copy: path
///   copyrev: sha1
///   \1
///   blob
///
/// If the blob starts with \1\n too, it's escaped by adding \1\n\1\n at the beginning.
pub fn strip_metadata(data: &Bytes) -> Result<(Bytes, Option<Key>)> {
    let slice = data.as_ref();
    if !slice.starts_with(b"\x01\n") {
        return Ok((data.clone(), None));
    }

    let slice = &slice[2..];

    if let Some(pos) = slice.windows(2).position(|needle| needle == b"\x01\n") {
        let slice = &slice[..pos];

        let mut path = None;
        let mut hgid = None;
        for line in slice.split(|c| c == &b'\n') {
            if line.is_empty() {
                continue;
            }

            if line.starts_with(b"copy: ") {
                path = Some(RepoPath::from_str(str::from_utf8(&line[6..])?)?.to_owned());
            } else if line.starts_with(b"copyrev: ") {
                hgid = Some(HgId::from_str(str::from_utf8(&line[9..])?)?);
            } else {
                bail!("Unknown metadata in data: {:?}", line);
            }
        }

        let key = match (path, hgid) {
            (None, Some(_)) => bail!("missing 'copyrev' metadata"),
            (Some(_), None) => bail!("missing 'copy' metadata"),

            (None, None) => None,
            (Some(path), Some(hgid)) => Some(Key::new(path, hgid)),
        };

        Ok((data.slice(2 + pos + 2..), key))
    } else {
        Ok((data.clone(), None))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::io::Cursor;

    use types::testutil::*;

    fn roundtrip_meta_serialize(meta: &Metadata) {
        let mut buf = vec![];
        meta.write(&mut buf).expect("write");
        let read_meta = Metadata::read(&mut Cursor::new(&buf)).expect("meta");
        assert!(*meta == read_meta);
    }

    #[test]
    fn test_metadata_serialize() {
        roundtrip_meta_serialize(&Metadata {
            size: None,
            flags: None,
        });
        roundtrip_meta_serialize(&Metadata {
            size: Some(5),
            flags: None,
        });
        roundtrip_meta_serialize(&Metadata {
            size: Some(0),
            flags: Some(12),
        });
        roundtrip_meta_serialize(&Metadata {
            size: Some(1000),
            flags: Some(12),
        });
        roundtrip_meta_serialize(&Metadata {
            size: Some(234214134),
            flags: Some(9879489),
        });
    }

    #[test]
    fn test_strip_metadata() -> Result<()> {
        let key = key("foo/bar/baz", "1234");
        let data = Bytes::copy_from_slice(
            format!(
                "\x01\ncopy: {}\ncopyrev: {}\n\x01\nthis is a blob",
                key.path, key.hgid
            )
            .as_bytes(),
        );
        let (split_data, path) = strip_metadata(&data)?;
        assert_eq!(split_data, Bytes::from(&b"this is a blob"[..]));
        assert_eq!(path, Some(key));

        let data = Bytes::from(&b"\x01\n\x01\nthis is a blob"[..]);
        let (split_data, path) = strip_metadata(&data)?;
        assert_eq!(split_data, Bytes::from(&b"this is a blob"[..]));
        assert_eq!(path, None);

        let data = Bytes::from(&b"\x01\nthis is a blob"[..]);
        let (split_data, path) = strip_metadata(&data)?;
        assert_eq!(split_data, data);
        assert_eq!(path, None);

        Ok(())
    }
}
