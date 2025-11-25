/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::panic::Location;
use std::pin::Pin;
use std::time::Duration;
use std::time::Instant;

use futures::future::Future;
use futures::task::Context;
use futures::task::Poll;
use pin_project::pin_project;
use tracing::warn;

#[pin_project]
pub struct WatchedFuture<F> {
    #[pin]
    inner: F,
    location: &'static Location<'static>,
    max_poll: Duration,
    label: Option<String>,
    unique_id: Option<String>,
}

impl<F> WatchedFuture<F> {
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

impl<F> Future for WatchedFuture<F>
where
    F: Future,
{
    type Output = F::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        let this = self.project();

        let now = Instant::now();
        let ret = this.inner.poll(cx);

        report(
            this.location,
            this.label.as_deref(),
            this.unique_id.as_deref(),
            this.max_poll,
            now.elapsed(),
        );

        ret
    }
}

fn report(
    location: &Location,
    label: Option<&str>,
    unique_id: Option<&str>,
    max_poll: &Duration,
    poll: Duration,
) {
    if poll <= *max_poll {
        return;
    }

    let label = label.unwrap_or("");
    if let Some(unique_id) = unique_id {
        warn!(
            %unique_id,
            file = location.file(),
            line = location.line(),
            column = location.column(),
            "Slow poll({label}) ran for {poll:.3?}"
        );
    } else {
        warn!(
            file = location.file(),
            line = location.line(),
            column = location.column(),
            "Slow poll({label}) ran for {poll:.3?}"
        );
    }
}

pub trait WatchdogExt: Sized {
    #[track_caller]
    fn watched(self) -> WatchedFuture<Self>;
}

impl<F> WatchdogExt for F
where
    F: Future + Sized,
{
    #[track_caller]
    fn watched(self) -> WatchedFuture<Self> {
        let location = std::panic::Location::caller();

        // This is a bit arbitrary but generally a very conservative default.
        let max_poll = Duration::from_millis(500);

        WatchedFuture {
            inner: self,
            location,
            max_poll,
            label: None,
            unique_id: None,
        }
    }
}
