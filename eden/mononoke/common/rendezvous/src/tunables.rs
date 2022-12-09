/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::AcquireError;
use tokio::sync::OwnedSemaphorePermit;
use tokio::sync::Semaphore;

use crate::MultiRendezVousController;
use crate::RendezVousController;
use crate::RendezVousOptions;

#[derive(Copy, Clone)]
pub struct TunablesMultiRendezVousController {
    opts: RendezVousOptions,
}

impl TunablesMultiRendezVousController {
    pub fn new(opts: RendezVousOptions) -> Self {
        Self { opts }
    }
}

impl MultiRendezVousController for TunablesMultiRendezVousController {
    type Controller = TunablesRendezVousController;

    fn new_controller(&self) -> Self::Controller {
        TunablesRendezVousController::new(self.opts)
    }
}

/// This RendezVousController is parameterized in two ways:
///
/// It allows a fixed number of "free" connections. This is how many requests we allow to exist
/// in flight at any point in time. Batching does not kick in until these are exhausted. This cannot be
/// changed after the RendezVousController is initialized, so it is not controlled by a tunable.
///
/// Provided functions control what we do once the free connections are exhausted. The functions are:
/// - max_threshold: number of keys after which we'll dispatch a full-size batch.
/// - max_delay: controls how long we wait before dispatching a small batch.
///
/// The provided functions should run very fast, please make sure they are hard-coded or cached.
///
/// Note that if a batch departs when either of those criteria are met, it will not count against
/// the count of free connections: free connections are just connections not subject to batching,
/// but once batching kicks in there is no limit to how many batches can be in flight concurrently
/// (though unless we receive infinite requests the concurrency will tend to approach the free
/// connection count).

///
pub struct ConfigurableRendezVousController {
    semaphore: Arc<Semaphore>,
    max_delay: fn() -> Duration,
    max_threshold: fn() -> usize,
}

impl ConfigurableRendezVousController {
    pub fn new(
        opts: RendezVousOptions,
        max_delay: fn() -> Duration,
        max_threshold: fn() -> usize,
    ) -> Self {
        Self {
            semaphore: Arc::new(Semaphore::new(opts.free_connections)),
            max_delay,
            max_threshold,
        }
    }
}

#[async_trait::async_trait]
impl RendezVousController for ConfigurableRendezVousController {
    // NOTE: We don't actually care about AcquireError here, since that can only happen when the
    // Semaphore is closed, but we don't close it.
    type RendezVousToken = Option<Result<OwnedSemaphorePermit, AcquireError>>;

    /// Wait for the configured dispatch delay.
    async fn wait_for_dispatch(&self) -> Self::RendezVousToken {
        tokio::time::timeout((self.max_delay)(), self.semaphore.clone().acquire_owned())
            .await
            .ok()
    }

    fn early_dispatch_threshold(&self) -> usize {
        (self.max_threshold)()
    }
}

fn max_delay() -> Duration {
    Duration::from_millis(
        ::tunables::tunables()
            .get_rendezvous_dispatch_delay_ms()
            .try_into()
            .unwrap_or(0),
    )
}

fn max_threshold() -> usize {
    ::tunables::tunables()
        .get_rendezvous_dispatch_max_threshold()
        .try_into()
        .unwrap_or(0)
}

/// This uses `ConfigurableRendezVousController`, but uses tunables to fetch the limits,
/// for ease of use and being able to quickly change them. The used tunables are:
/// - rendezvous_dispatch_max_threshold: number of keys after which we'll dispatch a full-size batch.
/// - rendezvous_dispatch_delay_ms: controls how long we wait before dispatching a small batch.
pub struct TunablesRendezVousController {
    inner: ConfigurableRendezVousController,
}

impl TunablesRendezVousController {
    pub fn new(opts: RendezVousOptions) -> Self {
        Self {
            inner: ConfigurableRendezVousController::new(opts, max_delay, max_threshold),
        }
    }
}

#[async_trait::async_trait]
impl RendezVousController for TunablesRendezVousController {
    type RendezVousToken = Option<Result<OwnedSemaphorePermit, AcquireError>>;

    /// Wait for the configured dispatch delay.
    async fn wait_for_dispatch(&self) -> Self::RendezVousToken {
        self.inner.wait_for_dispatch().await
    }

    fn early_dispatch_threshold(&self) -> usize {
        self.inner.early_dispatch_threshold()
    }
}
