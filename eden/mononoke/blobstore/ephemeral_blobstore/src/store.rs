/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Ephemeral Store

use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use blobstore::BlobstoreEnumerableWithUnlink;
use chrono::Duration as ChronoDuration;
use context::CoreContext;
use context::PerfCounterType;
use derivative::Derivative;
use futures_lazy_shared::LazyShared;
use metaconfig_types::BubbleDeletionMode;
use mononoke_types::ChangesetId;
use mononoke_types::DateTime;
use mononoke_types::RepositoryId;
use mononoke_types::Timestamp;
use sql_ext::SqlConnections;
use sql_ext::mononoke_queries;
use sql_query_config::SqlQueryConfig;

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
    /// Config used to do SQL queries to underlying DB
    pub(crate) sql_config: Arc<SqlQueryConfig>,

    #[derivative(Debug = "ignore")]
    /// Database used to manage the ephemeral store.
    pub(crate) connections: SqlConnections,

    /// Initial value of the lifespan for bubbles in this store, i.e. the
    /// amount of time they last from the call to create.
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

mononoke_queries! {
    write CreateBubble(
        created_at: Timestamp,
        expires_at: Timestamp,
        owner_identity: Option<&str>,
    ) {
        none,
        "INSERT INTO ephemeral_bubbles (created_at, expires_at, owner_identity)
         VALUES ({created_at}, {expires_at}, {owner_identity})"
    }

    write AddBubbleLabels(
        values: (
            bubble_id: BubbleId,
            label: str,
    )) {
        insert_or_ignore,
        "{insert_or_ignore} INTO ephemeral_bubble_labels (bubble_id, label)
        VALUES {values}"
    }

    write DeleteBubbleLabels(
        id: BubbleId,
        >list labels: &str
    ) {
        none,
        "DELETE FROM ephemeral_bubble_labels WHERE
        bubble_id = {id} AND label IN {labels}"
    }

    write DeleteBubbleLabelsById(
        id: BubbleId,
    ) {
        none,
        "DELETE FROM ephemeral_bubble_labels WHERE
        bubble_id = {id}"
    }

    cacheable read SelectBubbleById(
        id: BubbleId,
    ) -> (Timestamp, ExpiryStatus, Option<String>) {
        "SELECT expires_at, expired, owner_identity FROM ephemeral_bubbles
         WHERE id = {id}"
    }

    read SelectBubbleLabelsById(
        id: BubbleId,
    ) -> (String, ) {
        "SELECT label FROM ephemeral_bubble_labels
        WHERE bubble_id = {id}"
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
        "SELECT B.id
        FROM ephemeral_bubbles B
        WHERE B.expires_at < {expires_at}
        AND NOT EXISTS (SELECT id from ephemeral_bubble_labels L WHERE B.id = L.bubble_id LIMIT 1)
        LIMIT {limit}"
    }

    read SelectBubblesWithExpiryAndStatus(
        expires_at: Timestamp,
        limit: u32,
        expiry_status: ExpiryStatus,
    ) -> (BubbleId,) {
        "SELECT B.id
        FROM ephemeral_bubbles B
        WHERE B.expires_at < {expires_at} AND B.expired = {expiry_status}
        AND NOT EXISTS (SELECT id FROM ephemeral_bubble_labels L WHERE B.id = L.bubble_id LIMIT 1)
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

    write DeleteExpiredBubbleLabels(
        id: BubbleId,
    ) {
        none,
        "DELETE
        FROM ephemeral_bubble_labels
        WHERE bubble_id IN (SELECT id FROM ephemeral_bubbles WHERE id = {id} AND expired)"
    }
}

// Approximating a big duration to max chrono duration (~10^11 years) is good enough
fn to_chrono(duration: Duration) -> ChronoDuration {
    ChronoDuration::from_std(duration).unwrap_or(ChronoDuration::MAX)
}

impl RepoEphemeralStoreInner {
    async fn create_bubble(
        &self,
        ctx: &CoreContext,
        custom_duration: Option<Duration>,
        labels: Vec<String>,
    ) -> Result<Bubble> {
        let created_at = DateTime::now();
        let duration = match custom_duration {
            None => self.initial_bubble_lifespan,
            Some(duration) => to_chrono(duration),
        };
        let expires_at = created_at + duration;
        let txn = self
            .connections
            .write_connection
            .start_transaction()
            .await?;
        let (txn, res) = CreateBubble::maybe_traced_query_with_transaction(
            txn,
            ctx.client_request_info(),
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
                if !labels.is_empty() {
                    let bubble_labels = labels
                        .iter()
                        .map(|label| (&bubble_id, label as &str))
                        .collect::<Vec<_>>();
                    let (txn, _res) = AddBubbleLabels::maybe_traced_query_with_transaction(
                        txn,
                        ctx.client_request_info(),
                        bubble_labels.as_slice(),
                    )
                    .await?;
                    txn.commit().await?;
                } else {
                    txn.commit().await?;
                };
                ctx.perf_counters()
                    .increment_counter(PerfCounterType::SqlWrites);

                Ok(Bubble::new(
                    bubble_id,
                    expires_at + self.bubble_expiration_grace,
                    self.blobstore.clone(),
                    self.connections.clone(),
                    ExpiryStatus::Active,
                    LazyShared::new_ready(Ok(labels)),
                ))
            }
            _ => Err(EphemeralBlobstoreError::CreateBubbleFailed.into()),
        }
    }

    /// Add labels to an existing bubble
    async fn add_bubble_labels(
        &self,
        ctx: &CoreContext,
        bubble_id: BubbleId,
        labels: Vec<String>,
    ) -> Result<()> {
        // Open the bubble to validate if the bubble exists and has not expired.
        self.open_bubble(ctx, bubble_id).await?;
        let bubble_labels = labels
            .iter()
            .map(|label| (&bubble_id, label.as_str()))
            .collect::<Vec<_>>();
        // The bubble exists, add labels to it.
        ctx.perf_counters()
            .increment_counter(PerfCounterType::SqlWrites);
        AddBubbleLabels::maybe_traced_query(
            &self.connections.write_connection,
            ctx.client_request_info(),
            bubble_labels.as_slice(),
        )
        .await?;
        Ok(())
    }

    /// Remove labels associated with an existing bubble
    async fn remove_bubble_labels(
        &self,
        ctx: &CoreContext,
        bubble_id: BubbleId,
        labels: Vec<String>,
    ) -> Result<()> {
        // Open the bubble to validate if the bubble exists and has not expired.
        let bubble = self.open_bubble(ctx, bubble_id).await?;
        let labels = labels
            .iter()
            .map(|label| label.as_str())
            .collect::<Vec<_>>();
        // The bubble exists, remove labels from it.
        if labels.is_empty() {
            // No input labels were provided. In this case remove_labels should remove all
            // labels associated with the bubble.
            ctx.perf_counters()
                .increment_counter(PerfCounterType::SqlWrites);
            DeleteBubbleLabelsById::maybe_traced_query(
                &self.connections.write_connection,
                ctx.client_request_info(),
                &bubble.bubble_id(),
            )
            .await?;
        } else {
            // Specific labels were provided as input. Only remove those labels.
            ctx.perf_counters()
                .increment_counter(PerfCounterType::SqlWrites);
            DeleteBubbleLabels::maybe_traced_query(
                &self.connections.write_connection,
                ctx.client_request_info(),
                &bubble.bubble_id(),
                labels.as_slice(),
            )
            .await?;
        }
        Ok(())
    }

    /// Given a changeset ID, fetch the corresponding Bubble ID.
    async fn bubble_from_changeset(
        &self,
        ctx: &CoreContext,
        repo_id: &RepositoryId,
        cs_id: &ChangesetId,
    ) -> Result<Option<BubbleId>> {
        ctx.perf_counters()
            .increment_counter(PerfCounterType::SqlReadsReplica);
        let rows = SelectBubbleFromChangeset::maybe_traced_query(
            &self.connections.read_connection,
            ctx.client_request_info(),
            repo_id,
            cs_id,
        )
        .await?;
        Ok(rows.into_iter().next().map(|b| b.0))
    }

    /// Given a Bubble ID, fetch labels corresponding to that bubble.
    async fn labels_from_bubble(
        &self,
        ctx: &CoreContext,
        bubble_id: &BubbleId,
    ) -> Result<Vec<String>> {
        ctx.perf_counters()
            .increment_counter(PerfCounterType::SqlReadsReplica);
        let rows = SelectBubbleLabelsById::maybe_traced_query(
            &self.connections.read_connection,
            ctx.client_request_info(),
            bubble_id,
        )
        .await?;
        Ok(rows.into_iter().map(|l| l.0).collect::<Vec<_>>())
    }

    async fn changesets_from_bubble(
        &self,
        ctx: &CoreContext,
        bubble_id: &BubbleId,
    ) -> Result<Vec<ChangesetId>> {
        ctx.perf_counters()
            .increment_counter(PerfCounterType::SqlReadsReplica);
        let rows = SelectChangesetFromBubble::maybe_traced_query(
            &self.connections.read_connection,
            ctx.client_request_info(),
            bubble_id,
        )
        .await?;
        Ok(rows.into_iter().map(|b| b.0).collect::<Vec<_>>())
    }

    /// Gets the vector of bubbles that are past their expiry period
    /// by at least a duration of expiry_offset + bubble_expiration_grace
    async fn get_expired_bubbles(
        &self,
        ctx: &CoreContext,
        expiry_offset: Duration,
        max_bubbles: u32,
    ) -> Result<Vec<BubbleId>> {
        let expiry_cutoff = DateTime::now() - to_chrono(expiry_offset);
        let rows = match self.bubble_deletion_mode {
            // If deletion mode is MarkOnly, we want to fetch only those
            // bubbles that are past expiry period but NOT marked as
            // expired yet (i.e. are active)
            BubbleDeletionMode::MarkOnly => {
                ctx.perf_counters()
                    .increment_counter(PerfCounterType::SqlWrites);
                SelectBubblesWithExpiryAndStatus::maybe_traced_query(
                    &self.connections.write_connection,
                    ctx.client_request_info(),
                    &Timestamp::from(expiry_cutoff - self.bubble_expiration_grace),
                    &max_bubbles,
                    &ExpiryStatus::Active,
                )
                .await?
            }
            // If hard delete is required, we want to fetch bubbles regardless
            // of their expiry status as long as they are past expiry date.
            _ => {
                ctx.perf_counters()
                    .increment_counter(PerfCounterType::SqlWrites);
                SelectBubblesWithExpiry::maybe_traced_query(
                    &self.connections.write_connection,
                    ctx.client_request_info(),
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
        ctx: &CoreContext,
        bubble_id: BubbleId,
        start_from: Option<String>,
        max: u32,
    ) -> Result<Vec<String>> {
        let bubble = self.open_bubble_raw(ctx, bubble_id, false).await?;
        bubble.keys_in_bubble(ctx, start_from, max).await
    }

    /// Method responsible for deleting the bubble and all the data contained within.
    /// Returns the number of blobs deleted from the bubble.
    async fn delete_bubble(&self, ctx: &CoreContext, bubble_id: BubbleId) -> Result<usize> {
        // Step 0: Validate if bubble deletion is enabled.
        if let BubbleDeletionMode::Disabled = self.bubble_deletion_mode {
            return Err(EphemeralBlobstoreError::DeleteBubbleDisabled.into());
        }
        // Step 1: Mark the bubble as expired in the backing SQL Store.
        ctx.perf_counters()
            .increment_counter(PerfCounterType::SqlWrites);
        let res = UpdateExpired::maybe_traced_query(
            &self.connections.write_connection,
            ctx.client_request_info(),
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
        let bubble = self.open_bubble_raw(ctx, bubble_id, false).await?;
        let count = bubble.delete_blobs_in_bubble(ctx).await?;

        // Step 3: Delete the metadata associated with the bubble from
        // the backing SQL store.
        ctx.perf_counters()
            .increment_counter(PerfCounterType::SqlWrites);
        let res = DeleteBubbleChangesetMapping::maybe_traced_query(
            &self.connections.write_connection,
            ctx.client_request_info(),
            &bubble_id,
        )
        .await?;
        if res.affected_rows() > 1 {
            return Err(EphemeralBlobstoreError::DeleteBubbleFailed(bubble_id).into());
        }
        // When invoked through processes, this will always be a no-op. This is useful for
        // manually deleting a bubble regardless of its expiry status.
        ctx.perf_counters()
            .increment_counter(PerfCounterType::SqlWrites);
        DeleteExpiredBubbleLabels::maybe_traced_query(
            &self.connections.write_connection,
            ctx.client_request_info(),
            &bubble_id,
        )
        .await?;

        // Step 4: Delete the bubble itself from the backing SQL store.
        ctx.perf_counters()
            .increment_counter(PerfCounterType::SqlWrites);
        let res = DeleteBubble::maybe_traced_query(
            &self.connections.write_connection,
            ctx.client_request_info(),
            &bubble_id,
        )
        .await?;
        if res.affected_rows() > 1 {
            return Err(EphemeralBlobstoreError::DeleteBubbleFailed(bubble_id).into());
        }
        Ok(count)
    }

    async fn open_bubble_raw(
        &self,
        ctx: &CoreContext,
        bubble_id: BubbleId,
        fail_on_expired: bool,
    ) -> Result<Bubble> {
        ctx.perf_counters()
            .increment_counter(PerfCounterType::SqlReadsReplica);
        let mut bubble_rows = SelectBubbleById::maybe_traced_query(
            self.sql_config.as_ref(),
            None,
            &self.connections.read_connection,
            ctx.client_request_info(),
            &bubble_id,
        )
        .await?;
        let labels_future = {
            let ctx = ctx.clone();
            let connection = self.connections.read_connection.clone();
            let bubble_id = bubble_id.clone();
            async move {
                ctx.perf_counters()
                    .increment_counter(PerfCounterType::SqlReadsReplica);
                let label_rows = SelectBubbleLabelsById::maybe_traced_query(
                    &connection,
                    ctx.client_request_info(),
                    &bubble_id,
                )
                .await
                .map_err(|_| EphemeralBlobstoreError::FetchBubbleLabelsFailed(bubble_id))?;
                let labels = label_rows.into_iter().map(|l| l.0).collect::<Vec<_>>();
                Ok(labels)
            }
        };
        let labels_lazy = LazyShared::new_future(labels_future);

        if bubble_rows.is_empty() {
            // Perhaps the bubble hasn't showed up yet due to replication lag.
            // Let's retry on master just in case.
            ctx.perf_counters()
                .increment_counter(PerfCounterType::SqlReadsMaster);
            bubble_rows = SelectBubbleById::maybe_traced_query(
                self.sql_config.as_ref(),
                None,
                &self.connections.read_master_connection,
                ctx.client_request_info(),
                &bubble_id,
            )
            .await?;
            if bubble_rows.is_empty() {
                // S391328: Still no bubble.  Let's wait a short while and try
                // again.
                tokio::time::sleep(Duration::from_millis(500)).await;
                ctx.perf_counters()
                    .increment_counter(PerfCounterType::SqlReadsMaster);
                bubble_rows = SelectBubbleById::maybe_traced_query(
                    self.sql_config.as_ref(),
                    None,
                    &self.connections.read_master_connection,
                    ctx.client_request_info(),
                    &bubble_id,
                )
                .await?;
                if bubble_rows.is_empty() {
                    return Err(EphemeralBlobstoreError::NoSuchBubble(bubble_id).into());
                }
                let mut scuba = ctx.scuba().clone();
                ctx.perf_counters().insert_perf_counters(&mut scuba);
                scuba.log_with_msg("Bubble appeared after short wait", None);
            }
        }
        // TODO(mbthomas): check owner_identity
        let (expires_at, expiry_status, ref _owner_identity) = bubble_rows[0];
        let expires_at: DateTime = expires_at.into();
        // Checking only the expiry_status and not the expired_date since a bubble could
        // be active even if its past its expiry date if it has labels associated with it.
        if fail_on_expired && expiry_status == ExpiryStatus::Expired {
            return Err(EphemeralBlobstoreError::NoSuchBubble(bubble_id).into());
        }

        Ok(Bubble::new(
            bubble_id,
            expires_at + self.bubble_expiration_grace,
            self.blobstore.clone(),
            self.connections.clone(),
            expiry_status,
            labels_lazy,
        ))
    }

    async fn open_bubble(&self, ctx: &CoreContext, bubble_id: BubbleId) -> Result<Bubble> {
        self.open_bubble_raw(ctx, bubble_id, true).await
    }
}

impl RepoEphemeralStore {
    pub(crate) fn new(
        repo_id: RepositoryId,
        connections: SqlConnections,
        blobstore: Arc<dyn BlobstoreEnumerableWithUnlink>,
        sql_config: Arc<SqlQueryConfig>,
        initial_bubble_lifespan: Duration,
        bubble_expiration_grace: Duration,
        bubble_deletion_mode: BubbleDeletionMode,
    ) -> Self {
        Self {
            repo_id,
            inner: Some(Arc::new(RepoEphemeralStoreInner {
                blobstore,
                sql_config,
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

    pub async fn create_bubble(
        &self,
        ctx: &CoreContext,
        custom_duration: Option<Duration>,
        labels: Vec<String>,
    ) -> Result<Bubble> {
        self.inner()?
            .create_bubble(ctx, custom_duration, labels)
            .await
    }

    /// Method responsible for deleting the bubble and all the data contained within.
    /// Returns the number of blobs deleted from the bubble.
    /// NOTE: Deletes the bubble regardless of its expiry status. Make sure the bubble
    /// is suitable for deletion.
    pub async fn delete_bubble(&self, ctx: &CoreContext, bubble_id: BubbleId) -> Result<usize> {
        self.inner()?.delete_bubble(ctx, bubble_id).await
    }

    /// Gets the vector of bubbles that are past their expiry period
    /// by at least a duration of expiry_offset + bubble_expiration_grace
    pub async fn get_expired_bubbles(
        &self,
        ctx: &CoreContext,
        expiry_offset: Duration,
        max_bubbles: u32,
    ) -> Result<Vec<BubbleId>> {
        self.inner()?
            .get_expired_bubbles(ctx, expiry_offset, max_bubbles)
            .await
    }

    /// Gets the blob keys stored within the bubble, optionally starting
    /// from 'start_from' and upto 'max' in count.
    pub async fn keys_in_bubble(
        &self,
        ctx: &CoreContext,
        bubble_id: BubbleId,
        start_from: Option<String>,
        max: u32,
    ) -> Result<Vec<String>> {
        self.inner()?
            .keys_in_bubble(ctx, bubble_id, start_from, max)
            .await
    }

    /// Open the bubble corresponding to the given bubble ID if the bubble
    /// exists and has not yet expired.
    pub async fn open_bubble(&self, ctx: &CoreContext, bubble_id: BubbleId) -> Result<Bubble> {
        self.inner()?.open_bubble(ctx, bubble_id).await
    }

    /// Open the bubble corresponding to the given bubble ID regardless
    /// of the expiry status or date.
    /// NOTE: To be used only for debugging, use open_bubble for other
    /// production use cases.
    pub async fn open_bubble_raw(&self, ctx: &CoreContext, bubble_id: BubbleId) -> Result<Bubble> {
        self.inner()?.open_bubble_raw(ctx, bubble_id, false).await
    }

    /// Given a changeset ID, fetch the corresponding bubble ID.
    pub async fn bubble_from_changeset(
        &self,
        ctx: &CoreContext,
        cs_id: &ChangesetId,
    ) -> Result<Option<BubbleId>> {
        self.inner()?
            .bubble_from_changeset(ctx, &self.repo_id, cs_id)
            .await
    }

    /// Given a bubble ID, fetch the corresponding changeset ID within the
    /// repository associated with the bubble.
    pub async fn changesets_from_bubble(
        &self,
        ctx: &CoreContext,
        bubble_id: &BubbleId,
    ) -> Result<Vec<ChangesetId>> {
        self.inner()?.changesets_from_bubble(ctx, bubble_id).await
    }

    /// Associate the given labels with the bubble corresponding to the input
    /// bubble ID.
    pub async fn add_bubble_labels(
        &self,
        ctx: &CoreContext,
        bubble_id: BubbleId,
        labels: Vec<String>,
    ) -> Result<()> {
        self.inner()?
            .add_bubble_labels(ctx, bubble_id, labels)
            .await
    }

    /// Disassociate the given labels from the bubble corresponding to the input
    /// bubble ID.
    pub async fn remove_bubble_labels(
        &self,
        ctx: &CoreContext,
        bubble_id: BubbleId,
        labels: Vec<String>,
    ) -> Result<()> {
        self.inner()?
            .remove_bubble_labels(ctx, bubble_id, labels)
            .await
    }

    /// Given a bubble ID, fetches the labels corresponding to that bubble.
    pub async fn labels_from_bubble(
        &self,
        ctx: &CoreContext,
        bubble_id: &BubbleId,
    ) -> Result<Vec<String>> {
        self.inner()?.labels_from_bubble(ctx, bubble_id).await
    }
}

#[cfg(test)]
mod test {
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
    use mononoke_macros::mononoke;
    use mononoke_types_mocks::repo::REPO_ZERO;
    use packblob::PackBlob;
    use repo_blobstore::RepoBlobstore;
    use scuba_ext::MononokeScubaSampleBuilder;
    use sql_construct::SqlConstruct;

    use super::*;
    use crate::builder::RepoEphemeralStoreBuilder;

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
        let sql_config = Arc::new(SqlQueryConfig { caching: None });
        let eph = RepoEphemeralStoreBuilder::with_sqlite_in_memory()?.build(
            REPO_ZERO,
            blobstore.clone(),
            sql_config,
            initial_lifespan,
            grace_period,
            deletion_mode,
        );
        Ok((ctx, blobstore, repo_blobstore, eph))
    }

    #[mononoke::fbinit_test]
    async fn basic_test(fb: FacebookInit) -> Result<()> {
        let initial = Duration::from_secs(30 * 24 * 60 * 60);
        let grace = Duration::from_secs(6 * 60 * 60);
        let (ctx, blobstore, repo_blobstore, eph) =
            bootstrap(fb, initial, grace, BubbleDeletionMode::MarkAndDelete)?;
        let key = "test_key".to_string();

        // Create a bubble and put data in it.
        let bubble1 = eph.create_bubble(&ctx, None, vec![]).await?;
        let bubble1_id = bubble1.bubble_id();
        let bubble1 = bubble1.wrap_repo_blobstore(repo_blobstore.clone());
        bubble1
            .put(&ctx, key.clone(), BlobstoreBytes::from_bytes("test data"))
            .await?;
        let data = bubble1.get(&ctx, &key).await?.unwrap().into_bytes();
        assert_eq!(data.as_bytes().as_ref(), b"test data");

        // Re-open the bubble and confirm we can read the data.
        let bubble1_read = eph
            .open_bubble(&ctx, bubble1_id)
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
        let bubble2 = eph.create_bubble(&ctx, None, vec![]).await?;
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

    #[mononoke::fbinit_test]
    async fn basic_test_with_labels(fb: FacebookInit) -> Result<()> {
        let initial = Duration::from_secs(30 * 24 * 60 * 60);
        let grace = Duration::from_secs(6 * 60 * 60);
        let (ctx, _, _, eph) = bootstrap(fb, initial, grace, BubbleDeletionMode::MarkAndDelete)?;
        let labels = vec!["workspace".to_string(), "debug_version".to_string()];
        // Create a bubble with labels associated to it.
        let bubble1 = eph.create_bubble(&ctx, None, labels.clone()).await?;
        // Verify all the labels are associated with the bubble.
        assert!(
            bubble1
                .labels()
                .await?
                .iter()
                .zip(&labels)
                .filter(|&(l, r)| l == r)
                .count()
                == labels.len()
        );
        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn add_bubble_labels_test(fb: FacebookInit) -> Result<()> {
        let initial = Duration::from_secs(30 * 24 * 60 * 60);
        let grace = Duration::from_secs(6 * 60 * 60);
        let (ctx, _, _, eph) = bootstrap(fb, initial, grace, BubbleDeletionMode::MarkAndDelete)?;
        // Create a bubble with labels associated to it.
        let bubble = eph.create_bubble(&ctx, None, vec![]).await?;
        // Ensure that the bubble is created with no labels.
        assert!(bubble.labels().await?.is_empty());
        // Add labels to the newly created bubble.
        let labels = vec!["workspace".to_string(), "debug_version".to_string()];
        eph.add_bubble_labels(&ctx, bubble.bubble_id(), labels.clone())
            .await?;
        // Reopen bubble from the store and verify it has the added labels.
        let bubble_read = eph.open_bubble(&ctx, bubble.bubble_id()).await?;
        assert_eq!(bubble_read.labels().await?.len(), labels.len());
        assert!(
            bubble_read
                .labels()
                .await?
                .iter()
                .all(|label| labels.contains(label))
        );
        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn add_duplicate_bubble_labels_test(fb: FacebookInit) -> Result<()> {
        let initial = Duration::from_secs(30 * 24 * 60 * 60);
        let grace = Duration::from_secs(6 * 60 * 60);
        let (ctx, _, _, eph) = bootstrap(fb, initial, grace, BubbleDeletionMode::MarkAndDelete)?;
        // Create a bubble with labels associated to it.
        let bubble = eph.create_bubble(&ctx, None, vec![]).await?;
        // Ensure that the bubble is created with no labels.
        assert!(bubble.labels().await?.is_empty());
        // Add unique + duplicate labels to the newly created bubble.
        let labels = vec![
            "workspace".to_string(),
            "debug_version".to_string(),
            "workspace".to_string(),
            "workspace".to_string(),
        ];
        eph.add_bubble_labels(&ctx, bubble.bubble_id(), labels)
            .await?;
        // Reopen bubble from the store and verify it has the added labels
        // and the labels are not duplicated.
        let unique_labels = ["workspace".to_string(), "debug_version".to_string()];
        let bubble_read = eph.open_bubble(&ctx, bubble.bubble_id()).await?;
        assert_eq!(bubble_read.labels().await?.len(), unique_labels.len());
        assert!(
            bubble_read
                .labels()
                .await?
                .iter()
                .all(|label| unique_labels.contains(label))
        );
        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn add_empty_bubble_labels_test(fb: FacebookInit) -> Result<()> {
        let initial = Duration::from_secs(30 * 24 * 60 * 60);
        let grace = Duration::from_secs(6 * 60 * 60);
        let (ctx, _, _, eph) = bootstrap(fb, initial, grace, BubbleDeletionMode::MarkAndDelete)?;
        // Create a bubble with labels associated to it.
        let bubble = eph.create_bubble(&ctx, None, vec![]).await?;
        // Ensure that the bubble is created with no labels.
        assert!(bubble.labels().await?.is_empty());
        // Add an empty vec of labels.
        let labels = vec![];
        eph.add_bubble_labels(&ctx, bubble.bubble_id(), labels)
            .await?;
        // Reopen bubble from the store and verify it still doesn't
        // have any labels since we used an empty vec of labels as input.
        let bubble_read = eph.open_bubble(&ctx, bubble.bubble_id()).await?;
        assert!(bubble_read.labels().await?.is_empty());
        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn empty_labels_from_bubble_test(fb: FacebookInit) -> Result<()> {
        let initial = Duration::from_secs(30 * 24 * 60 * 60);
        let grace = Duration::from_secs(6 * 60 * 60);
        let (ctx, _, _, eph) = bootstrap(fb, initial, grace, BubbleDeletionMode::MarkAndDelete)?;
        // Create a bubble with no labels associated to it.
        let bubble = eph.create_bubble(&ctx, None, vec![]).await?;
        // Fetch the labels associated with the newly created bubble.
        let id = bubble.bubble_id();
        let labels = eph.labels_from_bubble(&ctx, &id).await?;
        // Validate that no labels are associated with the bubble.
        assert!(labels.is_empty());
        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn non_empty_labels_from_bubble_test(fb: FacebookInit) -> Result<()> {
        let initial = Duration::from_secs(30 * 24 * 60 * 60);
        let grace = Duration::from_secs(6 * 60 * 60);
        let (ctx, _, _, eph) = bootstrap(fb, initial, grace, BubbleDeletionMode::MarkAndDelete)?;
        let labels = vec!["workspace".to_string(), "debug".to_string()];
        // Create a bubble with some labels associated to it.
        let bubble = eph.create_bubble(&ctx, None, labels.clone()).await?;
        // Fetch the labels associated with the newly created bubble.
        let id = bubble.bubble_id();
        let returned_labels = eph.labels_from_bubble(&ctx, &id).await?;
        // Validate that the labels returned are the same as the stored labels.
        assert_eq!(returned_labels.len(), labels.len());
        assert!(returned_labels.iter().all(|label| labels.contains(label)));
        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn added_removed_labels_from_bubble_test(fb: FacebookInit) -> Result<()> {
        let initial = Duration::from_secs(30 * 24 * 60 * 60);
        let grace = Duration::from_secs(6 * 60 * 60);
        let (ctx, _, _, eph) = bootstrap(fb, initial, grace, BubbleDeletionMode::MarkAndDelete)?;
        // Create a bubble with no labels associated to it.
        let bubble = eph.create_bubble(&ctx, None, vec![]).await?;
        // Add labels to the newly created bubble.
        let labels = vec!["workspace".to_string(), "debug_version".to_string()];
        eph.add_bubble_labels(&ctx, bubble.bubble_id(), labels.clone())
            .await?;
        // Fetch the labels associated with the bubble.
        let id = bubble.bubble_id();
        let returned_labels = eph.labels_from_bubble(&ctx, &id).await?;
        // Validate that the labels returned are the same as the added labels.
        assert_eq!(returned_labels.len(), labels.len());
        assert!(returned_labels.iter().all(|label| labels.contains(label)));
        // Remove all labels associated with the bubble.
        eph.remove_bubble_labels(&ctx, bubble.bubble_id(), labels)
            .await?;
        let returned_labels = eph.labels_from_bubble(&ctx, &id).await?;
        // Validate that no labels are returned since all labels were deleted.
        assert!(returned_labels.is_empty());
        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn remove_bubble_labels_test(fb: FacebookInit) -> Result<()> {
        let initial = Duration::from_secs(30 * 24 * 60 * 60);
        let grace = Duration::from_secs(6 * 60 * 60);
        let (ctx, _, _, eph) = bootstrap(fb, initial, grace, BubbleDeletionMode::MarkAndDelete)?;
        let labels = vec!["workspace".to_string(), "debug_version".to_string()];
        // Create a bubble with labels associated to it.
        let bubble = eph.create_bubble(&ctx, None, labels.clone()).await?;
        // Reopen bubble and verify all the labels are associated with the bubble.
        let bubble_read = eph.open_bubble(&ctx, bubble.bubble_id()).await?;
        assert_eq!(bubble_read.labels().await?.len(), labels.len());
        assert!(
            bubble_read
                .labels()
                .await?
                .iter()
                .all(|label| labels.contains(label))
        );
        // Remove all labels associated with the bubble.
        eph.remove_bubble_labels(&ctx, bubble.bubble_id(), labels)
            .await?;
        // Reopen the bubble and validate that it has no labels associated with it.
        let bubble_read = eph.open_bubble(&ctx, bubble.bubble_id()).await?;
        assert!(bubble_read.labels().await?.is_empty());
        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn partial_remove_bubble_labels_test(fb: FacebookInit) -> Result<()> {
        let initial = Duration::from_secs(30 * 24 * 60 * 60);
        let grace = Duration::from_secs(6 * 60 * 60);
        let (ctx, _, _, eph) = bootstrap(fb, initial, grace, BubbleDeletionMode::MarkAndDelete)?;
        // Create a bubble with labels associated to it.
        let bubble = eph
            .create_bubble(
                &ctx,
                None,
                vec!["workspace".to_string(), "debug_version".to_string()],
            )
            .await?;
        // Remove a subset of the labels associated with the bubble.
        eph.remove_bubble_labels(&ctx, bubble.bubble_id(), vec!["workspace".to_string()])
            .await?;
        // Reopen the bubble and validate it has the remaining labels associated with it.
        let bubble_read = eph.open_bubble(&ctx, bubble.bubble_id()).await?;
        assert_eq!(
            bubble_read.labels().await?,
            vec!["debug_version".to_string()]
        );
        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn remove_absent_bubble_labels_test(fb: FacebookInit) -> Result<()> {
        let initial = Duration::from_secs(30 * 24 * 60 * 60);
        let grace = Duration::from_secs(6 * 60 * 60);
        let (ctx, _, _, eph) = bootstrap(fb, initial, grace, BubbleDeletionMode::MarkAndDelete)?;
        // Create a bubble with labels associated to it.
        let labels = vec!["workspace".to_string(), "debug_version".to_string()];
        let bubble = eph.create_bubble(&ctx, None, labels.clone()).await?;
        // Remove labels that are not part of the bubble. This should be a no-op
        // from the bubble's perspective.
        eph.remove_bubble_labels(
            &ctx,
            bubble.bubble_id(),
            vec!["some_random_label".to_string()],
        )
        .await?;
        // Reopen the bubble and validate it has the same labels as before since the
        // no existing labels were deleted.
        let bubble_read = eph.open_bubble(&ctx, bubble.bubble_id()).await?;
        assert_eq!(bubble_read.labels().await?.len(), labels.len());
        assert!(
            bubble_read
                .labels()
                .await?
                .iter()
                .all(|label| labels.contains(label))
        );
        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn remove_duplicate_bubble_labels_test(fb: FacebookInit) -> Result<()> {
        let initial = Duration::from_secs(30 * 24 * 60 * 60);
        let grace = Duration::from_secs(6 * 60 * 60);
        let (ctx, _, _, eph) = bootstrap(fb, initial, grace, BubbleDeletionMode::MarkAndDelete)?;
        // Create a bubble with labels associated to it.
        let labels = vec!["workspace".to_string(), "debug_version".to_string()];
        let bubble = eph.create_bubble(&ctx, None, labels.clone()).await?;
        // Remove labels that are not part of the bubble. This should be a no-op
        // from the bubble's perspective.
        eph.remove_bubble_labels(
            &ctx,
            bubble.bubble_id(),
            vec![
                "workspace".to_string(),
                "workspace".to_string(),
                "workspace".to_string(),
            ],
        )
        .await?;
        // Reopen the bubble and validate it has only "debug_version" label since
        // the other label was deleted. Passing input vec with duplicate labels
        // should not have any other effect.
        let bubble_read = eph.open_bubble(&ctx, bubble.bubble_id()).await?;
        assert_eq!(
            bubble_read.labels().await?,
            vec!["debug_version".to_string()]
        );
        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn remove_all_bubble_labels_without_input_test(fb: FacebookInit) -> Result<()> {
        let initial = Duration::from_secs(30 * 24 * 60 * 60);
        let grace = Duration::from_secs(6 * 60 * 60);
        let (ctx, _, _, eph) = bootstrap(fb, initial, grace, BubbleDeletionMode::MarkAndDelete)?;
        // Create a bubble with labels associated to it.
        let labels = vec!["workspace".to_string(), "debug_version".to_string()];
        let bubble = eph.create_bubble(&ctx, None, labels.clone()).await?;
        // Remove bubble labels without specifying the exact labels to remove.
        eph.remove_bubble_labels(&ctx, bubble.bubble_id(), Vec::new())
            .await?;
        // Reopen the bubble and validate that all the labels have been removed.
        // Calling remove_bubble_labels without any input labels should remove
        // all the associated labels.
        let bubble_read = eph.open_bubble(&ctx, bubble.bubble_id()).await?;
        assert_eq!(bubble_read.labels().await?, Vec::<String>::new());
        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn remove_all_bubble_labels_with_empty_bubble_test(fb: FacebookInit) -> Result<()> {
        let initial = Duration::from_secs(30 * 24 * 60 * 60);
        let grace = Duration::from_secs(6 * 60 * 60);
        let (ctx, _, _, eph) = bootstrap(fb, initial, grace, BubbleDeletionMode::MarkAndDelete)?;
        // Create a bubble with no labels associated to it.
        let bubble = eph.create_bubble(&ctx, None, Vec::new()).await?;
        // Remove bubble labels without specifying the exact labels to remove.
        // Even though no labels are associated with the bubble, operation of
        // removing all labels still succeeds as a no-op.
        eph.remove_bubble_labels(&ctx, bubble.bubble_id(), Vec::new())
            .await?;
        // Reopen the bubble and validate that the state of labels is the
        // same as before
        let bubble_read = eph.open_bubble(&ctx, bubble.bubble_id()).await?;
        assert_eq!(bubble_read.labels().await?, Vec::<String>::new());
        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn add_and_remove_bubble_labels_test(fb: FacebookInit) -> Result<()> {
        let initial = Duration::from_secs(30 * 24 * 60 * 60);
        let grace = Duration::from_secs(6 * 60 * 60);
        let (ctx, _, _, eph) = bootstrap(fb, initial, grace, BubbleDeletionMode::MarkAndDelete)?;
        // Create a bubble with labels associated to it.
        let bubble = eph.create_bubble(&ctx, None, vec![]).await?;
        // Ensure that the bubble is created with no labels.
        assert!(bubble.labels().await?.is_empty());
        // Add labels to the newly created bubble.
        let labels = vec![
            "workspace".to_string(),
            "debug_version".to_string(),
            "some_label".to_string(),
            "important_snapshot".to_string(),
        ];
        eph.add_bubble_labels(&ctx, bubble.bubble_id(), labels)
            .await?;
        // Remove a partial subset of the labels associated to the bubble.
        eph.remove_bubble_labels(
            &ctx,
            bubble.bubble_id(),
            vec!["workspace".to_string(), "debug_version".to_string()],
        )
        .await?;
        // Reopen the bubble and verify that it has the right set of labels after the
        // add and remove operation.
        let remaining_labels = ["some_label".to_string(), "important_snapshot".to_string()];
        let bubble_read = eph.open_bubble(&ctx, bubble.bubble_id()).await?;
        assert_eq!(bubble_read.labels().await?.len(), remaining_labels.len());
        assert!(
            bubble_read
                .labels()
                .await?
                .iter()
                .all(|label| remaining_labels.contains(label))
        );
        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn create_and_fetch_labels_test(fb: FacebookInit) -> Result<()> {
        let initial = Duration::from_secs(30 * 24 * 60 * 60);
        let grace = Duration::from_secs(6 * 60 * 60);
        let (ctx, _, _, eph) = bootstrap(fb, initial, grace, BubbleDeletionMode::MarkAndDelete)?;
        let labels = vec!["workspace".to_string(), "debug_version".to_string()];
        // Create a bubble with labels associated to it.
        let bubble = eph.create_bubble(&ctx, None, labels.clone()).await?;
        // Re-opening the bubble from storage.
        let bubble_read = eph.open_bubble(&ctx, bubble.bubble_id()).await?;
        // Validate that the labels in the stored bubble and the retrieved bubble are the same.
        let bubble_labels = bubble.labels().await?;
        let bubble_read_labels = bubble_read.labels().await?;
        assert_eq!(bubble_labels.len(), bubble_read_labels.len());
        assert!(
            bubble_labels
                .iter()
                .all(|label| bubble_read_labels.contains(label))
        );
        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn create_and_fetch_active_test(fb: FacebookInit) -> Result<()> {
        let initial = Duration::from_secs(30 * 24 * 60 * 60);
        let grace = Duration::from_secs(6 * 60 * 60);
        let (ctx, _, _, eph) = bootstrap(fb, initial, grace, BubbleDeletionMode::MarkAndDelete)?;
        let bubble1 = eph.create_bubble(&ctx, None, vec![]).await?;
        // Ensure a newly created bubble exists in Active status
        assert_eq!(bubble1.expired(), ExpiryStatus::Active);
        // Re-opening the bubble from storage returns the same status
        let bubble1_read = eph.open_bubble(&ctx, bubble1.bubble_id()).await?;
        assert_eq!(bubble1_read.expired(), bubble1.expired());
        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn deletion_mode_disabled_test(fb: FacebookInit) -> Result<()> {
        let initial = Duration::from_secs(30 * 24 * 60 * 60);
        let grace = Duration::from_secs(6 * 60 * 60);
        // We want an ephemeral store where deletion is disabled.
        let (ctx, _, _, eph) = bootstrap(fb, initial, grace, BubbleDeletionMode::Disabled)?;
        // Create an empty bubble.
        let bubble1 = eph.create_bubble(&ctx, None, vec![]).await?;
        // Attempt to delete the bubble
        let res = eph.delete_bubble(&ctx, bubble1.bubble_id()).await;
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

    #[mononoke::fbinit_test]
    async fn deletion_mode_markonly_test(fb: FacebookInit) -> Result<()> {
        let initial = Duration::from_secs(30 * 24 * 60 * 60);
        let grace = Duration::from_secs(6 * 60 * 60);
        // We want an ephemeral store where deletion mode is mark only.
        let (ctx, blobstore, repo_blobstore, eph) =
            bootstrap(fb, initial, grace, BubbleDeletionMode::MarkOnly)?;
        // Create a bubble and add data to it.
        let bubble1 = eph.create_bubble(&ctx, None, vec![]).await?;
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
        let deleted = eph.delete_bubble(&ctx, bubble1.bubble_id()).await?;
        // Should be 0 since deletion mode is MarkOnly
        assert_eq!(deleted, 0);
        // Even though the bubble hasn't been deleted from the backing physical store
        // it should appear as deleted to the user and hence not be accessible.
        let res = eph.open_bubble(&ctx, bubble1.bubble_id()).await;
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

    #[mononoke::fbinit_test]
    async fn delete_empty_bubble_test(fb: FacebookInit) -> Result<()> {
        let initial = Duration::from_secs(30 * 24 * 60 * 60);
        let grace = Duration::from_secs(6 * 60 * 60);
        let (ctx, _, _, eph) = bootstrap(fb, initial, grace, BubbleDeletionMode::MarkAndDelete)?;
        // Create an empty bubble.
        let bubble1 = eph.create_bubble(&ctx, None, vec![]).await?;
        // Delete the bubble
        let deleted = eph.delete_bubble(&ctx, bubble1.bubble_id()).await?;
        // Should be 0 since the bubble was empty
        assert_eq!(deleted, 0);
        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn delete_empty_bubble_with_label_test(fb: FacebookInit) -> Result<()> {
        let initial = Duration::from_secs(30 * 24 * 60 * 60);
        let grace = Duration::from_secs(6 * 60 * 60);
        let (ctx, _, _, eph) = bootstrap(fb, initial, grace, BubbleDeletionMode::MarkAndDelete)?;
        // Create an empty bubble with labels.
        let labels = vec!["workspace".to_string(), "test".to_string()];
        let bubble1 = eph.create_bubble(&ctx, None, labels).await?;
        // Delete the bubble
        let deleted = eph.delete_bubble(&ctx, bubble1.bubble_id()).await?;
        // Should be 0 since the bubble was empty
        assert_eq!(deleted, 0);
        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn delete_nonempty_bubble_test(fb: FacebookInit) -> Result<()> {
        let initial = Duration::from_secs(30 * 24 * 60 * 60);
        let grace = Duration::from_secs(6 * 60 * 60);
        let (ctx, blobstore, repo_blobstore, eph) =
            bootstrap(fb, initial, grace, BubbleDeletionMode::MarkAndDelete)?;
        // Create a bubble and add data to it.
        let bubble1 = eph.create_bubble(&ctx, None, vec![]).await?;
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
        let deleted = eph.delete_bubble(&ctx, bubble1.bubble_id()).await?;
        // Should be 4 based on the input data added
        assert_eq!(deleted, 4);
        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn reopen_deleted_bubble_test(fb: FacebookInit) -> Result<()> {
        let initial = Duration::from_secs(30 * 24 * 60 * 60);
        let grace = Duration::from_secs(6 * 60 * 60);
        let (ctx, _, repo_blobstore, eph) =
            bootstrap(fb, initial, grace, BubbleDeletionMode::MarkAndDelete)?;
        // Create a bubble and add data to it.
        let bubble1 = eph.create_bubble(&ctx, None, vec![]).await?;
        let bubble1_repo = bubble1.wrap_repo_blobstore(repo_blobstore.clone());
        bubble1_repo
            .put(
                &ctx,
                String::from("test_key_1"),
                BlobstoreBytes::from_bytes("test data 1"),
            )
            .await?;
        // Delete the bubble
        eph.delete_bubble(&ctx, bubble1.bubble_id()).await?;
        let res = eph.open_bubble(&ctx, bubble1.bubble_id()).await;
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

    #[mononoke::fbinit_test]
    async fn get_expired_bubbles_test(fb: FacebookInit) -> Result<()> {
        // We want immediately expiring bubbles
        let initial = Duration::from_secs(0);
        let grace = Duration::from_secs(0);
        let (ctx, _, _, eph) = bootstrap(fb, initial, grace, BubbleDeletionMode::MarkAndDelete)?;
        // Create an empty bubble that would expire immediately.
        let bubble1 = eph.create_bubble(&ctx, None, vec![]).await?;
        // Validate bubble is created in active state
        assert_eq!(bubble1.expired(), ExpiryStatus::Active);
        // Create an empty bubble that won't expire anytime soon.
        let bubble2 = eph
            .create_bubble(&ctx, Some(Duration::from_secs(10000)), vec![])
            .await?;
        // Validate bubble is created in active state
        assert_eq!(bubble2.expired(), ExpiryStatus::Active);
        // Get expired bubbles
        let res = eph
            .get_expired_bubbles(&ctx, Duration::from_secs(0), 10)
            .await?;
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

    #[mononoke::fbinit_test]
    async fn get_expired_bubbles_with_labels_test(fb: FacebookInit) -> Result<()> {
        // We want immediately expiring bubbles
        let initial = Duration::from_secs(0);
        let grace = Duration::from_secs(0);
        let (ctx, _, _, eph) = bootstrap(fb, initial, grace, BubbleDeletionMode::MarkAndDelete)?;
        // Create an empty bubble that would expire immediately but has labels
        // associated with it.
        let labels = vec!["workspace".to_string(), "test".to_string()];
        eph.create_bubble(&ctx, None, labels).await?;
        // Get expired bubbles
        let res = eph
            .get_expired_bubbles(&ctx, Duration::from_secs(0), 10)
            .await?;
        // Even though there exists one bubble beyond its expiry time, it should
        // not be returned since it has labels associated with it.
        assert_eq!(res.len(), 0);
        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn get_expired_bubbles_with_removed_labels_test(fb: FacebookInit) -> Result<()> {
        // We want immediately expiring bubbles
        let initial = Duration::from_secs(0);
        let grace = Duration::from_secs(0);
        let (ctx, _, _, eph) = bootstrap(fb, initial, grace, BubbleDeletionMode::MarkAndDelete)?;
        // Create an empty bubble that would expire immediately but has labels
        // associated with it.
        let labels = vec!["workspace".to_string(), "test".to_string()];
        let bubble = eph.create_bubble(&ctx, None, labels.clone()).await?;
        // Get expired bubbles
        let res = eph
            .get_expired_bubbles(&ctx, Duration::from_secs(0), 10)
            .await?;
        // Even though there exists one bubble beyond its expiry time, it should
        // not be returned since it has labels associated with it.
        assert_eq!(res.len(), 0);
        // Remove the labels associated with the bubble.
        eph.remove_bubble_labels(&ctx, bubble.bubble_id(), labels)
            .await?;
        // Get expired bubbles
        let res = eph
            .get_expired_bubbles(&ctx, Duration::from_secs(0), 10)
            .await?;
        // Now that the labels are removed, we should get the above bubble as
        // an expired bubble.
        assert_eq!(res.len(), 1);
        let res_bubble_id = res.first().expect("Invalid number of expired bubbles");
        assert_eq!(res_bubble_id, &bubble.bubble_id());
        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn add_labels_to_expired_bubble_test(fb: FacebookInit) -> Result<()> {
        // We want immediately expiring bubbles
        let initial = Duration::from_secs(0);
        let grace = Duration::from_secs(0);
        // Create in MarkOnly mode since we want to soft delete to test expiration
        let (ctx, _, _, eph) = bootstrap(fb, initial, grace, BubbleDeletionMode::MarkOnly)?;
        // Create an empty bubble that would expire immediately.
        let bubble = eph.create_bubble(&ctx, None, vec![]).await?;
        // Mark bubble as expired (i.e. soft delete)
        eph.delete_bubble(&ctx, bubble.bubble_id()).await?;
        // Add new labels to the bubble. This operation should fail since adding or
        // removing labels from an expired bubble is not permitted.
        let res = eph
            .add_bubble_labels(
                &ctx,
                bubble.bubble_id(),
                vec!["workspace".to_string(), "test".to_string()],
            )
            .await;
        assert!(res.is_err());
        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn remove_labels_from_expired_bubble_test(fb: FacebookInit) -> Result<()> {
        // We want immediately expiring bubbles
        let initial = Duration::from_secs(0);
        let grace = Duration::from_secs(0);
        // Create in MarkOnly mode since we want to soft delete to test expiration
        let (ctx, _, _, eph) = bootstrap(fb, initial, grace, BubbleDeletionMode::MarkOnly)?;
        // Create an empty bubble that would expire immediately.
        let bubble = eph.create_bubble(&ctx, None, vec![]).await?;
        // Mark bubble as expired (i.e. soft delete)
        eph.delete_bubble(&ctx, bubble.bubble_id()).await?;
        let res = eph
            .remove_bubble_labels(
                &ctx,
                bubble.bubble_id(),
                vec!["workspace".to_string(), "test".to_string()],
            )
            .await;
        assert!(res.is_err());
        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn get_expired_bubbles_offset_test(fb: FacebookInit) -> Result<()> {
        // We want immediately expiring bubbles
        let initial = Duration::from_secs(0);
        let grace = Duration::from_secs(0);
        let (ctx, _, _, eph) = bootstrap(fb, initial, grace, BubbleDeletionMode::MarkAndDelete)?;
        // Create an empty bubble that would expire immediately.
        let bubble1 = eph.create_bubble(&ctx, None, vec![]).await?;
        // Validate bubble is created in active state
        assert_eq!(bubble1.expired(), ExpiryStatus::Active);
        // Create an empty bubble that won't expire anytime soon.
        let bubble2 = eph
            .create_bubble(&ctx, Some(Duration::from_secs(10000)), vec![])
            .await?;
        // Validate bubble is created in active state
        assert_eq!(bubble2.expired(), ExpiryStatus::Active);
        // Get expired bubbles
        let res = eph
            .get_expired_bubbles(&ctx, Duration::from_secs(1000), 10)
            .await?;
        // No items should be returned since there aren't any bubbles
        // that have been expired for at least the past 1000 seconds
        assert_eq!(res.len(), 0);
        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn get_expired_bubbles_markonly_test(fb: FacebookInit) -> Result<()> {
        // We want immediately expiring bubbles
        let initial = Duration::from_secs(0);
        let grace = Duration::from_secs(0);
        // We want an ephemeral store that only marks the bubbles as expired.
        let (ctx, _, _, eph) = bootstrap(fb, initial, grace, BubbleDeletionMode::MarkOnly)?;
        // Create empty bubbles that would expire immediately.
        eph.create_bubble(&ctx, None, vec![]).await?;
        eph.create_bubble(&ctx, None, vec![]).await?;
        eph.create_bubble(&ctx, None, vec![]).await?;
        // Get expired bubbles
        let res = eph
            .get_expired_bubbles(&ctx, Duration::from_secs(0), 10)
            .await?;
        // All 3 bubbles qualify for expiry and currently have their
        // status as ACTIVE, hence should be present in res
        assert_eq!(res.len(), 3);
        // Delete the bubbles in mark-only mode
        for id in res.into_iter() {
            eph.delete_bubble(&ctx, id).await?;
        }
        // The bubbles should now be soft-deleted but still be present in the
        // backing table. However, get_expired_bubbles should not return those
        // bubbles since their status will be EXPIRED.
        let res = eph
            .get_expired_bubbles(&ctx, Duration::from_secs(0), 10)
            .await?;
        assert_eq!(res.len(), 0);
        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn get_n_expired_bubbles_test(fb: FacebookInit) -> Result<()> {
        // We want immediately expiring bubbles
        let initial = Duration::from_secs(0);
        let grace = Duration::from_secs(0);
        let (ctx, _, _, eph) = bootstrap(fb, initial, grace, BubbleDeletionMode::MarkAndDelete)?;
        // Create empty bubbles that would expire immediately.
        eph.create_bubble(&ctx, None, vec![]).await?;
        eph.create_bubble(&ctx, None, vec![]).await?;
        eph.create_bubble(&ctx, None, vec![]).await?;
        eph.create_bubble(&ctx, None, vec![]).await?;
        eph.create_bubble(&ctx, None, vec![]).await?;
        // Get expired bubbles
        let res = eph
            .get_expired_bubbles(&ctx, Duration::from_secs(0), 2)
            .await?;
        // Only 2 bubbles should be returned given the input limit of 2
        assert_eq!(res.len(), 2);
        // Get expired bubbles
        let res = eph
            .get_expired_bubbles(&ctx, Duration::from_secs(0), 0)
            .await?;
        // No bubbles should be returned since limit is 0
        assert_eq!(res.len(), 0);
        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn reopen_expired_bubble_test(fb: FacebookInit) -> Result<()> {
        // We want immediately expiring bubbles
        let initial = Duration::from_secs(0);
        let grace = Duration::from_secs(0);
        // Using MarkOnly mode since we want to mark the bubble as expired (Soft Delete)
        let (ctx, _, _, eph) = bootstrap(fb, initial, grace, BubbleDeletionMode::MarkOnly)?;
        // Create an empty bubble that would expire immediately.
        let bubble1 = eph.create_bubble(&ctx, None, vec![]).await?;
        // Mark the bubble as expired
        eph.delete_bubble(&ctx, bubble1.bubble_id()).await?;
        // Opening the expired bubble should give a
        // "No such bubble" error
        let res = eph.open_bubble(&ctx, bubble1.bubble_id()).await;
        match res {
            Err(e) => match e.downcast_ref::<EphemeralBlobstoreError>() {
                Some(EphemeralBlobstoreError::NoSuchBubble(_)) => Ok(()),
                _ => Err(anyhow!("Invalid error post bubble deletion")),
            },
            _ => Err(anyhow!("Bubble expected to be deleted but it still exists")),
        }
    }

    #[mononoke::fbinit_test]
    async fn reopen_expired_bubble_with_labels_test(fb: FacebookInit) -> Result<()> {
        // We want immediately expiring bubbles
        let initial = Duration::from_secs(0);
        let grace = Duration::from_secs(0);
        let (ctx, _, _, eph) = bootstrap(fb, initial, grace, BubbleDeletionMode::MarkAndDelete)?;
        // Create an empty bubble with labels that would expire immediately.
        let labels = vec!["workspace".to_string()];
        let bubble1 = eph.create_bubble(&ctx, None, labels).await?;
        let opened_bubble = eph.open_bubble(&ctx, bubble1.bubble_id()).await?;
        // Bubble should be reopened successfully since even though its expired by time
        // ,having labels associated with it should mark it as active.
        assert_eq!(opened_bubble.bubble_id(), bubble1.bubble_id());
        assert_eq!(opened_bubble.labels().await?, bubble1.labels().await?);
        Ok(())
    }
}
