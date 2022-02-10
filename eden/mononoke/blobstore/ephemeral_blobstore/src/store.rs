/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Ephemeral Store

use std::sync::Arc;

use anyhow::Result;
use blobstore::Blobstore;
use chrono::Duration as ChronoDuration;
use derivative::Derivative;
use mononoke_types::{ChangesetId, DateTime, RepositoryId, Timestamp};
use sql::queries;
use sql_ext::SqlConnections;
use std::time::Duration;

use crate::bubble::{Bubble, BubbleId};
use crate::error::EphemeralBlobstoreError;

/// Ephemeral Store.
#[derive(Derivative)]
#[derivative(Debug)]
struct RepoEphemeralStoreInner {
    /// The backing blobstore where blobs are stored, without any redaction
    /// or repo prefix wrappers.
    pub(crate) blobstore: Arc<dyn Blobstore>,

    #[derivative(Debug = "ignore")]
    /// Database used to manage the ephemeral store.
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

/// Ephemeral Store
#[facet::facet]
#[derive(Debug, Clone)]
pub struct RepoEphemeralStore {
    /// Repo this belongs to
    repo_id: RepositoryId,

    inner: Option<Arc<RepoEphemeralStoreInner>>,
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

    read SelectBubbleFromChangeset(
        repo_id: RepositoryId,
        cs_id: ChangesetId,
    ) -> (BubbleId,) {
        "SELECT bubble_id
        FROM ephemeral_bubble_changeset_mapping
        WHERE repo_id = {repo_id} AND cs_id = {cs_id}"
    }
}

// Approximating a big duration to max chrono duration (~10^11 years) is good enough
fn to_chrono(duration: Duration) -> ChronoDuration {
    ChronoDuration::from_std(duration).unwrap_or_else(|_| ChronoDuration::max_value())
}

impl RepoEphemeralStoreInner {
    async fn create_bubble(&self, custom_duration: Option<Duration>) -> Result<Bubble> {
        let created_at = DateTime::now();
        let duration = match custom_duration {
            None => self.initial_bubble_lifespan,
            Some(duration) => to_chrono(duration),
        };
        let expires_at = created_at + duration;

        let res = CreateBubble::query(
            &self.connections.write_connection,
            &Timestamp::from(created_at),
            &Timestamp::from(expires_at),
            &None,
        )
        .await?;

        match res.last_insert_id() {
            Some(id) if res.affected_rows() == 1 => {
                let bubble_id = BubbleId::new(
                    std::num::NonZeroU64::new(id)
                        .ok_or(EphemeralBlobstoreError::CreateBubbleFailed)?,
                );
                Ok(Bubble::new(
                    bubble_id,
                    expires_at + self.bubble_expiration_grace,
                    self.blobstore.clone(),
                    self.connections.clone(),
                ))
            }
            _ => Err(EphemeralBlobstoreError::CreateBubbleFailed.into()),
        }
    }

    async fn bubble_from_changeset(
        &self,
        repo_id: &RepositoryId,
        cs_id: &ChangesetId,
    ) -> Result<Option<BubbleId>> {
        let rows =
            SelectBubbleFromChangeset::query(&self.connections.read_connection, &repo_id, &cs_id)
                .await?;
        Ok(rows.into_iter().next().map(|b| b.0))
    }

    async fn open_bubble(&self, bubble_id: BubbleId) -> Result<Bubble> {
        let mut rows =
            SelectBubbleById::query(&self.connections.read_connection, &bubble_id).await?;

        if rows.is_empty() {
            // Perhaps the bubble hasn't showed up yet due to replication lag.
            // Let's retry on master just in case.
            rows = SelectBubbleById::query(&self.connections.read_master_connection, &bubble_id)
                .await?;
            if rows.is_empty() {
                return Err(EphemeralBlobstoreError::NoSuchBubble(bubble_id).into());
            }
        }

        // TODO(mbthomas): check owner_identity
        let (expires_at, expired, ref _owner_identity) = rows[0];
        let expires_at: DateTime = expires_at.into();
        if expired || expires_at < DateTime::now() {
            return Err(EphemeralBlobstoreError::NoSuchBubble(bubble_id).into());
        }

        Ok(Bubble::new(
            bubble_id,
            expires_at + self.bubble_expiration_grace,
            self.blobstore.clone(),
            self.connections.clone(),
        ))
    }
}

impl RepoEphemeralStore {
    pub(crate) fn new(
        repo_id: RepositoryId,
        connections: SqlConnections,
        blobstore: Arc<dyn Blobstore>,
        initial_bubble_lifespan: Duration,
        bubble_expiration_grace: Duration,
    ) -> Self {
        Self {
            repo_id,
            inner: Some(Arc::new(RepoEphemeralStoreInner {
                blobstore,
                connections,
                initial_bubble_lifespan: to_chrono(initial_bubble_lifespan),
                bubble_expiration_grace: to_chrono(bubble_expiration_grace),
            })),
        }
    }

