/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Ephemeral Blobstore Bubbles

use std::fmt;
use std::num::NonZeroU64;
use std::str::FromStr;
use std::sync::Arc;

use anyhow::Result;
use async_stream::try_stream;
use blobstore::Blobstore;
use blobstore::BlobstoreBytes;
use blobstore::BlobstoreEnumerableWithUnlink;
use blobstore::BlobstoreGetData;
use blobstore::BlobstoreIsPresent;
use blobstore::BlobstoreKeyParam;
use blobstore::BlobstoreKeySource;
use blobstore::BlobstoreUnlinkOps;
use changesets::ChangesetsArc;
use context::CoreContext;
use derivative::Derivative;
use futures::future::try_join_all;
use futures::pin_mut;
use futures::stream::TryStreamExt;
use futures::Stream;
use mononoke_types::repo::EPH_ID_PREFIX;
use mononoke_types::repo::EPH_ID_SUFFIX;
use mononoke_types::DateTime;
use prefixblob::PrefixBlobstore;
use repo_blobstore::RepoBlobstore;
use repo_blobstore::RepoBlobstoreRef;
use repo_identity::RepoIdentityArc;
use repo_identity::RepoIdentityRef;
use sql::mysql_async::prelude::ConvIr;
use sql::mysql_async::prelude::FromValue;
use sql::mysql_async::FromValueError;
use sql::mysql_async::Value;
use sql::sql_common::mysql::opt_try_from_rowfield;
use sql::sql_common::mysql::OptionalTryFromRowField;
use sql::sql_common::mysql::RowField;
use sql::sql_common::mysql::ValueError;
use sql_ext::SqlConnections;

use crate::changesets::EphemeralChangesets;
use crate::error::EphemeralBlobstoreError;
use crate::handle::EphemeralHandle;
use crate::view::EphemeralRepoView;

pub enum StorageLocation {
    // This is not ephemeral
    Persistent,
    // This is ephemeral, but its bubble is not yet known
    UnknownBubble,
    // This is ephemeral and located at given bubble
    Bubble(BubbleId),
}

impl From<BubbleId> for StorageLocation {
    fn from(bubble_id: BubbleId) -> Self {
        Self::Bubble(bubble_id)
    }
}

impl StorageLocation {
    pub fn ephemeral(maybe_bubble: Option<BubbleId>) -> Self {
        match maybe_bubble {
            None => Self::UnknownBubble,
            Some(id) => Self::Bubble(id),
        }
    }
}

/// Ephemeral Blobstore Bubble ID.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
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
        let from_u64 = |id, v| match NonZeroU64::new(id) {
            Some(id) => Ok(BubbleId(id)),
            None => Err(FromValueError(v)),
        };
        match v {
            Value::UInt(id) => from_u64(id, v),
            Value::Int(id) => from_u64(id as u64, v),
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

impl OptionalTryFromRowField for BubbleId {
    fn try_from_opt(field: RowField) -> Result<Option<Self>, ValueError> {
        opt_try_from_rowfield(field)
    }
}

impl TryFrom<i64> for BubbleId {
    type Error = ();

    fn try_from(value: i64) -> Result<Self, Self::Error> {
        u64::try_from(value)
            .ok()
            .and_then(NonZeroU64::new)
            .map(BubbleId::new)
            .ok_or(())
    }
}

impl FromStr for BubbleId {
    type Err = <NonZeroU64 as FromStr>::Err;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        NonZeroU64::from_str(s).map(Self)
    }
}

impl BubbleId {
    pub fn new(id: NonZeroU64) -> Self {
        BubbleId(id)
    }

    /// Generate the blobstore prefix for this bubble.
    pub fn prefix(&self) -> String {
        format!("{}{}{}", EPH_ID_PREFIX, self.0, EPH_ID_SUFFIX,)
    }
}

type RawBubbleBlobstore = PrefixBlobstore<Arc<dyn BlobstoreEnumerableWithUnlink>>;

