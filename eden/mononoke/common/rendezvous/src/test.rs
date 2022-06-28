/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use fbinit::FacebookInit;
use futures::future::BoxFuture;
use futures::future::FutureExt;
use maplit::hashmap;
use maplit::hashset;
use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tokio::sync::Notify;

pub use crate::RendezVous;
pub use crate::RendezVousController;
pub use crate::RendezVousStats;

#[derive(Clone)]
struct MockController {
    arrive: Arc<Notify>,
    depart: Arc<Notify>,
    threshold: usize,
}

impl MockController {
    pub fn new(threshold: usize) -> Self {
        Self {
            arrive: Arc::new(Notify::new()),
            depart: Arc::new(Notify::new()),
            threshold,
        }
    }

    /// Blocks until wait_for_dispatch is called, then allows wait_for_dispatch to proceed.
    pub async fn release(&self) {
        self.arrive.notified().await;
        self.depart.notify_one();
    }
}

#[async_trait::async_trait]
impl RendezVousController for MockController {
    type RendezVousToken = ();

    /// Blocks until release is called.
    async fn wait_for_dispatch(&self) -> () {
        self.arrive.notify_one();
        self.depart.notified().await;
    }

    fn early_dispatch_threshold(&self) -> usize {
        self.threshold
    }
}

#[derive(Clone)]
struct MockStore {
    calls: Arc<AtomicUsize>,
}

impl MockStore {
    pub fn new() -> Self {
        Self {
            calls: Arc::new(AtomicUsize::new(0)),
        }
    }

    pub fn calls(&self) -> usize {
        self.calls.load(Ordering::Relaxed)
    }

    pub fn callback(
        &self,
    ) -> impl FnOnce(HashSet<u64>) -> BoxFuture<'static, Result<HashMap<u64, u64>, Error>> {
        let this = self.clone();
        move |keys| {
            async move {
                this.calls.fetch_add(1, Ordering::Relaxed);
                Ok(keys.into_iter().map(|k| (k, k)).collect())
            }
            .boxed()
        }
    }
}

fn stats() -> Arc<RendezVousStats> {
    Arc::new(RendezVousStats::new("test".into()))
}

#[fbinit::test]
async fn test_batch_wait(fb: FacebookInit) -> Result<(), Error> {
    let store = MockStore::new();
    let controller = MockController::new(usize::MAX);

    let rdv = RendezVous::new(controller.clone(), stats());
    let mut f1 = rdv
        .dispatch(fb, hashset! { 1 }, || store.callback())
        .boxed();
    let mut f2 = rdv
        .dispatch(fb, hashset! { 2 }, || store.callback())
        .boxed();

    // We expect both futures to be blocked since there is nothing to release them.
    assert!(futures::poll!(&mut f1).is_pending());
    assert!(futures::poll!(&mut f2).is_pending());

    // We release everything now.
    controller.release().await;

    assert_eq!(f1.await?, hashmap! { 1 => Some(1) });
    assert_eq!(f2.await?, hashmap! { 2 => Some(2) });

    assert_eq!(store.calls(), 1);

    Ok(())
}

#[fbinit::test]
async fn test_batch_limit(fb: FacebookInit) -> Result<(), Error> {
    let store = MockStore::new();
    let controller = MockController::new(2);

    let rdv = RendezVous::new(controller.clone(), stats());

    let mut f1a = rdv
        .dispatch(fb, hashset! { 1 }, || store.callback())
        .boxed();
    let mut f1b = rdv
        .dispatch(fb, hashset! { 1 }, || store.callback())
        .boxed();

    // We expect both futures to be blocked since we're still just fetching one entry here.
    assert!(futures::poll!(&mut f1a).is_pending());
    assert!(futures::poll!(&mut f1b).is_pending());

    // We release everything now.
    let f2 = rdv
        .dispatch(fb, hashset! { 2 }, || store.callback())
        .boxed();

    assert_eq!(f1a.await?, hashmap! { 1 => Some(1) });
    assert_eq!(f1b.await?, hashmap! { 1 => Some(1) });
    assert_eq!(f2.await?, hashmap! { 2 => Some(2) });

    assert_eq!(store.calls(), 1);

    Ok(())
}

#[fbinit::test]
async fn test_unbatched(fb: FacebookInit) -> Result<(), Error> {
    let store = MockStore::new();
    let controller = MockController::new(2);

    let rdv = RendezVous::new(controller.clone(), stats());

    let mut f1 = rdv
        .dispatch(fb, hashset! { 1 }, || store.callback())
        .boxed();

    // We expect f1 to be blocked because nothing released it.
    assert!(futures::poll!(&mut f1).is_pending());

    // We expect f2 to not join f1 because it exceeds the batch threshold.
    let f2 = rdv
        .dispatch(fb, hashset! { 1, 2 }, || store.callback())
        .boxed();
    assert_eq!(f2.await?, hashmap! { 1 => Some(1), 2 => Some(2) });

    // Therefore, we expect f1 to still be blocked.
    assert!(futures::poll!(&mut f1).is_pending());

    assert_eq!(store.calls(), 1);

    Ok(())
}
