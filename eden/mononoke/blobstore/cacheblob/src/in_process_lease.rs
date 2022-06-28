/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use async_trait::async_trait;
use context::CoreContext;
use futures::channel::oneshot::channel;
use futures::channel::oneshot::Receiver;
use futures::channel::oneshot::Sender;
use futures::future::BoxFuture;
use futures::future::FutureExt;
use futures::future::Shared;
use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::LeaseOps;

/// LeaseOps that use in-memory data structures to avoid two separate tasks writing to the same key
#[derive(Clone, Debug)]
pub struct InProcessLease {
    leases: Arc<Mutex<HashMap<String, (Sender<()>, Shared<Receiver<()>>)>>>,
}

impl std::fmt::Display for InProcessLease {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "InProcessLease")
    }
}

impl InProcessLease {
    pub fn new() -> Self {
        Self {
            leases: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

#[async_trait]
impl LeaseOps for InProcessLease {
    async fn try_add_put_lease(&self, key: &str) -> Result<bool> {
        let key = key.to_string();
        let mut in_flight_leases = self.leases.lock().await;

        let entry = in_flight_leases.entry(key);
        if let Entry::Occupied(_) = entry {
            Ok(false)
        } else {
            let (send, recv) = channel();
            entry.or_insert((send, recv.shared()));
            Ok(true)
        }
    }

    fn renew_lease_until(&self, _ctx: CoreContext, key: &str, done: BoxFuture<'static, ()>) {
        let this = self.clone();
        let key = key.to_string();
        tokio::spawn(async move {
            done.await;
            this.release_lease(&key).await;
        });
    }

    async fn wait_for_other_leases(&self, key: &str) {
        let key = key.to_string();
        let in_flight_leases = self.leases.lock().await;

        if let Some((_, fut)) = in_flight_leases.get(&key) {
            let _ = fut.clone().await;
        }
    }

    async fn release_lease(&self, key: &str) {
        let key = key.to_string();
        let mut in_flight_leases = self.leases.lock().await;

        if let Some((sender, _)) = in_flight_leases.remove(&key) {
            // Don't care if there's no-one listening - just want to wake them up if possible.
            let _ = sender.send(());
        }
    }
}
