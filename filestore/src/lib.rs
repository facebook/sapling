// Copyright (c) 2019-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![feature(never_type)]
#![deny(warnings)]

use std::sync::Arc;

use bytes::Bytes;

use cloned::cloned;
use failure_ext::Error;
use futures::{future, prelude::*, stream};
use futures_ext::FutureExt;

use blobstore::Blobstore;
use context::CoreContext;
use mononoke_types::{
    blob::{BlobstoreBytes, BlobstoreValue},
    hash, ContentAlias, ContentId, ContentMetadata, ContentMetadataId, FileContents, MononokeId,
};

mod streamhash;

#[cfg(test)]
mod test;

use streamhash::*;

/// File storage.
///
/// This is a specialized wrapper around a blobstore specifically for user data files (rather
/// rather than metadata, trees, etc). Its primary (initial) goals are:
/// - providing a streaming interface for file access
/// - maintain multiple aliases for each file using different key schemes
/// - maintain reverse mapping from primary key to aliases
///
/// Secondary:
/// - Implement chunking at this level
/// - Compression
/// - Range access (initially fetch, and later store)
///
/// Implementation notes:
/// This code takes over the management of file content in a backwards compatible way - it uses
/// the same blobstore key structure and the same encoding schemes for existing files.
/// Extensions (compression, chunking) will change this, but it will still allow backwards
/// compatibility.
#[derive(Debug, Clone)]
pub struct Filestore {
    blobstore: Arc<dyn Blobstore>,
    config: FilestoreConfig,
}

#[derive(Debug, Clone)]
pub struct FilestoreConfig {}

impl Default for FilestoreConfig {
    fn default() -> Self {
        FilestoreConfig {}
    }
}

/// Key for fetching - we can access with any of the supported key types
#[derive(Debug, Clone)]
pub enum FetchKey {
    Canonical(ContentId),
    Sha1(hash::Sha1),
    Sha256(hash::Sha256),
    GitSha1(hash::GitSha1),
}

impl FetchKey {
    fn blobstore_key(&self) -> String {
        use FetchKey::*;

        match self {
            Canonical(contentid) => contentid.blobstore_key(),
            GitSha1(gitkey) => format!("alias.gitsha1.{}", gitkey.to_hex()),
            Sha1(sha1) => format!("alias.sha1.{}", sha1.to_hex()),
            Sha256(sha256) => format!("alias.sha256.{}", sha256.to_hex()),
        }
    }
}

/// Key for storing. We'll compute any missing keys forms, but we must have the
/// canonical key (blake2), and the total size.
#[derive(Debug, Clone)]
pub struct StoreKey {
    pub total_size: u64,
    pub canonical: ContentId,
    pub sha1: Option<hash::Sha1>,
    pub sha256: Option<hash::Sha256>,
    pub git_sha1: Option<hash::GitSha1>,
}

impl Filestore {
    pub fn new(blobstore: Arc<dyn Blobstore>) -> Self {
        Self::with_config(blobstore, FilestoreConfig::default())
    }

    pub fn with_config(blobstore: Arc<dyn Blobstore>, config: FilestoreConfig) -> Self {
        Filestore { blobstore, config }
    }

    /// Return the canonical ID for a key. It doesn't check if the corresponding content
    /// actually exists (its possible for an alias to exist before the ID if there was an
    /// interrupted store operation).
    pub fn get_canonical_id(
        &self,
        ctxt: CoreContext,
        key: &FetchKey,
    ) -> impl Future<Item = Option<ContentId>, Error = Error> {
        match key {
            FetchKey::Canonical(canonical) => future::ok(Some(*canonical)).left_future(),
            aliaskey => self
                .blobstore
                .get(ctxt, aliaskey.blobstore_key())
                .and_then(|maybe_alias| {
                    maybe_alias
                        .map(|blob| {
                            ContentAlias::from_bytes(blob.into_bytes().into())
                                .map(|alias| alias.content_id())
                        })
                        .transpose()
                })
                .right_future(),
        }
    }

