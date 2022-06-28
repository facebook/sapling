/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use futures::future::Future;
use futures::task::Context;
use futures::task::Poll;
use maybe_owned::MaybeOwned;
use pin_project::pin_project;
use slog::Logger;
use slog::Record;
use std::pin::Pin;
use std::time::Duration;
use std::time::Instant;

#[pin_project]
pub struct WatchedFuture<R, F> {
    reporter: R,
    #[pin]
    inner: F,
}

impl<R, F> Future for WatchedFuture<R, F>
where
    R: Reporter,
    F: Future,
{
    type Output = F::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        let this = self.project();

        let now = Instant::now();
        let ret = this.inner.poll(cx);
        this.reporter.report(now.elapsed());

        ret
    }
}

pub trait Reporter {
    fn report(&self, poll: Duration);
}

pub struct SlogReporter<'a> {
    logger: MaybeOwned<'a, Logger>,
    location: slog::RecordLocation,
    max_poll: Duration,
}

impl Reporter for SlogReporter<'_> {
    fn report(&self, poll: Duration) {
        if poll <= self.max_poll {
            return;
        }

        self.logger.log(&Record::new(
            &slog::RecordStatic {
                location: &self.location,
                level: slog::Level::Warning,
                tag: "futures_watchdog",
            },
            &format_args!("Slow poll() ran for {:?}", poll),
            slog::b!(),
        ));
    }
}

pub trait WatchdogExt: Sized {
    #[track_caller]
    fn watched<'a, L>(self, logger: L) -> WatchedFuture<SlogReporter<'a>, Self>
    where
        L: Into<MaybeOwned<'a, Logger>>;
}

impl<F> WatchdogExt for F
where
    F: Future + Sized,
{
    #[track_caller]
    fn watched<'a, L>(self, logger: L) -> WatchedFuture<SlogReporter<'a>, Self>
    where
        L: Into<MaybeOwned<'a, Logger>>,
    {
        let logger = logger.into();

        let location = std::panic::Location::caller();

        let location = slog::RecordLocation {
            file: location.file(),
            line: location.line(),
            column: location.column(),
            function: "",
            module: "",
        };

        // This is a bit arbitrary but generally a very conservative default.
        let max_poll = Duration::from_millis(500);

        let reporter = SlogReporter {
            logger,
            location,
            max_poll,
        };

        WatchedFuture {
            reporter,
            inner: self,
        }
    }
}
