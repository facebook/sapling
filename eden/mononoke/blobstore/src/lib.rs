/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]

use auto_impl::auto_impl;
use bytes::{Buf, Bytes};
use std::fmt;

use abomonation_derive::Abomonation;
use anyhow::Error;
use futures::future::{BoxFuture, FutureExt};
use std::io::Cursor;
use thiserror::Error;

use context::CoreContext;

mod counted_blobstore;
pub use crate::counted_blobstore::CountedBlobstore;

mod errors;
pub use crate::errors::ErrorKind;

mod disabled;
pub use crate::disabled::DisabledBlob;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlobstoreGetData {
    meta: BlobstoreMetadata,
    bytes: BlobstoreBytes,
}

const UNCOMPRESSED: u8 = b'0';
const COMPRESSED: u8 = b'1';

impl BlobstoreGetData {
    #[inline]
    pub fn new(meta: BlobstoreMetadata, bytes: BlobstoreBytes) -> Self {
        BlobstoreGetData { meta, bytes }
    }

    #[inline]
    pub fn as_meta(&self) -> &BlobstoreMetadata {
        &self.meta
    }

    #[inline]
    pub fn into_bytes(self) -> BlobstoreBytes {
        self.bytes
    }

    #[inline]
    pub fn as_bytes(&self) -> &BlobstoreBytes {
        &self.bytes
    }

    #[inline]
    pub fn into_raw_bytes(self) -> Bytes {
        self.into_bytes().into_bytes()
    }

    #[inline]
    pub fn as_raw_bytes(&self) -> &Bytes {
        self.as_bytes().as_bytes()
    }

    #[inline]
    pub fn from_bytes<B: Into<Bytes>>(bytes: B) -> Self {
        BlobstoreGetData {
            meta: BlobstoreMetadata { ctime: None },
            bytes: BlobstoreBytes::from_bytes(bytes.into()),
        }
    }

    #[inline]
    pub fn remove_ctime(&mut self) {
        self.meta.ctime = None;
    }

    pub fn encode(self, encode_limit: Option<u64>) -> Result<Bytes, ()> {
        let mut bytes = vec![UNCOMPRESSED];
        let get_data = BlobstoreGetDataSerialisable::from(self);
        unsafe { abomonation::encode(&get_data, &mut bytes).map_err(|_| ())? };

        match encode_limit {
            Some(encode_limit) if bytes.len() as u64 >= encode_limit => {
                let mut compressed = Vec::with_capacity(bytes.len());
                compressed.push(COMPRESSED);

                let mut cursor = Cursor::new(bytes);
                cursor.set_position(1);
                zstd::stream::copy_encode(cursor, &mut compressed, 0 /* use default */)
                    .map_err(|_| ())?;
                Ok(Bytes::from(compressed))
            }
            _ => Ok(Bytes::from(bytes)),
        }
    }

    pub fn decode(mut bytes: Bytes) -> Result<Self, ()> {
        let prefix_size = 1;
        if bytes.len() < prefix_size {
            return Err(());
        }

        let is_compressed = bytes.split_to(prefix_size);
        let mut bytes: Vec<u8> = if is_compressed[0] == COMPRESSED {
            let cursor = Cursor::new(bytes);
            zstd::decode_all(cursor).map_err(|_| ())?
        } else {
            bytes.bytes().into()
        };

        let get_data_serialisable =
            unsafe { abomonation::decode::<BlobstoreGetDataSerialisable>(&mut bytes) };

        let result = get_data_serialisable.and_then(|(get_data_serialisable, tail)| {
            if tail.is_empty() {
                Some(get_data_serialisable.clone().into())
            } else {
                None
            }
        });

        match result {
            Some(val) => Ok(val),
            None => Err(()),
        }
    }
}

impl From<BlobstoreBytes> for BlobstoreGetData {
    fn from(blob_bytes: BlobstoreBytes) -> Self {
        BlobstoreGetData {
            meta: BlobstoreMetadata { ctime: None },
            bytes: blob_bytes,
        }
    }
}

impl From<BlobstoreGetData> for BlobstoreMetadata {
    fn from(blob: BlobstoreGetData) -> Self {
        blob.meta
    }
}