    /// Fetch the alias ids for the underlying content.
    /// XXX Compute missing ones?
    /// XXX Allow caller to select which ones they're interested in?
    pub fn get_aliases(
        &self,
        ctxt: CoreContext,
        key: &FetchKey,
    ) -> impl Future<Item = Option<ContentMetadata>, Error = Error> {
        self.get_canonical_id(ctxt.clone(), key).and_then({
            cloned!(self.blobstore, ctxt);
            move |maybe_id| match maybe_id {
                None => Ok(None).into_future().left_future(),
                Some(id) => blobstore
                    .fetch(ctxt, ContentMetadataId::from(id))
                    .right_future(),
            }
        })
    }

    /// Return true if the given key exists. A successful return means the key definitely
    /// either exists or doesn't; an error means the existence could not be determined.
    pub fn exists(
        &self,
        ctxt: CoreContext,
        key: &FetchKey,
    ) -> impl Future<Item = bool, Error = Error> {
        self.get_canonical_id(ctxt.clone(), &key)
            .and_then({
                cloned!(self.blobstore, ctxt);
                move |maybe_id| maybe_id.map(|id| blobstore.is_present(ctxt, id.blobstore_key()))
            })
            .map(|exists: Option<bool>| exists.unwrap_or(false))
    }

    /// Fetch a file as a stream. This returns either success with a stream of data if the file
    ///  exists, success with None if it does not exist, or an Error if either existence can't
    /// be determined or if opening the file failed. File contents are returned in chunks
    /// configured by FilestoreConfig::read_chunk_size - this defines the max chunk size, but
    /// they may be shorter (not just the final chunks - any of them). Chunks are guaranteed to
    /// have non-zero size.
    ///
    /// XXX Just simplify the API by making "not present" an error, at the risk of inconsistency
    /// with `exists`?
    pub fn fetch(
        &self,
        ctxt: CoreContext,
        key: &FetchKey,
    ) -> impl Future<Item = Option<impl Stream<Item = Bytes, Error = Error>>, Error = Error> {
        // First fetch either the content or the alias
        self.get_canonical_id(ctxt.clone(), key)
            .and_then({
                cloned!(self.blobstore, ctxt);
                move |maybe_id| maybe_id.map(|id| blobstore.get(ctxt, id.blobstore_key()))
            })
            .and_then(|maybe_bytes|
                maybe_bytes
                .and_then(|x| x)
                 .map(|file_bytes| {
                     FileContents::from_encoded_bytes(file_bytes.into_bytes())
                     .map(FileContents::into_bytes)
                 })
                 .transpose(),
             )
            // -> Stream<Bytes> - XXX chunkify
            .map(|res: Option<Bytes>| res.map(|v| stream::once(Ok(v))))
    }

    /// Store a file from a stream. This is guaranteed atomic - either the store will succeed
    /// for the entire file, or it will fail and the file will logically not exist (however
    /// there's no guarantee that any partially written parts will be cleaned up).
    pub fn store(
        &self,
        ctxt: CoreContext,
        key: &StoreKey,
        data: impl Stream<Item = Bytes, Error = Error> + Send + 'static,
    ) -> impl Future<Item = (), Error = Error> + Send + 'static {
        // Since we don't have atomicity for puts, we need to make sure they're ordered
        // correctly:
        //
        // - compute missing hashes (trust the caller if provided)
        // - write the forward-mapping aliases
        // - write the data blob
        // - write the back-mapping blob
        //
        // Rationale for this order: since we can't guarantee the aliases are written atomically,
        // on failure we could end up writing some but not others. If the underlying blob exists
        // at that point, we've got an inconsistency. However writing the data blob is atomic,
        // and the aliases are only meaningful as references to that blob (in other words, an
        // alias referring to an absent blob is itself considered to be absent, so logically all
        // all the aliases come into existence atomically when the data blob is written).
        // Once the data blob is written we can write the back-mapping object. This is just a
        // cache, as everything in it can be computed from the content id. Therefore, in principle,
        // if it doesn't get written we can fix it up later.

