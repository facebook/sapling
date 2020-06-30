/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![allow(dead_code)]

use std::{
    cell::RefCell,
    fmt,
    iter::Sum,
    ops::{Add, AddAssign},
    rc::Rc,
    time::Instant,
};

#[derive(Default, Debug, Copy, Clone, Eq, PartialEq)]
pub struct Progress {
    pub downloaded: usize,
    pub total_downloaded: usize,
    pub uploaded: usize,
    pub total_uploaded: usize,
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
    inner: Rc<RefCell<ProgressInner>>,
    callback: RefCell<P>,
}

impl<P: FnMut(Progress)> ProgressReporter<P> {
    /// Create a new progress reporter that will call the provided
    /// callback whenever one of its underlying transfers reports
    /// progress.
    pub(crate) fn with_callback(callback: P) -> Self {
        Self {
            inner: Rc::new(RefCell::new(ProgressInner::default())),
            callback: RefCell::new(callback),
        }
    }

    /// Allocate a slot for a new transfer in the reporter,
    /// and return an updater so that the transfer handler
    /// can update the values as the transfer makes progress.
    pub(crate) fn updater(&self) -> ProgressUpdater {
        let inner = Rc::clone(&self.inner);
        let index = inner.borrow_mut().new_slot();
        ProgressUpdater { inner, index }
    }

    /// Sum all of the progress values across all slots.
    pub(crate) fn aggregate(&self) -> Progress {
        self.inner.borrow().aggregate()
    }

    /// Report the instant at which the first byte
    /// of any of the transfers was received.
    pub(crate) fn first_byte_received(&self) -> Option<Instant> {
        self.inner.borrow().first_byte_received.clone()
    }

    /// Call the user-provided progress callback if any of
    /// the transfers have made progress since the last time
    /// this method was called.
    pub(crate) fn report_if_updated(&self) {
        let mut inner = self.inner.borrow_mut();
        if inner.updated_since_last_report {
            (&mut *self.callback.borrow_mut())(inner.aggregate());
            inner.updated_since_last_report = false;
        }
    }
}

/// Handle representing a slot in the progress reporter.
/// The handle may be used to update the status of an
/// individual transfer with the reporter.
pub(crate) struct ProgressUpdater {
    inner: Rc<RefCell<ProgressInner>>,
    index: usize,
}

impl ProgressUpdater {
    pub fn update(&self, progress: Progress) {
        self.inner.borrow_mut().update(self.index, progress);
    }
}

/// Shared state between the updater and reporter.
/// Those structs are expected to manager interior
/// mutability, so the methods on the shared state
/// are free to take exclusive references to self.
#[derive(Default)]
struct ProgressInner {
    progress: Vec<Progress>,
    first_byte_received: Option<Instant>,
    updated_since_last_report: bool,
}

impl ProgressInner {
    fn new_slot(&mut self) -> usize {
        let index = self.progress.len();
        self.progress.push(Default::default());
        index
    }

    fn update(&mut self, index: usize, progress: Progress) {
        self.progress[index] = progress;
        if self.first_byte_received.is_none() && progress.downloaded > 0 {
            self.first_byte_received = Some(Instant::now());
        }
        self.updated_since_last_report = true;
    }

    fn aggregate(&self) -> Progress {
        self.progress.iter().cloned().sum()
    }
}

/// Trait indiciating that a type is able to report progress
/// using the given updater. Transfer handlers will typically
/// implement this trait so that the code managing the transfers
/// can arrange for progress to be reported in a generic way.
pub(crate) trait MonitorProgress {
    fn monitor_progress(&mut self, updater: ProgressUpdater);
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

        updater2.update(p(4, 3, 2, 1));

        reporter.report_if_updated();

        let expected = vec![p(2, 4, 6, 8), p(5, 5, 5, 5)];
        assert_eq!(&expected, &reported);
    }
}