/// Enum representing the expiry status of a bubble in the backing store.
#[derive(Copy, Debug, Clone, PartialEq)]
pub enum ExpiryStatus {
    Active = 0,
    Expired = 1,
}

impl fmt::Display for ExpiryStatus {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            ExpiryStatus::Expired => write!(fmt, "Expired"),
            ExpiryStatus::Active => write!(fmt, "Active"),
        }
    }
}

impl From<ExpiryStatus> for Value {
    fn from(status: ExpiryStatus) -> Self {
        let val = match status {
            ExpiryStatus::Expired => 1,
            ExpiryStatus::Active => 0,
        };
        Value::Int(val)
    }
}

impl ConvIr<ExpiryStatus> for ExpiryStatus {
    fn new(v: Value) -> Result<Self, FromValueError> {
        match v {
            Value::Int(1) => Ok(ExpiryStatus::Expired),
            Value::Int(0) => Ok(ExpiryStatus::Active),
            v => Err(FromValueError(v)),
        }
    }

    fn commit(self) -> ExpiryStatus {
        self
    }

    fn rollback(self) -> Value {
        self.into()
    }
}

impl FromValue for ExpiryStatus {
    type Intermediate = ExpiryStatus;
}

impl OptionalTryFromRowField for ExpiryStatus {
    fn try_from_opt(field: RowField) -> Result<Option<Self>, ValueError> {
        opt_try_from_rowfield(field)
    }
}

impl TryFrom<i64> for ExpiryStatus {
    type Error = ();

    fn try_from(value: i64) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(ExpiryStatus::Expired),
            0 => Ok(ExpiryStatus::Active),
            _ => Err(()),
        }
    }
}

/// An opened ephemeral blobstore bubble.  This is a miniature blobstore
/// that stores blobs just for this ephemeral bubble in a particular repo.
#[derive(Derivative, Clone)]
#[derivative(Debug)]
pub struct Bubble {
    /// ID of the current bubble.
    bubble_id: BubbleId,

    /// Expiration time.  After this time, the bubble no longer exists.
    /// This includes the grace period from the ephemeral blobstore.
    expires_at: DateTime,

    /// Blobstore to use for accessing blobs in this bubble, without redaction
    /// or repo prefix wrappers.
    pub(crate) blobstore: RawBubbleBlobstore,

    expired: ExpiryStatus,

    /// SQL connection
    #[derivative(Debug = "ignore")]
    connections: SqlConnections,
}

impl fmt::Display for Bubble {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        write!(fmt, "Bubble({})<{}>", self.bubble_id, self.blobstore)
    }
}

impl Bubble {
    pub(crate) fn new(
        bubble_id: BubbleId,
        expires_at: DateTime,
        blobstore: Arc<dyn BlobstoreEnumerableWithUnlink>,
        connections: SqlConnections,
        expired: ExpiryStatus,
    ) -> Self {
        let blobstore = PrefixBlobstore::new(blobstore, bubble_id.prefix());

        Self {
            bubble_id,
            expires_at,
            blobstore,
            connections,
            expired,
        }
    }

    pub(crate) fn check_unexpired(&self) -> Result<()> {
        if self.expires_at >= DateTime::now() && self.expired != ExpiryStatus::Expired {
            Ok(())
        } else {
            Err(EphemeralBlobstoreError::BubbleExpired(self.bubble_id).into())
        }
    }

    pub(crate) async fn delete_blobs_in_bubble(&self, ctx: &CoreContext) -> Result<usize> {
        let key_stream = self.get_keys_in_bubble(ctx, None).await;
        let mut keys_deleted = 0;
        pin_mut!(key_stream);
        while let Some(keys) = key_stream.try_next().await? {
            // As long as the unlink operation on underlying blobstores is "truly" async
            // (i.e. they yield on cross service I/O), the below will execute the unlinking
            // concurrently and terminate early on the first error encountered.
            let unlinked_keys =
                try_join_all(keys.iter().map(|key| self.blobstore.unlink(ctx, key))).await?;
            keys_deleted += unlinked_keys.len();
        }
        // If the unlinking of all blobs within the bubble was successful, return the
        // number of blobs unlinked.
        Ok(keys_deleted)
    }

