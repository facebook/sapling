/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use context::CoreContext;
use futures::{
    compat::Future01CompatExt,
    future::{FutureExt as NewFutureExt, TryFutureExt},
};
use futures_ext::{BoxFuture, FutureExt};
use futures_old::sync::oneshot::{channel, Receiver, Sender};
use futures_old::{future::Shared, Future, IntoFuture};
use std::collections::{hash_map::Entry, HashMap};
use std::sync::{Arc, Mutex};

use crate::LeaseOps;

/// LeaseOps that use in-memory data structures to avoid two separate tasks writing to the same key
#[derive(Clone, Debug)]
pub struct InProcessLease {
    leases: Arc<Mutex<HashMap<String, (Sender<()>, Shared<Receiver<()>>)>>>,
}

impl InProcessLease {
    pub fn new() -> Self {
        Self {
            leases: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

impl LeaseOps for InProcessLease {
    fn try_add_put_lease(&self, key: &str) -> BoxFuture<bool, ()> {
        let mut in_flight_leases = self.leases.lock().expect("lock poisoned");

        let entry = in_flight_leases.entry(key.to_string());
        if let Entry::Occupied(_) = entry {
            Ok(false).into_future().boxify()
        } else {
            let (send, recv) = channel();
            entry.or_insert((send, recv.shared()));
            Ok(true).into_future().boxify()
        }
    }

    fn renew_lease_until(&self, _ctx: CoreContext, key: &str, done: BoxFuture<(), ()>) {
        let this = self.clone();
        let key = key.to_string();
        tokio_old::spawn(
            async move {
                done.compat().await?;
                this.release_lease(&key).compat().await?;
                Ok(())
            }
            .boxed()
            .compat(),
        );
    }

    fn wait_for_other_leases(&self, key: &str) -> BoxFuture<(), ()> {
        let in_flight_leases = self.leases.lock().expect("lock poisoned");

        match in_flight_leases.get(key) {
            None => Ok(()).into_future().boxify(),
            // The map and map_err are just because FUT.shared() has different Item and Error
            // types to FUT.
            Some((_, fut)) => fut.clone().map(|_| ()).map_err(|_| ()).boxify(),
        }
    }

    fn release_lease(&self, key: &str) -> BoxFuture<(), ()> {
        let mut in_flight_leases = self.leases.lock().expect("lock poisoned");

        if let Some((sender, _)) = in_flight_leases.remove(key) {
            // Don't care if there's no-one listening - just want to wake them up if possible.
            let _ = sender.send(());
        }
        Ok(()).into_future().boxify()
    }
}
