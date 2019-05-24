// Copyright Facebook, Inc. 2019

use std::{cell::RefCell, rc::Rc, time::Instant};

pub use stats::ProgressStats;

mod stats;

pub type ProgressFn = Box<dyn FnMut(ProgressStats) + Send + 'static>;

pub struct ProgressHandle {
    inner: Rc<RefCell<ProgressManagerInner>>,
    index: usize,
}

impl ProgressHandle {
    pub fn update(&self, stats: ProgressStats) {
        self.inner.borrow_mut().update(self.index, stats);
    }
}

pub struct ProgressManager {
    inner: Rc<RefCell<ProgressManagerInner>>,
    callback: Option<ProgressFn>,
}

impl ProgressManager {
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            inner: Rc::new(RefCell::new(ProgressManagerInner::with_capacity(capacity))),
            callback: None,
        }
    }

    pub fn set_callback(&mut self, f: Option<ProgressFn>) {
        self.callback = f;
    }

    pub fn register(&self) -> ProgressHandle {
        let inner = Rc::clone(&self.inner);
        let index = inner.borrow_mut().register();
        ProgressHandle { inner, index }
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

struct ProgressManagerInner {
    stats: Vec<ProgressStats>,
    first_response: Option<Instant>,
}

impl ProgressManagerInner {
    fn with_capacity(capacity: usize) -> Self {
        Self {
            stats: Vec::with_capacity(capacity),
            first_response: None,
        }
    }

    fn register(&mut self) -> usize {
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
