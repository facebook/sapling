/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Context;
use anyhow::Result;
use async_trait::async_trait;
use blobstore::Blobstore;
use blobstore::Loadable;
use blobstore::LoadableError;
use blobstore::Storable;
use context::CoreContext;
use edenapi_types::AnyFileContentId;
use mononoke_types::errors::ErrorKind;
use mononoke_types::hash;
use mononoke_types::BlobstoreKey;
use mononoke_types::ContentAlias;
use mononoke_types::ContentId;

/// Key for fetching - we can access with any of the supported key types
#[derive(Debug, Copy, Clone)]
pub enum FetchKey {
    Canonical(ContentId),
    Aliased(Alias),
}

impl From<ContentId> for FetchKey {
    fn from(content_id: ContentId) -> Self {
        FetchKey::Canonical(content_id)
    }
}

impl From<Alias> for FetchKey {
    fn from(alias: Alias) -> Self {
        FetchKey::Aliased(alias)
    }
}

impl From<hash::Sha256> for FetchKey {
    fn from(hash: hash::Sha256) -> Self {
        FetchKey::Aliased(Alias::Sha256(hash))
    }
}

impl From<hash::Sha1> for FetchKey {
    fn from(hash: hash::Sha1) -> Self {
        FetchKey::Aliased(Alias::Sha1(hash))
    }
}

impl From<AnyFileContentId> for FetchKey {
    fn from(id: AnyFileContentId) -> Self {
        match id {
            AnyFileContentId::ContentId(id) => Self::from(ContentId::from(id)),
            AnyFileContentId::Sha1(id) => Self::from(hash::Sha1::from(id)),
            AnyFileContentId::Sha256(id) => Self::from(hash::Sha256::from(id)),
        }
    }
}

impl FetchKey {
    pub fn blobstore_key(&self) -> String {
        match self {
            Self::Canonical(cid) => cid.blobstore_key(),
            Self::Aliased(alias) => alias.blobstore_key(),
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum Alias {
    Sha1(hash::Sha1),
    Sha256(hash::Sha256),
    GitSha1(hash::GitSha1),
}

#[async_trait]
impl Loadable for FetchKey {
    type Value = ContentId;

    /// Return the canonical ID for a key. It doesn't check if the corresponding content actually
    /// exists:
    /// - When called with content_id, it doesn't check the content id is stored in the blobstore
    /// - It is possible for an alias to exist before the ID if there was an interrupted store
    /// operation.
    async fn load<'a, B: Blobstore>(
        &'a self,
        ctx: &'a CoreContext,
        blobstore: &'a B,
    ) -> Result<Self::Value, LoadableError> {
        match self {
            FetchKey::Canonical(content_id) => Ok(*content_id),
            FetchKey::Aliased(alias) => alias.load(ctx, blobstore).await,
        }
    }
}

impl Alias {
    pub fn blobstore_key(&self) -> String {
        match self {
            Alias::GitSha1(git_sha1) => format!("alias.gitsha1.{}", git_sha1.to_hex()),
            Alias::Sha1(sha1) => format!("alias.sha1.{}", sha1.to_hex()),
            Alias::Sha256(sha256) => format!("alias.sha256.{}", sha256.to_hex()),
        }
    }

    #[inline]
    pub fn sampling_fingerprint(&self) -> u64 {
        match self {
            Alias::GitSha1(git_sha1) => git_sha1.sampling_fingerprint(),
            Alias::Sha1(sha1) => sha1.sampling_fingerprint(),
            Alias::Sha256(sha256) => sha256.sampling_fingerprint(),
        }
    }
}

#[async_trait]
impl Loadable for Alias {
    type Value = ContentId;

    async fn load<'a, B: Blobstore>(
        &'a self,
        ctx: &'a CoreContext,
        blobstore: &'a B,
    ) -> Result<Self::Value, LoadableError> {
        let key = self.blobstore_key();
        let get = blobstore.get(ctx, &key);
        let maybe_alias = get.await?;
        let blob = maybe_alias.ok_or_else(|| LoadableError::Missing(key.clone()))?;

        ContentAlias::from_bytes(blob.into_raw_bytes())
            .map(|alias| alias.content_id())
            .with_context(|| ErrorKind::BlobKeyError(key.clone()))
            .map_err(LoadableError::Error)
    }
}

pub struct AliasBlob(pub Alias, pub ContentAlias);

#[async_trait]
impl Storable for AliasBlob {
    type Key = ();

    async fn store<'a, B: Blobstore>(
        self,
        ctx: &'a CoreContext,
        blobstore: &'a B,
    ) -> Result<Self::Key> {
        blobstore
            .put(ctx, self.0.blobstore_key(), self.1.into_blob())
            .await
    }
}