impl From<BlobstoreGetData> for BlobstoreBytes {
    fn from(blob: BlobstoreGetData) -> Self {
        blob.into_bytes()
    }
}

#[derive(Abomonation, Clone, Debug, PartialEq, Eq)]
pub struct BlobstoreMetadata {
    ctime: Option<i64>,
}

impl BlobstoreMetadata {
    #[inline]
    pub fn new(ctime: Option<i64>) -> BlobstoreMetadata {
        BlobstoreMetadata { ctime }
    }

    #[inline]
    pub fn as_ctime(&self) -> &Option<i64> {
        &self.ctime
    }

    #[inline]
    pub fn into_ctime(self) -> Option<i64> {
        self.ctime
    }
}

/// A type representing bytes written to or read from a blobstore. The goal here is to ensure
/// that only types that implement `From<BlobstoreBytes>` and `Into<BlobstoreBytes>` can be
/// stored in the blob store.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlobstoreBytes(Bytes);

impl BlobstoreBytes {
    #[inline]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// This should only be used by blobstore and From/Into<BlobstoreBytes> implementations.
    #[inline]
    pub fn from_bytes<B: Into<Bytes>>(bytes: B) -> Self {
        BlobstoreBytes(bytes.into())
    }

    /// This should only be used by blobstore and From/Into<BlobstoreBytes> implementations.
    #[inline]
    pub fn into_bytes(self) -> Bytes {
        self.0
    }

    /// This should only be used by blobstore and From/Into<BlobstoreBytes> implementations.
    #[inline]
    pub fn as_bytes(&self) -> &Bytes {
        &self.0
    }
}

/// Serialisable counterpart of BlobstoreGetDataSerialisable fields mimic exactly
/// its original struct except in types that cannot be serialised
#[derive(Abomonation, Clone)]
struct BlobstoreGetDataSerialisable {
    meta: BlobstoreMetadata,
    bytes: BlobstoreBytesSerialisable,
}

/// Serialisable counterpart of BlobstoreBytes fields mimic exactly its original
/// struct except in types that cannot be serialised
#[derive(Abomonation, Clone)]
struct BlobstoreBytesSerialisable(Vec<u8>);

impl From<BlobstoreGetData> for BlobstoreGetDataSerialisable {
    fn from(blob: BlobstoreGetData) -> Self {
        BlobstoreGetDataSerialisable {
            meta: blob.meta,
            bytes: blob.bytes.into(),
        }
    }
}

impl From<BlobstoreGetDataSerialisable> for BlobstoreGetData {
    fn from(blob: BlobstoreGetDataSerialisable) -> Self {
        BlobstoreGetData {
            meta: blob.meta,
            bytes: blob.bytes.into(),
        }
    }
}

impl From<BlobstoreBytes> for BlobstoreBytesSerialisable {
    fn from(blob_bytes: BlobstoreBytes) -> Self {
        BlobstoreBytesSerialisable(blob_bytes.into_bytes().bytes().into())
    }
}

impl From<BlobstoreBytesSerialisable> for BlobstoreBytes {
    fn from(blob_bytes: BlobstoreBytesSerialisable) -> Self {
        BlobstoreBytes(blob_bytes.0.into())
    }
}