    pub fn disabled(repo_id: RepositoryId) -> Self {
        Self {
            inner: None,
            repo_id,
        }
    }

    fn inner(&self) -> Result<&RepoEphemeralStoreInner, EphemeralBlobstoreError> {
        self.inner
            .as_deref()
            .ok_or_else(|| EphemeralBlobstoreError::NoEphemeralBlobstore(self.repo_id))
    }

    pub async fn create_bubble(&self, custom_duration: Option<Duration>) -> Result<Bubble> {
        self.inner()?.create_bubble(custom_duration).await
    }

    pub async fn open_bubble(&self, bubble_id: BubbleId) -> Result<Bubble> {
        self.inner()?.open_bubble(bubble_id).await
    }

    pub async fn bubble_from_changeset(&self, cs_id: &ChangesetId) -> Result<Option<BubbleId>> {
        self.inner()?
            .bubble_from_changeset(&self.repo_id, cs_id)
            .await
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::builder::RepoEphemeralStoreBuilder;
    use blobstore::{BlobstoreBytes, BlobstoreKeyParam, BlobstoreKeySource};
    use context::CoreContext;
    use fbinit::FacebookInit;
    use maplit::hashset;
    use memblob::Memblob;
    use metaconfig_types::PackFormat;
    use mononoke_types_mocks::repo::REPO_ZERO;
    use packblob::PackBlob;
    use repo_blobstore::RepoBlobstore;
    use scuba_ext::MononokeScubaSampleBuilder;
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
        let repo_blobstore = RepoBlobstore::new(
            Arc::new(Memblob::default()),
            None,
            REPO_ZERO,
            MononokeScubaSampleBuilder::with_discard(),
        );
        let eph = RepoEphemeralStoreBuilder::with_sqlite_in_memory()?.build(
            REPO_ZERO,
            blobstore.clone(),
            Duration::from_secs(30 * 24 * 60 * 60),
            Duration::from_secs(6 * 60 * 60),
        );
        let key = "test_key".to_string();

        // Create a bubble and put data in it.
        let bubble1 = eph.create_bubble(None).await?;
        let bubble1_id = bubble1.bubble_id();
        let bubble1 = bubble1.wrap_repo_blobstore(repo_blobstore.clone());
        bubble1
            .put(&ctx, key.clone(), BlobstoreBytes::from_bytes("test data"))
            .await?;
        let data = bubble1.get(&ctx, &key).await?.unwrap().into_bytes();
        assert_eq!(data.as_bytes().as_ref(), b"test data");

        // Re-open the bubble and confirm we can read the data.
        let bubble1_read = eph
            .open_bubble(bubble1_id)
            .await?
            .wrap_repo_blobstore(repo_blobstore.clone());
        let data = bubble1_read.get(&ctx, &key).await?.unwrap().into_bytes();
        assert_eq!(data.as_bytes().as_ref(), b"test data");

        // Enumerate the blobstore and check the key got its prefix.
        let enumerated = blobstore
            .enumerate(&ctx, &BlobstoreKeyParam::from(..))
            .await?;
        assert_eq!(
            enumerated.keys,
            hashset! { format!("eph{}.repo0000.{}", bubble1_id, key) }
        );

        // Create a new bubble and put data in it.
        let bubble2 = eph.create_bubble(None).await?;
        let bubble2_id = bubble2.bubble_id();
        let bubble2 = bubble2.wrap_repo_blobstore(repo_blobstore.clone());
        bubble2
            .put(
                &ctx,
                key.clone(),
                BlobstoreBytes::from_bytes("other test data"),
            )
            .await?;
        let data = bubble2.get(&ctx, &key).await?.unwrap().into_bytes();
        assert_eq!(data.as_bytes().as_ref(), b"other test data");

        let data = bubble1.get(&ctx, &key).await?.unwrap().into_bytes();
        assert_eq!(data.as_bytes().as_ref(), b"test data");

        // There should now be two separate keys.
        let enumerated = blobstore
            .enumerate(&ctx, &BlobstoreKeyParam::from(..))
            .await?;
        assert_eq!(
            enumerated.keys,
            hashset! {
                format!("eph{}.repo0000.{}", bubble1_id, key),
                format!("eph{}.repo0000.{}", bubble2_id, key),
            }
        );
        Ok(())
    }
}
