/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::hash_map::DefaultHasher;
use std::hash::Hash;
use std::hash::Hasher;
use std::num::NonZeroUsize;
use std::time::Duration;

use anyhow::Error;
use context::CoreContext;
use context::PerfCounterType;
use futures_stats::TimedTryFutureExt;
use time_ext::DurationExt;
use tokio::sync::Semaphore;
use tokio::sync::SemaphorePermit;

use crate::ratelimit::Ticket;

/// Returned by acquire. Indicates if a semaphore was acquired or not.
pub enum SemaphoreAcquisition<'a, T> {
    Acquired(SemaphorePermit<'a>),
    Cancelled(T, Ticket<'a>),
    /// We gave up waiting for the shard semaphore (timed out). The caller should
    /// proceed without a dedup lease rather than block on an unrelated slow
    /// access that hashed to the same shard. The unused ticket is returned so
    /// the caller can still honour the rate limit before going to the blobstore.
    Unsharded(Ticket<'a>),
}

struct ShardHandle<'a> {
    ctx: &'a CoreContext,
    semaphore: &'a Semaphore,
    perf_counter_type: PerfCounterType,
}

impl<'a> ShardHandle<'a> {
    /// Acquire the shard semaphore. If `timeout` is `Some`, give up after that
    /// duration and return `Ok(None)`; `None` waits indefinitely (the historical
    /// behaviour).
    async fn acquire(
        &self,
        timeout: Option<Duration>,
    ) -> Result<Option<SemaphorePermit<'a>>, Error> {
        match timeout {
            None => {
                let (stats, permit) = self.semaphore.acquire().try_timed().await?;
                self.record_wait(&stats);
                Ok(Some(permit))
            }
            Some(duration) => {
                match tokio::time::timeout(duration, self.semaphore.acquire().try_timed()).await {
                    Ok(res) => {
                        let (stats, permit) = res?;
                        self.record_wait(&stats);
                        Ok(Some(permit))
                    }
                    Err(_elapsed) => Ok(None),
                }
            }
        }
    }

    fn record_wait(&self, stats: &futures_stats::FutureStats) {
        self.ctx.perf_counters().add_to_counter(
            self.perf_counter_type,
            stats.completion_time.as_millis_unchecked() as i64,
        );
    }
}

pub struct Shards {
    semaphores: Vec<Semaphore>,
    perf_counter_type: PerfCounterType,
}

impl Shards {
    pub fn new(shard_count: NonZeroUsize, perf_counter_type: PerfCounterType) -> Self {
        let semaphores = (0..shard_count.get()).map(|_| Semaphore::new(1)).collect();

        Self {
            semaphores,
            perf_counter_type,
        }
    }

    pub fn len(&self) -> usize {
        self.semaphores.len()
    }

    fn handle<'a>(&'a self, ctx: &'a CoreContext, key: &str) -> ShardHandle<'a> {
        let mut hasher = DefaultHasher::new();
        key.hash(&mut hasher);
        let semaphore = &self.semaphores[(hasher.finish() % self.semaphores.len() as u64) as usize];
        ShardHandle {
            ctx,
            semaphore,
            perf_counter_type: self.perf_counter_type,
        }
    }

    pub async fn acquire<'a, T, D>(
        &'a self,
        ctx: &'a CoreContext,
        key: &str,
        mut ticket: Ticket<'a>,
        timeout: Option<Duration>,
        determinator: D,
    ) -> Result<SemaphoreAcquisition<'a, T>, Error>
    where
        D: Fn() -> Result<Option<T>, Error>,
    {
        let handle = self.handle(ctx, key);

        //  Await the semaphore once, then check if our determinator says we should proceed.

        let permit = match handle.acquire(timeout).await? {
            Some(permit) => permit,
            None => return self.timed_out(ticket, determinator),
        };

        if let Some(r) = determinator()? {
            return Ok(SemaphoreAcquisition::Cancelled(r, ticket));
        }

        // We need to proceed. Check our rate limit.

        if ticket.is_ready().await? {
            return Ok(SemaphoreAcquisition::Acquired(permit));
        }

        // We are rate limited. Drop our permit, and wait for the rate limit.

        drop(permit);
        let ticket = ticket.finish().await?;

        // Check our determinator again. This isn't strictly speaking necessary, but it makes it
        // easier to implement callsites so that they don't have to bother with where exactly we
        // chose to cancel.

        let permit = match handle.acquire(timeout).await? {
            Some(permit) => permit,
            None => return self.timed_out(ticket, determinator),
        };

        if let Some(r) = determinator()? {
            return Ok(SemaphoreAcquisition::Cancelled(r, ticket));
        }

        Ok(SemaphoreAcquisition::Acquired(permit))
    }

    /// Handle a timed-out shard acquisition: re-check the determinator one last
    /// time (the holder may have populated the cache while we waited), otherwise
    /// tell the caller to proceed without a lease.
    fn timed_out<'a, T, D>(
        &self,
        ticket: Ticket<'a>,
        determinator: D,
    ) -> Result<SemaphoreAcquisition<'a, T>, Error>
    where
        D: Fn() -> Result<Option<T>, Error>,
    {
        if let Some(r) = determinator()? {
            return Ok(SemaphoreAcquisition::Cancelled(r, ticket));
        }
        Ok(SemaphoreAcquisition::Unsharded(ticket))
    }
}
