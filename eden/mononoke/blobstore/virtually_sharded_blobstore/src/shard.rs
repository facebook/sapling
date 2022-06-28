/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use context::CoreContext;
use context::PerfCounterType;
use futures_stats::TimedTryFutureExt;
use std::collections::hash_map::DefaultHasher;
use std::hash::Hash;
use std::hash::Hasher;
use std::num::NonZeroUsize;
use time_ext::DurationExt;
use tokio::sync::Semaphore;
use tokio::sync::SemaphorePermit;

use crate::ratelimit::Ticket;

/// Returned by acquire. Indicates if a semaphore was acquired or not.
pub enum SemaphoreAcquisition<'a, T> {
    Acquired(SemaphorePermit<'a>),
    Cancelled(T, Ticket<'a>),
}

struct ShardHandle<'a> {
    ctx: &'a CoreContext,
    semaphore: &'a Semaphore,
    perf_counter_type: PerfCounterType,
}

impl<'a> ShardHandle<'a> {
    async fn acquire(&self) -> Result<SemaphorePermit<'a>, Error> {
        let (stats, permit) = self.semaphore.acquire().try_timed().await?;

        self.ctx.perf_counters().add_to_counter(
            self.perf_counter_type,
            stats.completion_time.as_millis_unchecked() as i64,
        );

        Ok(permit)
    }
}

pub struct Shards {
    semaphores: Vec<Semaphore>,
    perf_counter_type: PerfCounterType,
}

impl Shards {
    pub fn new(shard_count: NonZeroUsize, perf_counter_type: PerfCounterType) -> Self {
        let semaphores = (0..shard_count.get())
            .into_iter()
            .map(|_| Semaphore::new(1))
            .collect();

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
        determinator: D,
    ) -> Result<SemaphoreAcquisition<'a, T>, Error>
    where
        D: Fn() -> Result<Option<T>, Error>,
    {
        let handle = self.handle(ctx, key);

        //  Await the semaphore once, then check if our determinator says we should proceed.

        let permit = handle.acquire().await?;

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

        // Check our determinator again. This isn't stricly speaking necessary, but it makes it
        // easier to implement callsites so that they don't have to bother with where exactly we
        // chose to cancel.

        let permit = handle.acquire().await?;

        if let Some(r) = determinator()? {
            return Ok(SemaphoreAcquisition::Cancelled(r, ticket));
        }

        Ok(SemaphoreAcquisition::Acquired(permit))
    }
}
