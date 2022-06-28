/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::future::Future;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

use anyhow::Result;
use cacheblob::LeaseOps;
use context::CoreContext;
use futures::channel::oneshot;
use futures::future::FutureExt;
use slog::warn;

const LEASE_WARNING_THRESHOLD: Duration = Duration::from_secs(60);

#[derive(Clone)]
pub struct DerivedDataLease {
    lease_ops: Arc<dyn LeaseOps>,
}

impl DerivedDataLease {
    pub fn new(lease_ops: Arc<dyn LeaseOps>) -> Self {
        DerivedDataLease { lease_ops }
    }

    pub fn lease_ops(&self) -> &Arc<dyn LeaseOps> {
        &self.lease_ops
    }

    pub async fn try_acquire_in_loop<F, Fut>(
        &self,
        ctx: &CoreContext,
        key: &str,
        mut abort_fn: F,
    ) -> Result<Option<DerivedDataLeaseGuard>>
    where
        F: FnMut() -> Fut,
        Fut: Future<Output = Result<bool>>,
    {
        let mut start = Instant::now();
        let mut total_elapsed = Duration::from_secs(0);
        let mut backoff_ms = 200;
        while !self.lease_ops.try_add_put_lease(key).await? {
            if abort_fn().await? {
                return Ok(None);
            }
            let elapsed = start.elapsed();
            if elapsed > LEASE_WARNING_THRESHOLD {
                total_elapsed += elapsed;
                start = Instant::now();
                warn!(
                    ctx.logger(),
                    "Can not acquire lease {} for more than {:?}", key, total_elapsed
                );
            }
            let sleep = rand::random::<u64>() % backoff_ms;
            tokio::time::sleep(Duration::from_millis(sleep)).await;
            backoff_ms = std::cmp::min(backoff_ms * 2, 1000);
        }
        let (sender, receiver) = oneshot::channel();
        self.lease_ops
            .renew_lease_until(ctx.clone(), key, receiver.map(|_| ()).boxed());
        Ok(Some(DerivedDataLeaseGuard {
            sender: Some(sender),
        }))
    }
}

/// Guard representing an active lease.  We stop renewing the lease when
/// the guard is dropped.
pub struct DerivedDataLeaseGuard {
    sender: Option<oneshot::Sender<()>>,
}

impl Drop for DerivedDataLeaseGuard {
    fn drop(&mut self) {
        if let Some(sender) = self.sender.take() {
            let _ = sender.send(());
        }
    }
}
