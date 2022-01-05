/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::cell::RefCell;
use std::fmt;
use std::iter::Sum;
use std::ops::Add;
use std::ops::AddAssign;
use std::ops::Sub;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering::AcqRel;
use std::sync::atomic::Ordering::Acquire;
use std::sync::atomic::Ordering::Relaxed;
use std::sync::atomic::Ordering::Release;
use std::sync::Arc;
use std::time::Instant;

use once_cell::sync::OnceCell;

#[derive(Default, Debug, Copy, Clone, Eq, PartialEq)]
pub struct Progress {
    pub downloaded: usize,
    pub total_downloaded: usize,
    pub uploaded: usize,
    pub total_uploaded: usize,
}

#[derive(Default, Debug)]
struct MutableProgress {
    downloaded: AtomicUsize,
    total_downloaded: AtomicUsize,
    uploaded: AtomicUsize,
    total_uploaded: AtomicUsize,
}

impl Progress {
    pub fn new(
        downloaded: usize,
        total_downloaded: usize,
        uploaded: usize,
        total_uploaded: usize,
    ) -> Self {
        Self {
            downloaded,
            total_downloaded,
            uploaded,
            total_uploaded,
        }
    }

    /// Create a `Progress` struct from progress values in the format provided
    /// by libcurl.
    ///
    /// For historical reasons, libcurl's CURLOPT_PROGRESSFUNCTION provides the
    /// callback with floating-point values. More recently, libcurl has added
    /// a new API called CURLOPT_XFERINFOFUNCTION that uses integers instead.
    /// Unfortunately, the Rust bindings to libcurl do not expose
    /// CURLOPT_XFERINFOFUNCTION, so we need to manually cast to `usize`.
    pub fn from_curl(dltotal: f64, dlnow: f64, ultotal: f64, ulnow: f64) -> Self {
        Self::new(
            dlnow as usize,
            dltotal as usize,
            ulnow as usize,
            ultotal as usize,
        )
    }

    pub fn as_tuple(&self) -> (usize, usize, usize, usize) {
        (
            self.downloaded,
            self.total_downloaded,
            self.uploaded,
            self.total_uploaded,
        )
    }
}

impl MutableProgress {
    fn to_progress(&self) -> Progress {
        let downloaded = self.downloaded.load(Relaxed);
        let uploaded = self.uploaded.load(Relaxed);
        let total_downloaded = self.total_downloaded.load(Acquire);
        let total_uploaded = self.total_uploaded.load(Acquire);
        Progress {
            downloaded,
            total_downloaded,
            uploaded,
            total_uploaded,
        }
    }

    fn set(&self, progress: Progress) {
        self.total_downloaded
            .store(progress.total_downloaded, Release);
        self.downloaded.store(progress.downloaded, Relaxed);
        self.total_uploaded.store(progress.total_uploaded, Release);
        self.uploaded.store(progress.uploaded, Relaxed);
    }

    fn add_assign(&self, progress: Progress) {
        self.total_downloaded
            .fetch_add(progress.total_downloaded, AcqRel);
        self.downloaded.fetch_add(progress.downloaded, Relaxed);
        self.total_uploaded
            .fetch_add(progress.total_uploaded, AcqRel);
        self.uploaded.fetch_add(progress.uploaded, Relaxed);
    }
}

impl fmt::Display for Progress {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "Downloaded: {}/{} bytes; Uploaded {}/{} bytes",
            self.downloaded, self.total_downloaded, self.uploaded, self.total_uploaded
        )
    }
}

impl Add for Progress {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        Self {
            downloaded: self.downloaded + other.downloaded,
            total_downloaded: self.total_downloaded + other.total_downloaded,
            uploaded: self.uploaded + other.uploaded,
            total_uploaded: self.total_uploaded + other.total_uploaded,
        }
    }
}

impl AddAssign for Progress {
    fn add_assign(&mut self, other: Progress) {
        *self = *self + other
    }
}

impl Sub for Progress {
    type Output = Self;

    fn sub(self, other: Self) -> Self {
        Self {
            downloaded: self.downloaded.saturating_sub(other.downloaded),
            total_downloaded: self.total_downloaded.saturating_sub(other.total_downloaded),
            uploaded: self.uploaded.saturating_sub(other.uploaded),
            total_uploaded: self.total_uploaded.saturating_sub(other.total_uploaded),
        }
    }
}

