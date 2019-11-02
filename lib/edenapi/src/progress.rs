/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::{cell::RefCell, rc::Rc, time::Instant};

pub use stats::ProgressStats;

mod stats;

pub type ProgressFn = Box<dyn FnMut(ProgressStats) + Send + 'static>;

pub struct ProgressUpdater {
    inner: Rc<RefCell<ProgressInner>>,
    index: usize,
}

impl ProgressUpdater {
    pub fn update(&self, stats: ProgressStats) {
        self.inner.borrow_mut().update(self.index, stats);
    }
}

pub struct ProgressReporter {
    inner: Rc<RefCell<ProgressInner>>,
    callback: Option<ProgressFn>,
}

impl ProgressReporter {
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            inner: Rc::new(RefCell::new(ProgressInner::with_capacity(capacity))),
            callback: None,
        }
    }

    pub fn set_callback(&mut self, f: Option<ProgressFn>) {
        self.callback = f;
    }

    pub fn new_updater(&self) -> ProgressUpdater {
        let inner = Rc::clone(&self.inner);
        let index = inner.borrow_mut().new_slot();
        ProgressUpdater { inner, index }
    }

    pub fn stats(&self) -> ProgressStats {
        self.inner.borrow().stats()
    }

    pub fn first_response_time(&self) -> Option<Instant> {
        self.inner.borrow().first_response.clone()
    }

    pub fn report(&mut self) {
        let stats = self.stats();
        if let Some(ref mut callback) = self.callback {
            callback(stats);
        }
    }
}

struct ProgressInner {
    stats: Vec<ProgressStats>,
    first_response: Option<Instant>,
}

impl ProgressInner {
    fn with_capacity(capacity: usize) -> Self {
        Self {
            stats: Vec::with_capacity(capacity),
            first_response: None,
        }
    }

    fn new_slot(&mut self) -> usize {
        let index = self.stats.len();
        self.stats.push(Default::default());
        index
    }

    fn update(&mut self, index: usize, stats: ProgressStats) {
        self.stats[index] = stats;
        if self.first_response.is_none() && stats.downloaded > 0 {
            self.first_response = Some(Instant::now());
        }
    }

    fn stats(&self) -> ProgressStats {
        self.stats.iter().cloned().sum()
    }
}