        // Split the error out of the data stream so we don't need to worry about cloning it
        let (data, err) = futures_ext::split_err(data);

        // One stream for the data itself, and one for each hash format we might need
        let mut copies = futures_ext::stream_clone(data, 4).into_iter();
        let data = copies.next().unwrap();

        let sha1 = if let Some(sha1) = key.sha1 {
            future::ok::<_, !>(sha1).left_future()
        } else {
            sha1_hasher(copies.next().unwrap()).right_future()
        }
        .shared();

        let git_sha1 = if let Some(git_sha1) = key.git_sha1 {
            future::ok::<_, !>(git_sha1).left_future()
        } else {
            git_sha1_hasher(key.total_size, copies.next().unwrap()).right_future()
        }
        .shared();

        let sha256 = if let Some(sha256) = key.sha256 {
            future::ok::<_, !>(sha256).left_future()
        } else {
            sha256_hasher(copies.next().unwrap()).right_future()
        }
        .shared();

        let StoreKey {
            total_size,
            canonical,
            ..
        } = *key;

        // Join computation of various hashes to create the back-mapping
        let metadata = sha1
            .clone()
            .join3(git_sha1.clone(), sha256.clone())
            .map(move |(sha1, git_sha1, sha256)| ContentMetadata {
                total_size,
                content_id: canonical,
                sha1: Some(*sha1),
                git_sha1: Some(*git_sha1),
                sha256: Some(*sha256),
            })
            .map_err(|_| -> Error { unreachable!() });

        // Store the aliases
        let alias = ContentAlias::from_content_id(key.canonical).into_blob();

        let put_sha1 = sha1.map_err(|_| -> Error { unreachable!() }).and_then({
            cloned!(self.blobstore, alias, ctxt);
            move |sha1| blobstore.put(ctxt, FetchKey::Sha1(*sha1).blobstore_key(), alias)
        });
        let put_git_sha1 = git_sha1.map_err(|_| -> Error { unreachable!() }).and_then({
            cloned!(self.blobstore, alias, ctxt);
            move |git_sha1| blobstore.put(ctxt, FetchKey::GitSha1(*git_sha1).blobstore_key(), alias)
        });
        let put_sha256 = sha256.map_err(|_| -> Error { unreachable!() }).and_then({
            cloned!(self.blobstore, alias, ctxt);
            move |sha256| blobstore.put(ctxt, FetchKey::Sha256(*sha256).blobstore_key(), alias)
        });

        let put_aliases = put_sha1.join3(put_git_sha1, put_sha256);

        // Glom the whole stream into Filecontents for writing. Later this will
        // use chunking.
        let file_content = data
            .concat2()
            .map_err(|_| -> Error { unreachable!() })
            .map(|bytes| FileContents::Bytes(bytes));

        // Store the data
        let put_data = put_aliases.join(file_content).and_then({
            cloned!(self.blobstore, ctxt);
            move |(_, file_content)| {
                let blob = file_content.into_blob();
                blobstore.put(ctxt, canonical.blobstore_key(), BlobstoreBytes::from(blob))
            }
        });

        // Store the metadata
        let put_metadata = put_data.join(metadata).and_then({
            cloned!(self.blobstore, ctxt);
            move |((), metadata)| {
                let blob = metadata.into_blob();
                let key = ContentMetadataId::from(canonical);
                blobstore.put(ctxt, key.blobstore_key(), BlobstoreBytes::from(blob))
            }
        });

        // Reunite result with the error
        put_metadata
            .select(err.map(|_| -> () { unreachable!() }))
            .map(|(res, _)| res)
            .map_err(|(err, _)| err)
    }
}
