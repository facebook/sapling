/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]

mod counted_blobstore;
mod disabled;
mod errors;

use abomonation_derive::Abomonation;
use anyhow::{Error, Result};
use async_trait::async_trait;
use auto_impl::auto_impl;
use bytes::{Buf, Bytes};
use context::CoreContext;
use serde_derive::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fmt;
use std::io::Cursor;
use std::ops::{Range, RangeFrom, RangeFull, RangeTo};
use strum_macros::{AsRefStr, Display, EnumIter, EnumString, IntoStaticStr};
use thiserror::Error;

pub use crate::counted_blobstore::CountedBlobstore;
pub use crate::disabled::DisabledBlob;
pub use crate::errors::ErrorKind;

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
        unsafe {
            abomonation::encode(&get_data, &mut bytes).map_err(|_| ())?
        };

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

#[derive(Abomonation, Clone, Debug, PartialEq, Eq, Hash)]
pub struct BlobstoreMetadata {
    ctime: Option<i64>,
}

impl BlobstoreMetadata {
    #[inline]
    pub fn new(ctime: Option<i64>) -> BlobstoreMetadata {
        BlobstoreMetadata { ctime }
    }

    #[inline]
    pub fn ctime(&self) -> Option<i64> {
        self.ctime
    }
}

/// A type representing bytes written to or read from a blobstore. The goal here is to ensure
/// that only types that implement `From<BlobstoreBytes>` and `Into<BlobstoreBytes>` can be
/// stored in the blob store.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct BlobstoreBytes(Bytes);

impl BlobstoreBytes {
    /// Construct an empty BlobstoreBytes
    pub fn empty() -> Self {
        BlobstoreBytes(Bytes::new())
    }

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
#[async_trait]
#[auto_impl(&, Arc, Box)]
pub trait Blobstore: fmt::Display + fmt::Debug + Send + Sync {
    /// Fetch the value associated with `key`, or None if no value is present
    async fn get<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: &'a str,
    ) -> Result<Option<BlobstoreGetData>>;
    /// Associate `value` with `key` for future gets; if `put` is called with different `value`s
    /// for the same key, the implementation may return any `value` it's been given in response
    /// to a `get` for that `key`.
    async fn put<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: String,
        value: BlobstoreBytes,
    ) -> Result<()>;
    /// Check that `get` will return a value for a given `key`, and not None. The provided
    /// implentation just calls `get`, and discards the return value; this can be overridden to
    /// avoid transferring data. In the absence of concurrent `put` calls, this must return
    /// `false` if `get` would return `None`, and `true` if `get` would return `Some(_)`.
    async fn is_present<'a>(&'a self, ctx: &'a CoreContext, key: &'a str) -> Result<bool> {
        Ok(self.get(ctx, key).await?.is_some())
    }
}

/// Mononoke binaries will not overwrite existing blobstore keys by default
pub const DEFAULT_PUT_BEHAVIOUR: PutBehaviour = PutBehaviour::IfAbsent;

/// For blobstore implementors and advanced admin type users to control requested put behaviour
#[derive(
    IntoStaticStr,
    Clone,
    Copy,
    Debug,
    Display,
    EnumIter,
    EnumString,
    Eq,
    PartialEq
)]
pub enum PutBehaviour {
    /// Blobstore::put will overwrite even if key is already present
    Overwrite,
    /// Blobstore::put will overwrite even if key is already present, plus log that and overwrite occured
    OverwriteAndLog,
    /// Blobstore::put will not overwrite if the key is already present.
    /// NB due to underlying stores TOCTOU limitations some puts might overwrite when when racing another put.
    /// This is expected, thus Blobstore::put() cannot reveal if the put wrote or not as behaviour other than
    /// logging/metrics should not depend on it.
    IfAbsent,
}

impl PutBehaviour {
    // For use inside BlobstoreWithPutBehaviour::put_with_behaviour implementations
    pub fn should_overwrite(&self) -> bool {
        match self {
            PutBehaviour::Overwrite | PutBehaviour::OverwriteAndLog => true,
            PutBehaviour::IfAbsent => false,
        }
    }

    // For use inside OverwriteHandler::on_put_overwrite implementations
    pub fn should_log(&self) -> bool {
        match self {
            PutBehaviour::Overwrite => false,
            PutBehaviour::IfAbsent | PutBehaviour::OverwriteAndLog => true,
        }
    }
}

/// For use from logging blobstores so they can record the overwrite status
/// `BlobstorePutOps::put_with_status`, and eventually `BlobstoreWithLink::link()` will return this.
#[derive(AsRefStr, Clone, Copy, Debug, Eq, PartialEq)]
pub enum OverwriteStatus {
    // We did not check if the key existed before writing it
    NotChecked,
    // This key did not exist before, therefore we did not overwrite
    New,
    // This did exist before, but we wrote over it
    Overwrote,
    // This did exist before, and the overwrite was prevented
    Prevented,
}