    pub(crate) async fn keys_in_bubble(
        &self,
        ctx: &CoreContext,
        start_from: Option<String>,
        limit: u32,
    ) -> Result<Vec<String>> {
        let key_stream = self.get_keys_in_bubble(ctx, start_from).await;
        let mut collected_keys = vec![];
        let limit = limit.try_into().unwrap();
        pin_mut!(key_stream);
        // Executing the below sequentially since we want to maintain
        // the ordering of the elements returned. Plus, we want to exit
        // as soon as we have the required number of keys.
        while let Some(keys) = key_stream.try_next().await? {
            collected_keys.extend(keys);
            if collected_keys.len() >= limit {
                break;
            }
        }
        // In cases where limit % batch_size != 0, we would have fetched
        // more than required, trim the extra keys.
        collected_keys.truncate(limit);
        Ok(collected_keys)
    }

    async fn get_keys_in_bubble<'a>(
        &'a self,
        ctx: &'a CoreContext,
        start_from: Option<String>,
    ) -> impl Stream<Item = Result<Vec<String>>> + 'a {
        let search_range = match start_from {
            Some(start) => BlobstoreKeyParam::from(start..),
            None => BlobstoreKeyParam::from(..),
        };
        let mut token = Arc::new(search_range);
        try_stream! {
            loop {
                let result = self.blobstore.enumerate(ctx, &token).await?;
                yield Vec::from_iter(result.keys);
                token = match result.next_token {
                    Some(next_token) => Arc::new(next_token),
                    None => break,
                };
            }
        }
    }

    pub fn bubble_id(&self) -> BubbleId {
        self.bubble_id
    }

    pub fn expired(&self) -> ExpiryStatus {
        self.expired
    }

    pub fn expires_at(&self) -> DateTime {
        self.expires_at
    }

    /// Return a blobstore that gives priority to accessing the bubble, but falls back
    /// to the main blobstore.
    pub fn wrap_repo_blobstore(&self, main_blobstore: RepoBlobstore) -> RepoBlobstore {
        // Repo prefix/redaction is added only once by RepoBlobstore
        RepoBlobstore::new_with_wrapped_inner_blobstore(main_blobstore, |bs| {
            Arc::new(EphemeralHandle::new(self.clone(), bs))
        })
    }

    fn changesets_with_blobstore(
        &self,
        repo_blobstore: RepoBlobstore,
        container: impl ChangesetsArc + RepoIdentityRef,
    ) -> EphemeralChangesets {
        EphemeralChangesets::new(
            container.repo_identity().id(),
            self.bubble_id(),
            repo_blobstore,
            self.connections.clone(),
            container.changesets_arc(),
        )
    }

    pub fn changesets(
        &self,
        container: impl ChangesetsArc + RepoIdentityRef + RepoBlobstoreRef,
    ) -> EphemeralChangesets {
        let repo_blobstore = self.wrap_repo_blobstore(container.repo_blobstore().clone());
        self.changesets_with_blobstore(repo_blobstore, container)
    }

    pub fn repo_view(
        &self,
        container: impl RepoBlobstoreRef + RepoIdentityRef + RepoIdentityArc + ChangesetsArc,
    ) -> EphemeralRepoView {
        let repo_blobstore = self.wrap_repo_blobstore(container.repo_blobstore().clone());
        let repo_identity = container.repo_identity_arc();
        EphemeralRepoView {
            repo_blobstore: Arc::new(repo_blobstore.clone()),
            changesets: Arc::new(self.changesets_with_blobstore(repo_blobstore, container)),
            repo_identity,
        }
    }

    pub async fn extend_lifespan(&self) -> Result<()> {
        unimplemented!()
    }
}

// These blobstore methods are not to be used directly as they bypass redaction.
// Instead use .wrap_repo_blobstore
impl Bubble {
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
