/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Mutex;

use anyhow::Result;
use anyhow::bail;
use async_trait::async_trait;
use outbox_publisher::EventSink;
use outbox_publisher::LeaseRequest;
use outbox_publisher::LeasedEvent;
use outbox_publisher::OutboxEventId;
use outbox_publisher::OutboxStore;
use outbox_publisher::publish_batch;

struct TestStore {
    events: Mutex<Option<Vec<LeasedEvent>>>,
    acknowledged: Mutex<Vec<OutboxEventId>>,
    retried: Mutex<Vec<OutboxEventId>>,
}

#[async_trait]
impl OutboxStore for TestStore {
    async fn lease(&self, _request: &LeaseRequest) -> Result<Vec<LeasedEvent>> {
        Ok(self.events.lock().unwrap().take().unwrap_or_default())
    }

    async fn acknowledge(&self, _owner: &str, ids: &[OutboxEventId]) -> Result<usize> {
        self.acknowledged.lock().unwrap().extend_from_slice(ids);
        Ok(ids.len())
    }

    async fn retry_at(
        &self,
        _owner: &str,
        ids: &[OutboxEventId],
        _retry_at_ms: u64,
    ) -> Result<usize> {
        self.retried.lock().unwrap().extend_from_slice(ids);
        Ok(ids.len())
    }
}

struct TestSink {
    fail: bool,
    published: Mutex<Vec<Vec<OutboxEventId>>>,
}

#[async_trait]
impl EventSink for TestSink {
    async fn publish_batch(&self, events: &[LeasedEvent]) -> Result<()> {
        self.published
            .lock()
            .unwrap()
            .push(events.iter().map(|event| event.id.clone()).collect());
        if self.fail {
            bail!("injected failure")
        }
        Ok(())
    }
}

fn event(id: &str) -> LeasedEvent {
    LeasedEvent {
        id: OutboxEventId(id.to_owned()),
        payload: format!("payload-{id}").into_bytes(),
        attempt_count: 0,
    }
}

fn store(events: Vec<LeasedEvent>) -> TestStore {
    TestStore {
        events: Mutex::new(Some(events)),
        acknowledged: Mutex::new(Vec::new()),
        retried: Mutex::new(Vec::new()),
    }
}

fn sink(fail: bool) -> TestSink {
    TestSink {
        fail,
        published: Mutex::new(Vec::new()),
    }
}

fn request() -> LeaseRequest {
    LeaseRequest {
        owner: "worker".to_owned(),
        now_ms: 100,
        lease_duration_ms: 1_000,
        batch_size: 10,
    }
}

#[tokio::test]
async fn publishes_and_acknowledges_the_whole_batch() -> Result<()> {
    let store = store(vec![event("one"), event("two"), event("three")]);
    let sink = sink(false);

    let outcome = publish_batch(&store, &sink, &request(), 500).await?;

    assert_eq!(outcome.leased, 3);
    assert_eq!(outcome.published, 3);
    assert_eq!(outcome.failed, 0);
    assert_eq!(sink.published.lock().unwrap().len(), 1);
    assert_eq!(store.acknowledged.lock().unwrap().len(), 3);
    assert!(store.retried.lock().unwrap().is_empty());
    Ok(())
}

#[tokio::test]
async fn retries_the_whole_batch_when_publish_fails() -> Result<()> {
    let store = store(vec![event("one"), event("two"), event("three")]);
    let sink = sink(true);

    let outcome = publish_batch(&store, &sink, &request(), 500).await?;

    assert_eq!(outcome.leased, 3);
    assert_eq!(outcome.published, 0);
    assert_eq!(outcome.failed, 3);
    assert_eq!(sink.published.lock().unwrap().len(), 1);
    assert!(store.acknowledged.lock().unwrap().is_empty());
    assert_eq!(store.retried.lock().unwrap().len(), 3);
    Ok(())
}

#[tokio::test]
async fn does_not_publish_an_empty_batch() -> Result<()> {
    let store = store(Vec::new());
    let sink = sink(false);

    let outcome = publish_batch(&store, &sink, &request(), 500).await?;

    assert_eq!(outcome, Default::default());
    assert!(sink.published.lock().unwrap().is_empty());
    assert!(store.acknowledged.lock().unwrap().is_empty());
    assert!(store.retried.lock().unwrap().is_empty());
    Ok(())
}
