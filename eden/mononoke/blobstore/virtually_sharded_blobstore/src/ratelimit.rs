/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use async_limiter::AsyncLimiter;
use context::CoreContext;
use context::PerfCounterType;
use futures::future::BoxFuture;
use futures::future::Fuse;
use futures::future::FutureExt;
use futures_stats::TimedTryFutureExt;
use time_ext::DurationExt;

#[derive(Copy, Clone)]
pub enum AccessReason {
    Read,
    Write,
}

/// A state machine representing access to the underlying blobstore. This lets us acquire access,
/// as well as attempt to cancel requests.
pub enum Ticket<'a> {
    /// No ticket was requested or awaited.
    NoTicket,
    /// A ticket was requested, but may not have been awaited yet.
    Pending {
        inner: TicketInner<'a>,
        /// We keep track of whether this was awaited so that in tests we can warn if we're failing
        /// to await things we should be awaiting. This isn't used outside of tests (and shouldn't:
        /// if the runtime is shutting down, our futures can be dropped with those unused tickets),
        /// and is only used to assist in debugging / validation.
        awaited: bool,
    },
    /// A ticket was requested and awaited.
    Acquired(&'a AsyncLimiter),
}

impl<'a> Ticket<'a> {
    pub fn new(ctx: &'a CoreContext, reason: AccessReason) -> Self {
        let limiter = match reason {
            AccessReason::Read => ctx.session().blobstore_read_limiter(),
            AccessReason::Write => ctx.session().blobstore_write_limiter(),
        };

        let limiter = match limiter {
            Some(limiter) => limiter,
            None => {
                return Self::NoTicket;
            }
        };

        Self::Pending {
            inner: TicketInner {
                ctx,
                reason,
                access: limiter.access().boxed().fuse(),
                limiter,
            },
            awaited: false,
        }
    }

    /// Check if this ticket is ready. This might kick off a request to acquire access.
    pub async fn is_ready(&mut self) -> Result<bool, Error> {
        match self {
            Self::NoTicket => Ok(true),
            Self::Pending {
                ref mut inner,
                ref mut awaited,
            } => {
                let ready = inner.is_ready().await?;
                if ready {
                    *awaited = true;
                }
                Ok(ready)
            }
            Self::Acquired(_) => Ok(true),
        }
    }

    /// Wait for this ticket. Calling finish again on a ticket that has already finished will just
    /// return immediately.
    pub async fn finish(mut self) -> Result<Ticket<'a>, Error> {
        match self {
            Self::Pending {
                ref mut inner,
                ref mut awaited,
            } => {
                inner.wait_for_access().await?;
                *awaited = true;
                Ok(Self::Acquired(inner.limiter))
            }
            x => Ok(x),
        }
    }

    /// Attempt to relinquish this ticket, if anything had been acquired.
    pub fn cancel(mut self) {
        match self {
            Self::NoTicket => {
                // Nothing to do here: we never acquired anything.
            }
            Self::Pending {
                ref mut awaited, ..
            } => {
                // If we never polled this ticket, there is nothing to do. If we did, then we
                // cannot cancel synchronously (since we didn't wait)
                *awaited = true;
            }
            Self::Acquired(l) => {
                l.cancel();
            }
        };
    }
}

pub struct TicketInner<'a> {
    ctx: &'a CoreContext,
    reason: AccessReason,
    access: Fuse<BoxFuture<'static, Result<(), Error>>>,
    limiter: &'a AsyncLimiter,
}

impl<'a> TicketInner<'a> {
    /// Check if the ticket is ready right now. This is async but will return immediately. It will
    /// request a ticket from the underlying rate limiter.
    async fn is_ready(&mut self) -> Result<bool, Error> {
        // Race access future against one that is always ready. Since the select is biased, it will
        // favor the access future if it is indeed ready.
        futures::select_biased! {
            r = &mut self.access => r.map(|()| true),
            default => Ok(false)
        }
    }

    /// Wait for this ticket to be ready. This will block until the underlying access future
    /// indicates that it is OK to proceed.
    async fn wait_for_access(&mut self) -> Result<(), Error> {
        let (stats, ()) = (&mut self.access).try_timed().await?;

        let counter = match self.reason {
            AccessReason::Read => PerfCounterType::BlobGetsAccessWait,
            AccessReason::Write => PerfCounterType::BlobPutsAccessWait,
        };

        self.ctx
            .perf_counters()
            .add_to_counter(counter, stats.completion_time.as_millis_unchecked() as i64);

        Ok(())
    }
}

/// This Drop impementation is here to assist in validation: if a ticket is dropped but was pending
/// and not awaited, we fail the test.
#[cfg(test)]
impl<'a> Drop for Ticket<'a> {
    fn drop(&mut self) {
        if let Self::Pending { awaited: false, .. } = self {
            panic!("Dropped a Pending ticket. This should normally not happen");
        }
    }
}