/// The blobstore interface, shared across all blobstores.
/// A blobstore must provide the following guarantees:
/// 1. `get` and `put` are atomic with respect to each other; a put will either put the entire
///    value, or not put anything, and a get will return either None, or the entire value that an
///    earlier put inserted.
/// 2. Once the future returned by `put` completes, the data is durably stored. This implies that
///    a permanent failure of the backend will not lose the data unless multiple replicas in the
///    backend are lost. For example, if you have replicas in multiple datacentres, you will
///    not lose data until you lose two or more datacentres. However, losing replicas can make the
///    data inaccessible for a time.
/// 3. Once the future returned by `put` completes, calling `get` from any process will get you a
///    future that will return the data that was saved in the blobstore; this is so that after the
///    `put` completes, Mononoke can update a database table and be confident that all Mononoke
///    instances can `get` the blobs that the database refers to.
///
/// Implementations of this trait can assume that the same value is supplied if two keys are
/// equal - in other words, each key is associated with at most one globally unique value.
/// In other words, `put(key, value).and_then(put(key, value2))` implies `value == value2` for the
/// `BlobstoreBytes` definition of equality. If `value != value2`, then the implementation's
/// behaviour is implementation defined (it can overwrite or not write at all, as long as it does
/// not break the atomicity guarantee, and does not have to be consistent in its behaviour).
///
/// Implementations of Blobstore must be `Clone` if they are to interoperate with other Mononoke
/// uses of Blobstores
#[auto_impl(Arc, Box)]
pub trait Blobstore: fmt::Debug + Send + Sync + 'static {
    /// Fetch the value associated with `key`, or None if no value is present
    fn get(
        &self,
        ctx: CoreContext,
        key: String,
    ) -> BoxFuture<'static, Result<Option<BlobstoreGetData>, Error>>;
    /// Associate `value` with `key` for future gets; if `put` is called with different `value`s
    /// for the same key, the implementation may return any `value` it's been given in response
    /// to a `get` for that `key`.
    fn put(
        &self,
        ctx: CoreContext,
        key: String,
        value: BlobstoreBytes,
    ) -> BoxFuture<'static, Result<(), Error>>;
    /// Check that `get` will return a value for a given `key`, and not None. The provided
    /// implentation just calls `get`, and discards the return value; this can be overridden to
    /// avoid transferring data. In the absence of concurrent `put` calls, this must return
    /// `false` if `get` would return `None`, and `true` if `get` would return `Some(_)`.
    fn is_present(&self, ctx: CoreContext, key: String) -> BoxFuture<'static, Result<bool, Error>> {
        let get = self.get(ctx, key);
        async move {
            let opt = get.await?;
            Ok(opt.is_some())
        }
        .boxed()
    }
    /// Errors if a given `key` is not present in the blob store. Useful to abort a chained
    /// future computation early if it cannot succeed unless the `key` is present
    fn assert_present(
        &self,
        ctx: CoreContext,
        key: String,
    ) -> BoxFuture<'static, Result<(), Error>> {
        let is_present = self.is_present(ctx, key.clone());
        async move {
            let present = is_present.await?;
            if present {
                Ok(())
            } else {
                Err(ErrorKind::NotFound(key).into())
            }
        }
        .boxed()
    }
}

/// Mixin trait for blobstores that support the `link()` operation
#[auto_impl(Arc, Box)]
pub trait BlobstoreWithLink: Blobstore {
    fn link(
        &self,
        ctx: CoreContext,
        existing_key: String,
        link_key: String,
    ) -> BoxFuture<'static, Result<(), Error>>;
}

#[derive(Debug, Error)]
pub enum LoadableError {
    #[error("Blobstore error")]
    Error(#[from] Error),
    #[error("Blob is missing: {0}")]
    Missing(String),
}

pub trait Loadable: Sized + 'static {
    type Value;

    fn load<B: Blobstore + Clone>(
        &self,
        ctx: CoreContext,
        blobstore: &B,
    ) -> BoxFuture<'static, Result<Self::Value, LoadableError>>;
}

pub trait Storable: Sized + 'static {
    type Key;

    fn store<B: Blobstore + Clone>(
        self,
        ctx: CoreContext,
        blobstore: &B,
    ) -> BoxFuture<'static, Result<Self::Key, Error>>;
}

/// StoreLoadable represents an object that be loaded asynchronously through a given store of type
/// S. This offers a bit more flexibility over Blobstore's Loadable, which requires that the object
/// be asynchronously load loadable from a Blobstore. This level of indirection allows for using
/// Manifest's implementations with Manifests that are not backed by a Blobstore.
pub trait StoreLoadable<S> {
    type Value;

    fn load(
        &self,
        ctx: CoreContext,
        store: &S,
    ) -> BoxFuture<'static, Result<Self::Value, LoadableError>>;
}

/// For convenience, all Blobstore Loadables are StoreLoadable through any Blobstore.
impl<L: Loadable, S: Blobstore + Clone> StoreLoadable<S> for L {
    type Value = <L as Loadable>::Value;

    fn load(
        &self,
        ctx: CoreContext,
        store: &S,
    ) -> BoxFuture<'static, Result<Self::Value, LoadableError>> {
        self.load(ctx, store)
    }
}
