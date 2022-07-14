/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Ephemeral Store

use std::sync::Arc;

use anyhow::Result;
use blobstore::BlobstoreEnumerableWithUnlink;
use chrono::Duration as ChronoDuration;
use context::CoreContext;
use derivative::Derivative;
use metaconfig_types::BubbleDeletionMode;
use mononoke_types::ChangesetId;
use mononoke_types::DateTime;
use mononoke_types::RepositoryId;
use mononoke_types::Timestamp;
use sql::queries;
use sql_ext::SqlConnections;
use std::time::Duration;

use crate::bubble::Bubble;
use crate::bubble::BubbleId;
use crate::bubble::ExpiryStatus;
use crate::error::EphemeralBlobstoreError;

/// Ephemeral Store.
#[derive(Derivative)]
#[derivative(Debug)]
struct RepoEphemeralStoreInner {
    /// The backing blobstore where blobs are stored, without any redaction
    /// or repo prefix wrappers.
    pub(crate) blobstore: Arc<dyn BlobstoreEnumerableWithUnlink>,

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

    /// The mode of deletion to be used when cleaning up expired bubbles.
    /// The value determines if the bubbles need to be simply marked as
    /// expired or actually deleted from the physical store.
    pub(crate) bubble_deletion_mode: BubbleDeletionMode,
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
    ) -> (Timestamp, ExpiryStatus, Option<String>) {
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

    read SelectChangesetFromBubble(
        id: BubbleId,
    ) -> (ChangesetId,) {
        "SELECT cs_id
        FROM ephemeral_bubble_changeset_mapping
        WHERE bubble_id = {id}"
    }

    read SelectBubblesWithExpiry(
        expires_at: Timestamp,
        limit: u32,
    ) -> (BubbleId,) {
        "SELECT id
        FROM ephemeral_bubbles
        WHERE expires_at < {expires_at}
        LIMIT {limit}"
    }

    read SelectBubblesWithExpiryAndStatus(
        expires_at: Timestamp,
        limit: u32,
        expiry_status: ExpiryStatus,
    ) -> (BubbleId,) {
        "SELECT id
        FROM ephemeral_bubbles
        WHERE expires_at < {expires_at} AND expired = {expiry_status}
        LIMIT {limit}"
    }

    write UpdateExpired(
        expired: ExpiryStatus,
        id: BubbleId
    ) {
        none,
        "UPDATE ephemeral_bubbles
        SET expired={expired}
        WHERE id={id}"
    }

    write DeleteBubble(
        id: BubbleId,
    ) {
        none,
        "DELETE
        FROM ephemeral_bubbles
        WHERE id={id} AND expired"
    }

    write DeleteBubbleChangesetMapping(
        id: BubbleId,
    ) {
        none,
        "DELETE
        FROM ephemeral_bubble_changeset_mapping
        WHERE bubble_id IN (SELECT id FROM ephemeral_bubbles WHERE id = {id} AND expired)"
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
                    ExpiryStatus::Active,
                ))
            }
            _ => Err(EphemeralBlobstoreError::CreateBubbleFailed.into()),
        }
    }

    /// Given a changeset ID, fetch the corresponding Bubble ID.
    async fn bubble_from_changeset(
        &self,
        repo_id: &RepositoryId,
        cs_id: &ChangesetId,
    ) -> Result<Option<BubbleId>> {
        let rows =
            SelectBubbleFromChangeset::query(&self.connections.read_connection, repo_id, cs_id)
                .await?;
        Ok(rows.into_iter().next().map(|b| b.0))
    }

    async fn changesets_from_bubble(&self, bubble_id: &BubbleId) -> Result<Vec<ChangesetId>> {
        let rows =
            SelectChangesetFromBubble::query(&self.connections.read_connection, bubble_id).await?;
        Ok(rows.into_iter().map(|b| b.0).collect::<Vec<_>>())
    }

    /// Gets the vector of bubbles that are past their expiry period
    /// by atleast a duration of expiry_offset + bubble_expiration_grace
    async fn get_expired_bubbles(
        &self,
        expiry_offset: Duration,
        max_bubbles: u32,
    ) -> Result<Vec<BubbleId>> {
        let expiry_cutoff = DateTime::now() - to_chrono(expiry_offset);
        let rows = match self.bubble_deletion_mode {
            // If deletion mode is MarkOnly, we want to fetch only those
            // bubbles that are past expiry period but NOT marked as
            // expired yet (i.e. are active)
            BubbleDeletionMode::MarkOnly => {
                SelectBubblesWithExpiryAndStatus::query(
                    &self.connections.write_connection,
                    &Timestamp::from(expiry_cutoff - self.bubble_expiration_grace),
                    &max_bubbles,
                    &ExpiryStatus::Active,
                )
                .await?
            }
            // If hard delete is required, we want to fetch bubbles regardless
            // of their expiry status as long as they are past expiry date.
            _ => {
                SelectBubblesWithExpiry::query(
                    &self.connections.write_connection,
                    &Timestamp::from(expiry_cutoff - self.bubble_expiration_grace),
                    &max_bubbles,
                )
                .await?
            }
        };
        Ok(rows.into_iter().map(|b| b.0).collect::<Vec<_>>())
    }

    async fn keys_in_bubble(
        &self,
        bubble_id: BubbleId,
        ctx: &CoreContext,
        start_from: Option<String>,
        max: u32,
    ) -> Result<Vec<String>> {
        let bubble = self.open_bubble_raw(bubble_id, false).await?;
        bubble.keys_in_bubble(ctx, start_from, max).await
    }

    /// Method responsible for deleting the bubble and all the data contained within.
    /// Returns the number of blobs deleted from the bubble.
    async fn delete_bubble(&self, bubble_id: BubbleId, ctx: &CoreContext) -> Result<usize> {
        // Step 0: Validate if bubble deletion is enabled.
        if let BubbleDeletionMode::Disabled = self.bubble_deletion_mode {
            return Err(EphemeralBlobstoreError::DeleteBubbleDisabled.into());
        }
        // Step 1: Mark the bubble as expired in the backing SQL Store.
        let res = UpdateExpired::query(
            &self.connections.write_connection,
            &ExpiryStatus::Expired,
            &bubble_id,
        )
        .await?;
        if res.affected_rows() != 1 {
            return Err(EphemeralBlobstoreError::DeleteBubbleFailed(bubble_id).into());
        }
        // If only marking is required, exit now.
        if let BubbleDeletionMode::MarkOnly = self.bubble_deletion_mode {
            return Ok(0); // Since 0 blob items were unlinked/removed.
        }
        // Step 2: Delete the blob content within the expired bubble.
        let bubble = self.open_bubble_raw(bubble_id, false).await?;
        let count = bubble.delete_blobs_in_bubble(ctx).await?;

        // Step 3: Delete the metadata associated with the bubble from
        // the backing SQL store.
        let res =
            DeleteBubbleChangesetMapping::query(&self.connections.write_connection, &bubble_id)
                .await?;
        if res.affected_rows() > 1 {
            return Err(EphemeralBlobstoreError::DeleteBubbleFailed(bubble_id).into());
        }

        // Step 4: Delete the bubble itself from the backing SQL store.
        let res = DeleteBubble::query(&self.connections.write_connection, &bubble_id).await?;
        if res.affected_rows() > 1 {
            return Err(EphemeralBlobstoreError::DeleteBubbleFailed(bubble_id).into());
        }
        Ok(count)
    }

    async fn open_bubble_raw(&self, bubble_id: BubbleId, fail_on_expired: bool) -> Result<Bubble> {
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
        let (expires_at, expiry_status, ref _owner_identity) = rows[0];
        let expires_at: DateTime = expires_at.into();
        if fail_on_expired
            && (expiry_status == ExpiryStatus::Expired || expires_at < DateTime::now())
        {
            return Err(EphemeralBlobstoreError::NoSuchBubble(bubble_id).into());
        }

        Ok(Bubble::new(
            bubble_id,
            expires_at + self.bubble_expiration_grace,
            self.blobstore.clone(),
            self.connections.clone(),
            expiry_status,
        ))
    }

    async fn open_bubble(&self, bubble_id: BubbleId) -> Result<Bubble> {
        self.open_bubble_raw(bubble_id, true).await
    }
}