impl Sum for Progress {
    fn sum<I: Iterator<Item = Progress>>(iter: I) -> Progress {
        iter.fold(Default::default(), Add::add)
    }
}

/// Struct that keeps track of the collective progress of
/// a collection of active transfers. Its main purpose is
/// to report the aggregate progress of these transfers
/// as if they were a single transfer.
pub(crate) struct ProgressReporter<P> {
    inner: Arc<ProgressInner>,
    callback: RefCell<P>,
    last_progress: MutableProgress,
}

impl<P: FnMut(Progress)> ProgressReporter<P> {
    /// Create a new progress reporter that will call the provided
    /// callback whenever one of its underlying transfers reports
    /// progress.
    pub(crate) fn with_callback(callback: P) -> Self {
        Self {
            inner: Arc::new(ProgressInner::default()),
            callback: RefCell::new(callback),
            last_progress: Default::default(),
        }
    }

    /// Allocate a slot for a new transfer in the reporter,
    /// and return an updater so that the transfer handler
    /// can update the values as the transfer makes progress.
    pub(crate) fn updater(&self) -> ProgressUpdater {
        let inner = Arc::clone(&self.inner);
        ProgressUpdater {
            inner,
            last_progress: Default::default(),
        }
    }

    /// Sum all of the progress values across all slots.
    pub(crate) fn aggregate(&self) -> Progress {
        self.inner.aggregate()
    }

    /// Report the instant at which the first byte
    /// of any of the transfers was received.
    pub(crate) fn first_byte_received(&self) -> Option<Instant> {
        self.inner.first_byte_received.get().cloned()
    }

    /// Call the user-provided progress callback if any of
    /// the transfers have made progress since the last time
    /// this method was called.
    pub(crate) fn report_if_updated(&self) {
        let inner = &self.inner;
        let progress = inner.aggregate();
        if progress != self.last_progress.to_progress() {
            (&mut *self.callback.borrow_mut())(inner.aggregate());
            self.last_progress.set(progress);
        }
    }
}

/// Handle representing a slot in the progress reporter.
/// The handle may be used to update the status of an
/// individual transfer with the reporter.
pub(crate) struct ProgressUpdater {
    inner: Arc<ProgressInner>,
    last_progress: MutableProgress,
}

impl ProgressUpdater {
    pub fn update(&self, progress: Progress) {
        self.inner
            .update(self.last_progress.to_progress(), progress);
        self.last_progress.set(progress);
    }
}

/// Shared state between the updater and reporter.
/// Those structs are expected to manager interior
/// mutability, so the methods on the shared state
/// are free to take exclusive references to self.
#[derive(Default)]
struct ProgressInner {
    total_progress: MutableProgress,
    first_byte_received: OnceCell<Instant>,
}

impl ProgressInner {
    fn update(&self, last_progress: Progress, progress: Progress) {
        if progress.downloaded > 0 {
            self.first_byte_received.get_or_init(Instant::now);
        }
        self.total_progress.add_assign(progress - last_progress);
    }

    fn aggregate(&self) -> Progress {
        self.total_progress.to_progress()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(dl: usize, dlt: usize, ul: usize, ult: usize) -> Progress {
        Progress::new(dl, dlt, ul, ult)
    }

    #[test]
    fn test_progress() {
        let mut reported = Vec::new();
        let callback = |progress| {
            reported.push(progress);
        };

        let reporter = ProgressReporter::with_callback(callback);
        let updater1 = reporter.updater();
        let updater2 = reporter.updater();

        reporter.report_if_updated(); // No-op.
        assert_eq!(reporter.first_byte_received(), None);

        updater1.update(p(0, 0, 0, 0));
        assert!(reporter.first_byte_received().is_none());

        updater1.update(p(1, 2, 3, 4));
        updater2.update(p(1, 2, 3, 4));

        assert_eq!(reporter.aggregate(), p(2, 4, 6, 8));
        assert!(reporter.first_byte_received().is_some());

        reporter.report_if_updated();
        reporter.report_if_updated(); // No-op.

        updater2.update(p(4, 3, 102, 101));

        reporter.report_if_updated();

        let expected = vec![p(2, 4, 6, 8), p(5, 5, 105, 105)];
        assert_eq!(&expected, &reported);
    }
}
