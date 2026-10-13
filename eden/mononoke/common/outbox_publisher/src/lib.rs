/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Generic at-least-once publisher for a transactional SQL outbox.
//!
//! The store owns claiming and fencing. The sink owns delivery. A successful
//! delivery is acknowledged only after the sink returns success, so a crash in
//! between can redeliver the event but cannot lose it.

use anyhow::Context;
use anyhow::Result;
use anyhow::ensure;
use async_trait::async_trait;

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct OutboxEventId(pub String);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LeasedEvent {
    pub id: OutboxEventId,
    pub payload: Vec<u8>,
    pub attempt_count: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LeaseRequest {
    pub owner: String,
    pub now_ms: u64,
    pub lease_duration_ms: u64,
    pub batch_size: usize,
}

#[async_trait]
pub trait OutboxStore: Send + Sync {
    async fn lease(&self, request: &LeaseRequest) -> Result<Vec<LeasedEvent>>;

    /// Acknowledge successfully published events. The store must fence this
    /// operation on `owner` and return the number of rows it changed.
    async fn acknowledge(&self, owner: &str, ids: &[OutboxEventId]) -> Result<usize>;

    /// Release failed events for another attempt. The store must fence this
    /// operation on `owner` and return the number of rows it changed.
    async fn retry_at(&self, owner: &str, ids: &[OutboxEventId], retry_at_ms: u64)
    -> Result<usize>;
}

#[async_trait]
pub trait EventSink: Send + Sync {
    /// Publish a batch of events. Returning an error retries the whole batch.
    async fn publish_batch(&self, events: &[LeasedEvent]) -> Result<()>;
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PublishBatchOutcome {
    pub leased: usize,
    pub published: usize,
    pub failed: usize,
    /// Delivery failure that caused the batch to be released for retry.
    pub sink_error: Option<String>,
}

/// Lease and publish one batch.
///
/// The sink receives the leased events in one call. A successful batch is
/// acknowledged in full; a failed batch is retried in full. A sink may have
/// accepted some events before returning an error, so consumers must tolerate
/// duplicate notifications.
pub async fn publish_batch(
    store: &impl OutboxStore,
    sink: &impl EventSink,
    request: &LeaseRequest,
    retry_at_ms: u64,
) -> Result<PublishBatchOutcome> {
    ensure!(request.batch_size > 0, "batch size must be positive");
    ensure!(!request.owner.is_empty(), "lease owner must not be empty");

    let events = store
        .lease(request)
        .await
        .context("leasing outbox events")?;
    let leased = events.len();
    if events.is_empty() {
        return Ok(PublishBatchOutcome::default());
    }

    let ids = events
        .iter()
        .map(|event| event.id.clone())
        .collect::<Vec<_>>();
    let publish_result = sink.publish_batch(&events).await;
    let (published, failed, sink_error) = match publish_result {
        Ok(()) => {
            let acknowledged = store
                .acknowledge(&request.owner, &ids)
                .await
                .context("acknowledging published outbox events")?;
            ensure!(
                acknowledged == ids.len(),
                "outbox acknowledge was fenced: expected {} rows, changed {acknowledged}",
                ids.len()
            );
            (leased, 0, None)
        }
        Err(error) => {
            let sink_error = format!("{error:#}");
            let released = store
                .retry_at(&request.owner, &ids, retry_at_ms)
                .await
                .with_context(|| {
                    format!("releasing failed outbox events after sink failure: {sink_error}")
                })?;
            ensure!(
                released == ids.len(),
                "outbox retry was fenced after sink failure ({sink_error}): expected {} rows, \
                 changed {released}",
                ids.len()
            );
            (0, leased, Some(sink_error))
        }
    };

    Ok(PublishBatchOutcome {
        leased,
        published,
        failed,
        sink_error,
    })
}
