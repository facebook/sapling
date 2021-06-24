/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Ephemeral Blobstore

use std::sync::Arc;

use anyhow::Result;
use blobstore::{Blobstore, BlobstoreKeySource, BlobstorePutOps};
use chrono::Duration as ChronoDuration;
use mononoke_types::{DateTime, RepositoryId, Timestamp};
use sql::queries;
use sql_ext::SqlConnections;

use crate::bubble::{Bubble, BubbleId};
use crate::error::EphemeralBlobstoreError;
use crate::repo::RepoEphemeralBlobstore;

/// Trait alias for the combination of traits that ephemeral blobstore backing
/// blobstores must implement.
pub trait BackingBlobstore: Blobstore + BlobstorePutOps + BlobstoreKeySource {}

impl<T: Blobstore + BlobstorePutOps + BlobstoreKeySource> BackingBlobstore for T {}

/// Ephemeral Blobstore.
pub struct EphemeralBlobstoreInner {
    /// The backing blobstore where blobs are stored.
    pub(crate) blobstore: Arc<dyn BackingBlobstore>,

    /// Database used to manage the ephemeral blobstore.
    pub(crate) connections: SqlConnections,

    /// Initial value of the lifespan for bubbles in this store, i.e. the
    /// amount of time they last from either the call to create or the last
    /// call to extend_lifespan.
    pub(crate) initial_bubble_lifespan: ChronoDuration,

    /// Grace period after bubbles expire during which requests which have
    /// already opened a bubble can continue to access them.  The bubble
    /// contents will not be deleted until after the grace period.
    pub(crate) bubble_expiration_grace: ChronoDuration,
}

/// Ephemeral Blobstore.
#[derive(Clone)]
pub struct EphemeralBlobstore {
    pub(crate) inner: Arc<EphemeralBlobstoreInner>,
}

queries! {
    write CreateBubble(
        created_at: Timestamp,
        expires_at: Timestamp,
        owner_identity: Option<&str>,
    ) {
        none,
        "INSERT INTO ephemeral_bubbles (created_at, expires_at, owner_identity)
         VALUES ({created_at}, {expires_at}, {owner_identity})"
    }

    read SelectBubbleById(
        id: BubbleId,
    ) -> (Timestamp, bool, Option<String>) {
        "SELECT expires_at, expired, owner_identity FROM ephemeral_bubbles
         WHERE id = {id}"
    }
}

impl EphemeralBlobstore {
    pub(crate) fn new(
        connections: SqlConnections,
        blobstore: Arc<dyn BackingBlobstore>,
        initial_bubble_lifespan: ChronoDuration,
        bubble_expiration_grace: ChronoDuration,
    ) -> Self {
        EphemeralBlobstore {
            inner: Arc::new(EphemeralBlobstoreInner {
                blobstore,
                connections,
                initial_bubble_lifespan,
                bubble_expiration_grace,
            }),
        }
    }

    pub fn for_repo(&self, repo_id: RepositoryId) -> RepoEphemeralBlobstore {
        RepoEphemeralBlobstore::new(repo_id, self.clone())
    }

    pub(crate) async fn create_bubble(&self, repo_id: RepositoryId) -> Result<Bubble> {
        let created_at = DateTime::now();
        let expires_at = created_at + self.inner.initial_bubble_lifespan;

        let res = CreateBubble::query(
            &self.inner.connections.write_connection,
            &Timestamp::from(created_at),
            &Timestamp::from(expires_at),
            &None,
        )
        .await?;

        match res.last_insert_id() {
            Some(id) if res.affected_rows() == 1 => {
                let bubble_id = BubbleId::new(id);
                Ok(Bubble::new(
                    repo_id,
                    bubble_id,
                    expires_at + self.inner.bubble_expiration_grace,
                    self.clone(),
                ))
            }
            _ => Err(EphemeralBlobstoreError::CreateBubbleFailed.into()),
        }
    }

