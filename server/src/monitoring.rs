// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! Scaffolding for service-level integration and monitoring.

use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
use std::thread::{self, JoinHandle};

use clap::ArgMatches;
use futures::{Async, Future, Poll};
use slog::Logger;
use tokio_core::reactor::Core;

use services::{self, Fb303Service, FbStatus};
use stats;

use errors::*;

pub(crate) fn start_stats() -> Result<JoinHandle<!>> {
    Ok(thread::Builder::new()
        .name("stats_aggregation".to_owned())
        .spawn(move || {
            let mut core = Core::new().expect("failed to create tokio core");
            let scheduler = stats::schedule_stats_aggregation(&core.handle())
                .expect("failed to create stats aggregation scheduler");
            core.run(scheduler).expect("stats scheduler failed");
            // stats scheduler shouldn't finish successfully
            unreachable!()
        })?)
}

// XXX (rain1) not sure if this should be moved to a more central location -- unclear how generally
// useful this is.

/// Represents a set of binary "ready" states that an observer is interested in.
///
/// The readiness "fails open" -- the `Drop` implementation (called even if the thread holding a
/// `ReadyHandle` or `ReadyFuture` fails) will mark its corresponding state as ready.
///
/// The typical way this is used is by calling `create_handle` to get a `ReadyHandle`, sending
/// this `ReadyHandle` to another thread if necessary, then operating on it.
#[derive(Debug)]
pub(crate) struct ReadyStateBuilder {
    markers: Vec<(String, Arc<AtomicBool>)>,
}

impl ReadyStateBuilder {
    #[inline]
    pub(crate) fn new() -> Self {
        Self {
            markers: Vec::with_capacity(4),
        }
    }

    pub(crate) fn create_handle<S: Into<String>>(&mut self, name: S) -> ReadyHandle {
        let name = name.into();
        let marker = Arc::new(AtomicBool::new(false));
        self.markers.push((name.clone(), marker.clone()));
        ReadyHandle {
            inner: Some(ReadyHandleInner { name, marker }),
        }
    }

    #[inline]
    pub(crate) fn freeze(self) -> ReadyState {
        ReadyState {
            markers: self.markers,
        }
    }
}

#[derive(Debug)]
pub(crate) struct ReadyState {
    // (possible optimization here: set a flag once all waiting is done)
    markers: Vec<(String, Arc<AtomicBool>)>,
}

impl ReadyState {
    #[inline]
    fn is_ready(&self) -> bool {
        self.markers.iter().all(|(_, b)| b.load(Ordering::Relaxed))
    }
}

// `ReadyHandle` instances shouldn't be clonable because only one caller should be able to set it.

#[derive(Debug)]
pub(crate) struct ReadyHandle {
    // The Option is so that the name and marker can be moved into a ReadyFuture without
    // conflicting with the Drop implementation.
    inner: Option<ReadyHandleInner>,
}

#[derive(Debug)]
struct ReadyHandleInner {
    name: String,
    marker: Arc<AtomicBool>,
}

impl ReadyHandle {
    pub(crate) fn wait_for<F>(mut self, fut: F) -> ReadyFuture<F>
    where
        F: Future,
    {
        let inner = self.inner
            .take()
            .expect("inner should only be None in the Drop impl");
        ReadyFuture {
            inner: fut,
            name: inner.name,
            marker: inner.marker,
        }
    }

    // XXX can implement direct setting of readiness if required
}

impl Drop for ReadyHandle {
    #[inline]
    fn drop(&mut self) {
        // Note that if the marker has been moved into a ReadyFuture it won't be set to True. This
        // is what's expected.
        if let Some(ref inner) = self.inner {
            inner.marker.store(true, Ordering::Relaxed);
        }
    }
}

// `ReadyFuture` instances shouldn't be clonable because only one caller should be able to set it.

/// Represents a future that a `ReadyState` is waiting to complete. Completion can happen either
/// when the wrapped future returns anything except `Async::NotReady` (regardless of whether it's
/// an error), or if the thread panics. (This is so that a thread panicking doesn't permanently
/// leave the ReadyState in a stuck state.)
#[derive(Debug)]
pub(crate) struct ReadyFuture<F> {
    inner: F,
    name: String,
    marker: Arc<AtomicBool>,
}

impl<F> Future for ReadyFuture<F>
where
    F: Future,
{
    type Item = F::Item;
    type Error = F::Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        match self.inner.poll() {
            Ok(Async::Ready(item)) => {
                self.marker.store(true, Ordering::Relaxed);
                Ok(Async::Ready(item))
            }
            Ok(Async::NotReady) => Ok(Async::NotReady),
            Err(err) => {
                self.marker.store(true, Ordering::Relaxed);
                Err(err)
            }
        }
    }
}

impl<F> Drop for ReadyFuture<F> {
    #[inline]
    fn drop(&mut self) {
        self.marker.store(true, Ordering::Relaxed);
    }
}