/// Lower level blobstore put api used by blobstore implementors and admin tooling
#[async_trait]
#[auto_impl(Arc, Box)]
pub trait BlobstorePutOps: Blobstore {
    /// Adds ability to specify the put behaviour explicitly so that even if per process default was
    /// IfAbsent  once could chose to OverwriteAndLog.  Expected to be used only in admin tools
    async fn put_explicit<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: String,
        value: BlobstoreBytes,
        put_behaviour: PutBehaviour,
    ) -> Result<OverwriteStatus>;

    /// Similar to `Blobstore::put`, but returns the OverwriteStatus as feedback rather than unit.
    /// Its here rather so we don't reveal the OverwriteStatus to regular put users.
    async fn put_with_status<'a>(
        &'a self,
        ctx: &'a CoreContext,
        key: String,
        value: BlobstoreBytes,
    ) -> Result<OverwriteStatus>;
}

/// Mixin trait for blobstores that support the `link()` operation
/// TODO(ahornby) rename to BlobstoreLinkOps for consistency with BlobstorePutOps
#[async_trait]
#[auto_impl(Arc, Box)]
pub trait BlobstoreWithLink: Blobstore {
    // TODO(ahornby) return OverwriteStatus for logging
    async fn link<'a>(
        &'a self,
        ctx: &'a CoreContext,
        existing_key: &'a str,
        link_key: String,
    ) -> Result<()>;
}

/// BlobstoreKeySource Interface
/// Abstract for use with populate_healer
#[async_trait]
pub trait BlobstoreKeySource: Blobstore {
    async fn enumerate<'a>(
        &'a self,
        ctx: &'a CoreContext,
        range: &'a BlobstoreKeyParam,
    ) -> Result<BlobstoreEnumerationData>;
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub struct BlobstoreKeyRange {
    // Should match manifold inclusiveness rules, please check and document.
    pub begin_key: String,
    pub end_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub enum BlobstoreKeyToken {
    // For fileblob and manifold
    StringToken(String),
    // its an enum as other stores might have non-string tokens
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub enum BlobstoreKeyParam {
    Start(BlobstoreKeyRange),
    Continuation(BlobstoreKeyToken),
}

impl From<Range<String>> for BlobstoreKeyParam {
    fn from(range: Range<String>) -> Self {
        BlobstoreKeyParam::Start(BlobstoreKeyRange {
            begin_key: range.start,
            end_key: range.end,
        })
    }
}

impl From<RangeTo<String>> for BlobstoreKeyParam {
    fn from(range: RangeTo<String>) -> Self {
        BlobstoreKeyParam::Start(BlobstoreKeyRange {
            begin_key: String::from(""),
            end_key: range.end,
        })
    }
}

impl From<RangeFrom<String>> for BlobstoreKeyParam {
    fn from(range: RangeFrom<String>) -> Self {
        BlobstoreKeyParam::Start(BlobstoreKeyRange {
            begin_key: range.start,
            end_key: String::from(""),
        })
    }
}

impl From<RangeFull> for BlobstoreKeyParam {
    fn from(_: RangeFull) -> Self {
        BlobstoreKeyParam::Start(BlobstoreKeyRange {
            begin_key: String::from(""),
            end_key: String::from(""),
        })
    }
}

#[derive(Debug, Clone)]
pub struct BlobstoreEnumerationData {
    pub keys: HashSet<String>,
    /// current range being iterated, this range can be used to resume enumeration
    pub next_token: Option<BlobstoreKeyParam>,
}

#[derive(Debug, Error)]
pub enum LoadableError {
    #[error("Blobstore error")]
    Error(#[from] Error),
    #[error("Blob is missing: {0}")]
    Missing(String),
}

#[async_trait]
pub trait Loadable {
    type Value: Sized + 'static;

    async fn load<'a, B: Blobstore>(
        &'a self,
        ctx: &'a CoreContext,
        blobstore: &'a B,
    ) -> Result<Self::Value, LoadableError>;
}

#[async_trait]
pub trait Storable: Sized {
    type Key: 'static;

    async fn store<'a, B: Blobstore>(
        self,
        ctx: &'a CoreContext,
        blobstore: &'a B,
    ) -> Result<Self::Key>;
}

/// StoreLoadable represents an object that be loaded asynchronously through a given store of type
/// S. This offers a bit more flexibility over Blobstore's Loadable, which requires that the object
/// be asynchronously load loadable from a Blobstore. This level of indirection allows for using
/// Manifest's implementations with Manifests that are not backed by a Blobstore.
#[async_trait]
pub trait StoreLoadable<S> {
    type Value;

    async fn load<'a>(
        &'a self,
        ctx: &'a CoreContext,
        store: &'a S,
    ) -> Result<Self::Value, LoadableError>;
}

/// For convenience, all Blobstore Loadables are StoreLoadable through any Blobstore.
#[async_trait]
impl<L: Loadable + Sync, S: Blobstore> StoreLoadable<S> for L {
    type Value = <L as Loadable>::Value;

    async fn load<'a>(
        &'a self,
        ctx: &'a CoreContext,
        store: &'a S,
    ) -> Result<Self::Value, LoadableError> {
        self.load(ctx, store).await
    }
}
