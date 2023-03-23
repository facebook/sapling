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

/// Keep track of whether the ticket has been awaited.  This is used in tests
/// to validate that we always await the ticket.
#[derive(Clone, Eq, PartialEq, Default)]
pub struct CheckAwaited {
    awaited: bool,
}

impl CheckAwaited {
    /// Mark that the ticket that contains this `CheckAwaited` has been
    /// awaited and thus is now safe to drop.
    fn mark(&mut self) {
        self.awaited = true;
    }
}

/// Implementation of drop to assist with validation during tests.  If the
/// `Ticket` that owns this `CheckAwaited` has not been awaited before being
/// dropped, we fail the test.
#[cfg(test)]
impl Drop for CheckAwaited {
    fn drop(&mut self) {
        if !self.awaited {
            panic!("Dropped a Pending ticket. This should normally not happen");
        }
    }
}

/// A state machine representing access to the underlying blobstore. This lets us acquire access,
/// as well as attempt to cancel requests.
pub enum Ticket<'a> {
    /// No ticket was requested or awaited.
    NoTicket,
    /// A ticket was requested, but may not have been awaited yet.
    Pending {
        ctx: &'a CoreContext,
        reason: AccessReason,
        access: Fuse<BoxFuture<'static, Result<(), Error>>>,
        limiter: &'a AsyncLimiter,
        /// We keep track of whether this was awaited so that in tests we can warn if we're failing
        /// to await things we should be awaiting. This isn't used outside of tests (and shouldn't:
        /// if the runtime is shutting down, our futures can be dropped with those unused tickets),
        /// and is only used to assist in debugging / validation.
        awaited: CheckAwaited,
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
            ctx,
            reason,
            access: limiter.access().boxed().fuse(),
            limiter,
            awaited: CheckAwaited::default(),
        }
    }

    /// Check if this ticket is ready. This might kick off a request to acquire access.
    pub async fn is_ready(&mut self) -> Result<bool, Error> {
        match self {
            Self::NoTicket => Ok(true),
            Self::Pending {
                ref mut access,
                ref mut awaited,
                ..
            } => {
                // Race access future against one that is always ready. Since the select is biased, it will
                // favor the access future if it is indeed ready.
                let ready = futures::select_biased! {
                    r = &mut *access => r.map(|()| true),
                    default => Ok(false)
                }?;
                if ready {
                    awaited.mark();
                }
                Ok(ready)
            }
            Self::Acquired(_) => Ok(true),
        }
    }

    /// Wait for this ticket. Calling finish again on a ticket that has already finished will just
    /// return immediately.
    pub async fn finish(self) -> Result<Ticket<'a>, Error> {
        match self {
            Self::Pending {
                ctx,
                reason,
                access,
                limiter,
                mut awaited,
            } => {
                let (stats, ()) = access.try_timed().await?;
                awaited.mark();

                let counter = match reason {
                    AccessReason::Read => PerfCounterType::BlobGetsAccessWait,
                    AccessReason::Write => PerfCounterType::BlobPutsAccessWait,
                };
                ctx.perf_counters()
                    .add_to_counter(counter, stats.completion_time.as_millis_unchecked() as i64);

                Ok(Self::Acquired(limiter))
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
                awaited.mark();
            }
            Self::Acquired(l) => {
                l.cancel();
            }
        };
    }
}
