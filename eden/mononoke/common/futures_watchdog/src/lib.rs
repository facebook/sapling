/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::pin::Pin;
use std::time::Duration;
use std::time::Instant;

use futures::future::Future;
use futures::task::Context;
use futures::task::Poll;
use maybe_owned::MaybeOwned;
use pin_project::pin_project;
use slog::Logger;
use slog::Record;

#[pin_project]
pub struct WatchedFuture<R, F> {
    reporter: R,
    #[pin]
    inner: F,
    max_poll: Duration,
    label: Option<String>,
    unique_id: Option<String>,
}

impl<R, F> WatchedFuture<R, F> {
    pub fn with_max_poll(mut self, max_poll: u64) -> Self {
        self.max_poll = Duration::from_millis(max_poll);
        self
    }

    pub fn with_label(mut self, label: &str) -> Self {
        self.label = Some(label.to_string());
        self
    }

    pub fn with_unique_id(mut self, unique_id: &str) -> Self {
        self.unique_id = Some(unique_id.to_string());
        self
    }
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
        this.reporter
            .report(this.label, this.unique_id, this.max_poll, now.elapsed());

        ret
    }
}

pub trait Reporter {
    fn report(
        &self,
        name: &Option<String>,
        unique_id: &Option<String>,
        max_poll: &Duration,
        poll: Duration,
    );
}

pub struct SlogReporter<'a> {
    logger: MaybeOwned<'a, Logger>,
    location: slog::RecordLocation,
}

impl Reporter for SlogReporter<'_> {
    fn report(
        &self,
        name: &Option<String>,
        unique_id: &Option<String>,
        max_poll: &Duration,
        poll: Duration,
    ) {
        if poll <= *max_poll {
            return;
        }

        let name = name.as_deref().unwrap_or("");
        let unique_id_suffix = match unique_id {
            Some(unique_id) => format!(", unique_id={}", unique_id),
            None => "".to_string(),
        };

        self.logger.log(&Record::new(
            &slog::RecordStatic {
                location: &self.location,
                level: slog::Level::Warning,
                tag: "futures_watchdog",
            },
            &format_args!("Slow poll({}) ran for {:?}{}", name, poll, unique_id_suffix),
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

        let reporter = SlogReporter { logger, location };

        WatchedFuture {
            reporter,
            inner: self,
            label: None,
            unique_id: None,
            max_poll,
        }
    }
}