struct MononokeService {
    ready: ReadyState,
}

impl Fb303Service for MononokeService {
    fn getStatus(&self) -> FbStatus {
        // TODO: return Starting while precaching is active.
        if self.ready.is_ready() {
            FbStatus::Alive
        } else {
            FbStatus::Starting
        }
    }
}

pub(crate) fn start_thrift_service<'a>(
    logger: &Logger,
    matches: &ArgMatches<'a>,
    ready: ReadyState,
) -> Option<Result<JoinHandle<!>>> {
    matches.value_of("thrift_port").map(|port| {
        let port = port.parse().expect("Failed to parse thrift_port as number");
        info!(logger, "Initializing thrift server on port {}", port);

        thread::Builder::new()
            .name("thrift_service".to_owned())
            .spawn(move || {
                services::run_service_framework(
                    "mononoke_server",
                    port,
                    0, // Disables separate status http server
                    Box::new(MononokeService { ready }),
                ).expect("failure while running thrift service framework")
            })
            .map_err(Error::from)
    })
}

#[cfg(test)]
mod test {
    use super::*;

    use std::mem;

    use futures::future;

    #[test]
    fn ready_none() {
        let ready = ReadyStateBuilder::new();
        let ready = ready.freeze();
        assert!(ready.is_ready());
    }

    #[test]
    fn ready_handle_drop() {
        let mut ready = ReadyStateBuilder::new();
        let handle = ready.create_handle("foo");
        let ready = ready.freeze();

        assert!(!ready.is_ready());
        mem::drop(handle);
        assert!(ready.is_ready());
    }

    #[test]
    fn ready_handle_panic() {
        let mut ready = ReadyStateBuilder::new();
        let handle = ready.create_handle("foo");
        let ready = ready.freeze();

        let panic_thread = thread::spawn(move || {
            // move handle into the panicking thread
            let _handle = handle;
            panic!("thread panic should cause handle to be dropped");
        });
        let res = panic_thread.join();
        assert!(res.is_err());
        assert!(ready.is_ready());
    }

    #[test]
    fn ready_future_drop() {
        let mut ready = ReadyStateBuilder::new();
        let handle = ready.create_handle("foo");
        let ready = ready.freeze();

        let fut = handle.wait_for(future::ok::<_, !>(123));
        // Ensure that converting the handle into a future doesn't by itself cause it to be marked
        // ready.
        assert!(!ready.is_ready());
        // Drop the future without polling for it.
        mem::drop(fut);
        assert!(ready.is_ready());
    }

    #[test]
    fn ready_future_panic() {
        let mut ready = ReadyStateBuilder::new();
        let handle = ready.create_handle("foo");
        let ready = ready.freeze();

        let panic_thread = thread::spawn(move || {
            let _fut = handle.wait_for(future::ok::<_, !>(123));
            panic!("thread panic should cause future to be dropped");
        });
        let res = panic_thread.join();
        assert!(res.is_err());
        assert!(ready.is_ready());
    }

    #[test]
    fn ready_future_ok() {
        let mut ready = ReadyStateBuilder::new();
        let handle = ready.create_handle("foo");
        let ready = ready.freeze();

        let fut = handle.wait_for(future::ok::<_, !>(123));
        assert!(!ready.is_ready());

        let _ = fut.wait();
        assert!(ready.is_ready());
    }

    #[test]
    fn ready_future_err() {
        let mut ready = ReadyStateBuilder::new();
        let handle = ready.create_handle("foo");
        let ready = ready.freeze();

        let fut = handle.wait_for(future::err::<!, _>(456));
        assert!(!ready.is_ready());

        let _ = fut.wait();
        assert!(ready.is_ready());
    }

    struct LaterFuture<T> {
        value: T,
        remaining_polls: usize,
    }

    impl<T> Future for LaterFuture<T>
    where
        T: Clone,
    {
        type Item = T;
        type Error = !;

        fn poll(&mut self) -> Poll<T, !> {
            if self.remaining_polls <= 0 {
                Ok(Async::Ready(self.value.clone()))
            } else {
                self.remaining_polls -= 1;
                Ok(Async::NotReady)
            }
        }
    }

    #[test]
    fn ready_future_later() {
        let mut ready = ReadyStateBuilder::new();
        let handle = ready.create_handle("foo");
        let ready = ready.freeze();

        let mut fut = handle.wait_for(LaterFuture {
            value: 789,
            remaining_polls: 2,
        });
        assert!(!ready.is_ready());

        assert_eq!(fut.poll(), Ok(Async::NotReady));
        assert!(!ready.is_ready());
        assert_eq!(fut.poll(), Ok(Async::NotReady));
        assert!(!ready.is_ready());

        assert_eq!(fut.poll(), Ok(Async::Ready(789)));
        assert!(ready.is_ready());
    }
}