    pub(crate) async fn open_bubble(
        &self,
        repo_id: RepositoryId,
        bubble_id: BubbleId,
    ) -> Result<Bubble> {
        let rows =
            SelectBubbleById::query(&self.inner.connections.read_connection, &bubble_id).await?;

        if rows.is_empty() {
            return Err(EphemeralBlobstoreError::NoSuchBubble(bubble_id).into());
        }

        // TODO(mbthomas): check owner_identity
        let (expires_at, expired, ref _owner_identity) = rows[0];
        let expires_at: DateTime = expires_at.into();
        if expired || expires_at < DateTime::now() {
            return Err(EphemeralBlobstoreError::NoSuchBubble(bubble_id).into());
        }

        Ok(Bubble::new(
            repo_id,
            bubble_id,
            expires_at + self.inner.bubble_expiration_grace,
            self.clone(),
        ))
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::builder::EphemeralBlobstoreBuilder;
    use blobstore::{BlobstoreBytes, BlobstoreKeyParam};
    use context::CoreContext;
    use fbinit::FacebookInit;
    use maplit::hashset;
    use memblob::Memblob;
    use metaconfig_types::PackFormat;
    use mononoke_types_mocks::repo::REPO_ZERO;
    use packblob::PackBlob;
    use sql_construct::SqlConstruct;

    #[fbinit::test]
    async fn basic_test(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        // The ephemeral blobstore will normally be used stacked on top of
        // packblob, so use this in the test, too.
        let blobstore = Arc::new(PackBlob::new(
            Memblob::default(),
            PackFormat::ZstdIndividual(0),
        ));
        let eph = EphemeralBlobstoreBuilder::with_sqlite_in_memory()?.build(
            blobstore.clone(),
            ChronoDuration::days(30),
            ChronoDuration::hours(6),
        );

        // Create a bubble and put data in it.
        let bubble1 = eph.create_bubble(REPO_ZERO).await?;
        bubble1
            .put(&ctx, "test_key", BlobstoreBytes::from_bytes("test data"))
            .await?;
        let data = bubble1.get(&ctx, "test_key").await?.unwrap().into_bytes();
        assert_eq!(data.as_bytes().as_ref(), b"test data");

        // Re-open the bubble and confirm we can read the data.
        let bubble1_id = bubble1.bubble_id();
        let bubble1_read = eph.open_bubble(REPO_ZERO, bubble1_id).await?;
        let data = bubble1_read
            .get(&ctx, "test_key")
            .await?
            .unwrap()
            .into_bytes();
        assert_eq!(data.as_bytes().as_ref(), b"test data");

        // Enumerate the blobstore and check the key got its prefix.
        let enumerated = blobstore
            .enumerate(&ctx, &BlobstoreKeyParam::from(..))
            .await?;
        assert_eq!(
            enumerated.keys,
            hashset! { format!("eph{}.repo0000.test_key", bubble1_id) }
        );

        // Create a new bubble and put data in it.
        let bubble2 = eph.create_bubble(REPO_ZERO).await?;
        bubble2
            .put(
                &ctx,
                "test_key",
                BlobstoreBytes::from_bytes("other test data"),
            )
            .await?;
        let data = bubble2.get(&ctx, "test_key").await?.unwrap().into_bytes();
        assert_eq!(data.as_bytes().as_ref(), b"other test data");

        let data = bubble1.get(&ctx, "test_key").await?.unwrap().into_bytes();
        assert_eq!(data.as_bytes().as_ref(), b"test data");

        // There should now be two separate keys.
        let bubble2_id = bubble2.bubble_id();
        let enumerated = blobstore
            .enumerate(&ctx, &BlobstoreKeyParam::from(..))
            .await?;
        assert_eq!(
            enumerated.keys,
            hashset! {
                format!("eph{}.repo0000.test_key", bubble1_id),
                format!("eph{}.repo0000.test_key", bubble2_id),
            }
        );
        Ok(())
    }
}