impl RepoEphemeralStore {
    pub(crate) fn new(
        repo_id: RepositoryId,
        connections: SqlConnections,
        blobstore: Arc<dyn BlobstoreEnumerableWithUnlink>,
        initial_bubble_lifespan: Duration,
        bubble_expiration_grace: Duration,
        bubble_deletion_mode: BubbleDeletionMode,
    ) -> Self {
        Self {
            repo_id,
            inner: Some(Arc::new(RepoEphemeralStoreInner {
                blobstore,
                connections,
                initial_bubble_lifespan: to_chrono(initial_bubble_lifespan),
                bubble_expiration_grace: to_chrono(bubble_expiration_grace),
                bubble_deletion_mode,
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
            .ok_or(EphemeralBlobstoreError::NoEphemeralBlobstore(self.repo_id))
    }

    pub async fn create_bubble(&self, custom_duration: Option<Duration>) -> Result<Bubble> {
        self.inner()?.create_bubble(custom_duration).await
    }

    /// Method responsible for deleting the bubble and all the data contained within.
    /// Returns the number of blobs deleted from the bubble.
    /// NOTE: Deletes the bubble regardless of its expiry status. Make sure the bubble
    /// is suitable for deletion.
    pub async fn delete_bubble(&self, bubble_id: BubbleId, ctx: &CoreContext) -> Result<usize> {
        self.inner()?.delete_bubble(bubble_id, ctx).await
    }

    /// Gets the vector of bubbles that are past their expiry period
    /// by atleast a duration of expiry_offset + bubble_expiration_grace
    pub async fn get_expired_bubbles(
        &self,
        expiry_offset: Duration,
        max_bubbles: u32,
    ) -> Result<Vec<BubbleId>> {
        self.inner()?
            .get_expired_bubbles(expiry_offset, max_bubbles)
            .await
    }

    /// Gets the blob keys stored within the bubble, optionally starting
    /// from 'start_from' and upto 'max' in count.
    pub async fn keys_in_bubble(
        &self,
        bubble_id: BubbleId,
        ctx: &CoreContext,
        start_from: Option<String>,
        max: u32,
    ) -> Result<Vec<String>> {
        self.inner()?
            .keys_in_bubble(bubble_id, ctx, start_from, max)
            .await
    }

    /// Open the bubble corresponding to the given bubble ID if the bubble
    /// exists and has not yet expired.
    pub async fn open_bubble(&self, bubble_id: BubbleId) -> Result<Bubble> {
        self.inner()?.open_bubble(bubble_id).await
    }

    /// Open the bubble corresponding to the given bubble ID regardless
    /// of the expiry status or date.
    /// NOTE: To be used only for debugging, use open_bubble for other
    /// production use cases.
    pub async fn open_bubble_raw(&self, bubble_id: BubbleId) -> Result<Bubble> {
        self.inner()?.open_bubble_raw(bubble_id, false).await
    }

    /// Given a changeset ID, fetch the corresponding bubble ID.
    pub async fn bubble_from_changeset(&self, cs_id: &ChangesetId) -> Result<Option<BubbleId>> {
        self.inner()?
            .bubble_from_changeset(&self.repo_id, cs_id)
            .await
    }

    /// Given a bubble ID, fetch the corresponding changeset ID within the
    /// repository associated with the bubble.
    pub async fn changesets_from_bubble(&self, bubble_id: &BubbleId) -> Result<Vec<ChangesetId>> {
        self.inner()?.changesets_from_bubble(bubble_id).await
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::builder::RepoEphemeralStoreBuilder;
    use anyhow::anyhow;
    use blobstore::Blobstore;
    use blobstore::BlobstoreBytes;
    use blobstore::BlobstoreEnumerableWithUnlink;
    use blobstore::BlobstoreKeyParam;
    use context::CoreContext;
    use fbinit::FacebookInit;
    use maplit::hashset;
    use memblob::Memblob;
    use metaconfig_types::BubbleDeletionMode;
    use metaconfig_types::PackFormat;
    use mononoke_types_mocks::repo::REPO_ZERO;
    use packblob::PackBlob;
    use repo_blobstore::RepoBlobstore;
    use scuba_ext::MononokeScubaSampleBuilder;
    use sql_construct::SqlConstruct;

    fn bootstrap(
        fb: FacebookInit,
        initial_lifespan: Duration,
        grace_period: Duration,
        deletion_mode: BubbleDeletionMode,
    ) -> Result<(
        CoreContext,
        Arc<dyn BlobstoreEnumerableWithUnlink>,
        RepoBlobstore,
        RepoEphemeralStore,
    )> {
        let ctx = CoreContext::test_mock(fb);
        // The ephemeral blobstore will normally be used stacked on top of
        // packblob, so use this in the test, too.
        let blobstore = Arc::new(PackBlob::new(
            Memblob::default(),
            PackFormat::ZstdIndividual(0),
        )) as Arc<dyn BlobstoreEnumerableWithUnlink>;
        let repo_blobstore = RepoBlobstore::new(
            Arc::new(Memblob::default()),
            None,
            REPO_ZERO,
            MononokeScubaSampleBuilder::with_discard(),
        );
        let eph = RepoEphemeralStoreBuilder::with_sqlite_in_memory()?.build(
            REPO_ZERO,
            blobstore.clone(),
            initial_lifespan,
            grace_period,
            deletion_mode,
        );
        Ok((ctx, blobstore, repo_blobstore, eph))
    }

    #[fbinit::test]
    async fn basic_test(fb: FacebookInit) -> Result<()> {
        let initial = Duration::from_secs(30 * 24 * 60 * 60);
        let grace = Duration::from_secs(6 * 60 * 60);
        let (ctx, blobstore, repo_blobstore, eph) =
            bootstrap(fb, initial, grace, BubbleDeletionMode::MarkAndDelete)?;
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

    #[fbinit::test]
    async fn create_and_fetch_active_test(fb: FacebookInit) -> Result<()> {
        let initial = Duration::from_secs(30 * 24 * 60 * 60);
        let grace = Duration::from_secs(6 * 60 * 60);
        let (_, _, _, eph) = bootstrap(fb, initial, grace, BubbleDeletionMode::MarkAndDelete)?;
        let bubble1 = eph.create_bubble(None).await?;
        // Ensure a newly created bubble exists in Active status
        assert_eq!(bubble1.expired(), ExpiryStatus::Active);
        // Re-opening the bubble from storage returns the same status
        let bubble1_read = eph.open_bubble(bubble1.bubble_id()).await?;
        assert_eq!(bubble1_read.expired(), bubble1.expired());
        Ok(())
    }

    #[fbinit::test]
    async fn deletion_mode_disabled_test(fb: FacebookInit) -> Result<()> {
        let initial = Duration::from_secs(30 * 24 * 60 * 60);
        let grace = Duration::from_secs(6 * 60 * 60);
        // We want an ephemeral store where deletion is disabled.
        let (ctx, _, _, eph) = bootstrap(fb, initial, grace, BubbleDeletionMode::Disabled)?;
        // Create an empty bubble.
        let bubble1 = eph.create_bubble(None).await?;
        // Attempt to delete the bubble
        let res = eph.delete_bubble(bubble1.bubble_id(), &ctx).await;
        // Since the bubble is deleted, reopening the bubble should
        // throw the "no such bubble" error
        match res {
            Err(e) => match e.downcast_ref::<EphemeralBlobstoreError>() {
                Some(EphemeralBlobstoreError::DeleteBubbleDisabled) => Ok(()),
                _ => Err(anyhow!("Invalid error during bubble deletion")),
            },
            _ => Err(anyhow!(
                "Bubble deletion should be disabled but it is enabled"
            )),
        }
    }

    #[fbinit::test]
    async fn deletion_mode_markonly_test(fb: FacebookInit) -> Result<()> {
        let initial = Duration::from_secs(30 * 24 * 60 * 60);
        let grace = Duration::from_secs(6 * 60 * 60);
        // We want an ephemeral store where deletion mode is mark only.
        let (ctx, blobstore, repo_blobstore, eph) =
            bootstrap(fb, initial, grace, BubbleDeletionMode::MarkOnly)?;
        // Create a bubble and add data to it.
        let bubble1 = eph.create_bubble(None).await?;
        let bubble1_repo = bubble1.wrap_repo_blobstore(repo_blobstore.clone());
        bubble1_repo
            .put(
                &ctx,
                String::from("test_key_1"),
                BlobstoreBytes::from_bytes("test data 1"),
            )
            .await?;
        // Add more data to it
        bubble1_repo
            .put(
                &ctx,
                String::from("test_key_2"),
                BlobstoreBytes::from_bytes("test data 2"),
            )
            .await?;
        // Enumerate the blobstore and check the required data is present.
        let enumerated = blobstore
            .enumerate(&ctx, &BlobstoreKeyParam::from(..))
            .await?;
        // Should contain two keys for now
        assert_eq!(enumerated.keys.len(), 2);
        // Delete the bubble
        let deleted = eph.delete_bubble(bubble1.bubble_id(), &ctx).await?;
        // Should be 0 since deletion mode is MarkOnly
        assert_eq!(deleted, 0);
        // Even though the bubble hasn't been deleted from the backing physical store
        // it should appear as deleted to the user and hence not be accessible.
        let res = eph.open_bubble(bubble1.bubble_id()).await;
        // Since the bubble is deleted, reopening the bubble should
        // throw the "no such bubble" error
        match res {
            Err(e) => match e.downcast_ref::<EphemeralBlobstoreError>() {
                Some(EphemeralBlobstoreError::NoSuchBubble(_)) => Ok(()),
                _ => Err(anyhow!("Invalid error post bubble deletion")),
            },
            _ => Err(anyhow!(
                "Bubble expected to be (soft) deleted but it still exists for the user"
            )),
        }
    }

    #[fbinit::test]
    async fn delete_empty_bubble_test(fb: FacebookInit) -> Result<()> {
        let initial = Duration::from_secs(30 * 24 * 60 * 60);
        let grace = Duration::from_secs(6 * 60 * 60);
        let (ctx, _, _, eph) = bootstrap(fb, initial, grace, BubbleDeletionMode::MarkAndDelete)?;
        // Create an empty bubble.
        let bubble1 = eph.create_bubble(None).await?;
        // Delete the bubble
        let deleted = eph.delete_bubble(bubble1.bubble_id(), &ctx).await?;
        // Should be 0 since the bubble was empty
        assert_eq!(deleted, 0);
        Ok(())
    }

    #[fbinit::test]
    async fn delete_nonempty_bubble_test(fb: FacebookInit) -> Result<()> {
        let initial = Duration::from_secs(30 * 24 * 60 * 60);
        let grace = Duration::from_secs(6 * 60 * 60);
        let (ctx, blobstore, repo_blobstore, eph) =
            bootstrap(fb, initial, grace, BubbleDeletionMode::MarkAndDelete)?;
        // Create a bubble and add data to it.
        let bubble1 = eph.create_bubble(None).await?;
        let bubble1_repo = bubble1.wrap_repo_blobstore(repo_blobstore.clone());
        bubble1_repo
            .put(
                &ctx,
                String::from("test_key_1"),
                BlobstoreBytes::from_bytes("test data 1"),
            )
            .await?;
        // Add more data to it
        bubble1_repo
            .put(
                &ctx,
                String::from("test_key_2"),
                BlobstoreBytes::from_bytes("test data 2"),
            )
            .await?;
        // Add some more data to it
        bubble1_repo
            .put(
                &ctx,
                String::from("test_key_3"),
                BlobstoreBytes::from_bytes("test data 3"),
            )
            .await?;
        bubble1_repo
            .put(
                &ctx,
                String::from("test_key_4"),
                BlobstoreBytes::from_bytes("test data 4"),
            )
            .await?;
        // Enumerate the blobstore and check the required data is present.
        let enumerated = blobstore
            .enumerate(&ctx, &BlobstoreKeyParam::from(..))
            .await?;
        // Should contain four keys for now
        assert_eq!(enumerated.keys.len(), 4);
        // Delete the bubble
        let deleted = eph.delete_bubble(bubble1.bubble_id(), &ctx).await?;
        // Should be 4 based on the input data added
        assert_eq!(deleted, 4);
        Ok(())
    }

    #[fbinit::test]
    async fn reopen_deleted_bubble_test(fb: FacebookInit) -> Result<()> {
        let initial = Duration::from_secs(30 * 24 * 60 * 60);
        let grace = Duration::from_secs(6 * 60 * 60);
        let (ctx, _, repo_blobstore, eph) =
            bootstrap(fb, initial, grace, BubbleDeletionMode::MarkAndDelete)?;
        // Create a bubble and add data to it.
        let bubble1 = eph.create_bubble(None).await?;
        let bubble1_repo = bubble1.wrap_repo_blobstore(repo_blobstore.clone());
        bubble1_repo
            .put(
                &ctx,
                String::from("test_key_1"),
                BlobstoreBytes::from_bytes("test data 1"),
            )
            .await?;
        // Delete the bubble
        eph.delete_bubble(bubble1.bubble_id(), &ctx).await?;
        let res = eph.open_bubble(bubble1.bubble_id()).await;
        // Since the bubble is deleted, reopening the bubble should
        // throw the "no such bubble" error
        match res {
            Err(e) => match e.downcast_ref::<EphemeralBlobstoreError>() {
                Some(EphemeralBlobstoreError::NoSuchBubble(_)) => Ok(()),
                _ => Err(anyhow!("Invalid error post bubble deletion")),
            },
            _ => Err(anyhow!("Bubble expected to be deleted but it still exists")),
        }
    }

    #[fbinit::test]
    async fn get_expired_bubbles_test(fb: FacebookInit) -> Result<()> {
        // We want immediately expiring bubbles
        let initial = Duration::from_secs(0);
        let grace = Duration::from_secs(0);
        let (_, _, _, eph) = bootstrap(fb, initial, grace, BubbleDeletionMode::MarkAndDelete)?;
        // Create an empty bubble that would expire immediately.
        let bubble1 = eph.create_bubble(None).await?;
        // Validate bubble is created in active state
        assert_eq!(bubble1.expired(), ExpiryStatus::Active);
        // Create an empty bubble that won't expire anytime soon.
        let bubble2 = eph.create_bubble(Some(Duration::from_secs(10000))).await?;
        // Validate bubble is created in active state
        assert_eq!(bubble2.expired(), ExpiryStatus::Active);
        // Get expired bubbles
        let res = eph.get_expired_bubbles(Duration::from_secs(0), 10).await?;
        // Only one bubble should be returned since only one has expired so far
        assert_eq!(res.len(), 1);
        let res_bubble_id = res
            .first()
            .expect("Invalid number of expired bubbles")
            .clone();
        // The first bubble should be the only one that's expired
        assert_eq!(res_bubble_id, bubble1.bubble_id());
        Ok(())
    }

    #[fbinit::test]
    async fn get_expired_bubbles_offset_test(fb: FacebookInit) -> Result<()> {
        // We want immediately expiring bubbles
        let initial = Duration::from_secs(0);
        let grace = Duration::from_secs(0);
        let (_, _, _, eph) = bootstrap(fb, initial, grace, BubbleDeletionMode::MarkAndDelete)?;
        // Create an empty bubble that would expire immediately.
        let bubble1 = eph.create_bubble(None).await?;
        // Validate bubble is created in active state
        assert_eq!(bubble1.expired(), ExpiryStatus::Active);
        // Create an empty bubble that won't expire anytime soon.
        let bubble2 = eph.create_bubble(Some(Duration::from_secs(10000))).await?;
        // Validate bubble is created in active state
        assert_eq!(bubble2.expired(), ExpiryStatus::Active);
        // Get expired bubbles
        let res = eph
            .get_expired_bubbles(Duration::from_secs(1000), 10)
            .await?;
        // No items should be returned since there aren't any bubbles
        // that have been expired for atleast the past 1000 seconds
        assert_eq!(res.len(), 0);
        Ok(())
    }

    #[fbinit::test]
    async fn get_expired_bubbles_markonly_test(fb: FacebookInit) -> Result<()> {
        // We want immediately expiring bubbles
        let initial = Duration::from_secs(0);
        let grace = Duration::from_secs(0);
        // We want an ephemeral store that only marks the bubbles as expired.
        let (ctx, _, _, eph) = bootstrap(fb, initial, grace, BubbleDeletionMode::MarkOnly)?;
        // Create empty bubbles that would expire immediately.
        eph.create_bubble(None).await?;
        eph.create_bubble(None).await?;
        eph.create_bubble(None).await?;
        // Get expired bubbles
        let res = eph.get_expired_bubbles(Duration::from_secs(0), 10).await?;
        // All 3 bubbles qualify for expiry and currently have their
        // status as ACTIVE, hence should be present in res
        assert_eq!(res.len(), 3);
        // Delete the bubbles in mark-only mode
        for id in res.into_iter() {
            eph.delete_bubble(id, &ctx).await?;
        }
        // The bubbles should now be soft-deleted but still be present in the
        // backing table. However, get_expired_bubbles should not return those
        // bubbles since their status will be EXPIRED.
        let res = eph.get_expired_bubbles(Duration::from_secs(0), 10).await?;
        assert_eq!(res.len(), 0);
        Ok(())
    }

    #[fbinit::test]
    async fn get_n_expired_bubbles_test(fb: FacebookInit) -> Result<()> {
        // We want immediately expiring bubbles
        let initial = Duration::from_secs(0);
        let grace = Duration::from_secs(0);
        let (_, _, _, eph) = bootstrap(fb, initial, grace, BubbleDeletionMode::MarkAndDelete)?;
        // Create empty bubbles that would expire immediately.
        eph.create_bubble(None).await?;
        eph.create_bubble(None).await?;
        eph.create_bubble(None).await?;
        eph.create_bubble(None).await?;
        eph.create_bubble(None).await?;
        // Get expired bubbles
        let res = eph.get_expired_bubbles(Duration::from_secs(0), 2).await?;
        // Only 2 bubbles should be returned given the input limit of 2
        assert_eq!(res.len(), 2);
        // Get expired bubbles
        let res = eph.get_expired_bubbles(Duration::from_secs(0), 0).await?;
        // No bubbles should be returned since limit is 0
        assert_eq!(res.len(), 0);
        Ok(())
    }

    #[fbinit::test]
    async fn reopen_expired_bubble_test(fb: FacebookInit) -> Result<()> {
        // We want immediately expiring bubbles
        let initial = Duration::from_secs(0);
        let grace = Duration::from_secs(0);
        let (_, _, _, eph) = bootstrap(fb, initial, grace, BubbleDeletionMode::MarkAndDelete)?;
        // Create an empty bubble that would expire immediately.
        let bubble1 = eph.create_bubble(None).await?;
        // Opening the expired bubble should give a
        // "No such bubble" error
        let res = eph.open_bubble(bubble1.bubble_id()).await;
        match res {
            Err(e) => match e.downcast_ref::<EphemeralBlobstoreError>() {
                Some(EphemeralBlobstoreError::NoSuchBubble(_)) => Ok(()),
                _ => Err(anyhow!("Invalid error post bubble deletion")),
            },
            _ => Err(anyhow!("Bubble expected to be deleted but it still exists")),
        }
    }
}
