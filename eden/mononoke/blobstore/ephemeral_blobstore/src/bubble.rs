/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Ephemeral Blobstore Bubbles

use std::fmt;
use std::num::NonZeroU64;
use std::sync::Arc;

use anyhow::Result;
use blobstore::{Blobstore, BlobstoreBytes, BlobstoreGetData, BlobstoreIsPresent};
use context::CoreContext;
use mononoke_types::repo::{EPH_ID_PREFIX, EPH_ID_SUFFIX};
use mononoke_types::{DateTime, RepositoryId};
use prefixblob::PrefixBlobstore;
use sql::mysql_async::prelude::{ConvIr, FromValue};
use sql::mysql_async::{FromValueError, Value};

use crate::error::EphemeralBlobstoreError;
use crate::handle::EphemeralHandle;
use crate::store::EphemeralBlobstore;

/// Ephemeral Blobstore Bubble ID.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct BubbleId(NonZeroU64);

impl fmt::Display for BubbleId {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        self.0.fmt(fmt)
    }
}

impl From<BubbleId> for Value {
    fn from(bubble_id: BubbleId) -> Self {
        Value::UInt(bubble_id.0.into())
    }
}

impl From<BubbleId> for NonZeroU64 {
    fn from(bubble_id: BubbleId) -> Self {
        bubble_id.0
    }
}

impl ConvIr<BubbleId> for BubbleId {
    fn new(v: Value) -> Result<Self, FromValueError> {
        match v {
            Value::UInt(id) => match NonZeroU64::new(id) {
                Some(id) => Ok(BubbleId(id)),
                None => Err(FromValueError(v))?,
            },
            v => Err(FromValueError(v)),
        }
    }

    fn commit(self) -> BubbleId {
        self
    }

    fn rollback(self) -> Value {
        self.into()
    }
}

impl FromValue for BubbleId {
    type Intermediate = BubbleId;
}

impl BubbleId {
    pub(crate) fn new(id: NonZeroU64) -> Self {
        BubbleId(id)
    }

    /// Generate the blobstore prefix for this bubble.
    fn prefix(&self) -> String {
        format!("{}{}{}", EPH_ID_PREFIX, self.0, EPH_ID_SUFFIX,)
    }
}

/// An opened ephemeral blobstore bubble.  This is a miniature blobstore
/// that stores blobs just for this ephemeral bubble in a particular repo.
#[derive(Debug, Clone)]
pub struct Bubble {
    /// ID of the repository this bubble applies to.
    repo_id: RepositoryId,

    /// ID of the current bubble.
    bubble_id: BubbleId,

    /// Expiration time.  After this time, the bubble no longer exists.
    /// This includes the grace period from the ephemeral blobstore.
    expires_at: DateTime,

    /// Blobstore to use for accessing blobs in this bubble.
    blobstore: Arc<dyn Blobstore>,

    /// Ephemeral blobstore this bubble is located in.
    #[allow(unused)] // will be used to extend bubble lifespan
    ephemeral_blobstore: EphemeralBlobstore,
}

impl fmt::Display for Bubble {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        write!(fmt, "Bubble({})<{}>", self.bubble_id, self.blobstore)
    }
}

impl Bubble {
    pub(crate) fn new(
        repo_id: RepositoryId,
        bubble_id: BubbleId,
        expires_at: DateTime,
        ephemeral_blobstore: EphemeralBlobstore,
    ) -> Self {
        // Construct a blobstore for accessing blobs within this bubble.
        // TODO(mbthomas): add redaction
        let prefix = format!("{}{}", bubble_id.prefix(), repo_id.prefix());
        let blobstore = Arc::new(PrefixBlobstore::new(
            ephemeral_blobstore.inner.blobstore.clone(),
            prefix,
        ));

        Self {
            repo_id,
            bubble_id,
            expires_at,
            blobstore,
            ephemeral_blobstore,
        }
    }

    fn check_unexpired(&self) -> Result<()> {
        if self.expires_at >= DateTime::now() {
            Ok(())
        } else {
            Err(EphemeralBlobstoreError::BubbleExpired(self.bubble_id).into())
        }
    }

    pub fn repo_id(&self) -> RepositoryId {
        self.repo_id
    }

    pub fn bubble_id(&self) -> BubbleId {
        self.bubble_id
    }

    pub fn get_handle(&self, main_blobstore: Arc<dyn Blobstore>) -> EphemeralHandle {
        EphemeralHandle::new(self.clone(), main_blobstore)
    }

    pub async fn extend_lifespan(&self) -> Result<()> {
        unimplemented!()
    }

    pub(crate) async fn get(
        &self,
        ctx: &CoreContext,
        key: &str,
    ) -> Result<Option<BlobstoreGetData>> {
        self.check_unexpired()?;
        self.blobstore.get(ctx, key).await
    }

    pub(crate) async fn put(
        &self,
        ctx: &CoreContext,
        key: String,
        value: BlobstoreBytes,
    ) -> Result<()> {
        self.check_unexpired()?;
        self.blobstore.put(ctx, key, value).await
    }

    pub(crate) async fn is_present(
        &self,
        ctx: &CoreContext,
        key: &str,
    ) -> Result<BlobstoreIsPresent> {
        self.check_unexpired()?;
        self.blobstore.is_present(ctx, key).await
    }
}
