/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::LazyLock;
use std::sync::OnceLock;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

use futures::Future;
use parking_lot::RwLock;

pub trait Sink: Send + Sync {
    fn increment(&self, name: &'static str, value: i64);
}

static SINK: OnceLock<Arc<dyn Sink>> = OnceLock::new();

pub fn install_sink(sink: Arc<dyn Sink>) -> anyhow::Result<()> {
    SINK.set(sink)
        .map_err(|_| anyhow::anyhow!("metrics sink already initialized"))
}

// In-memory counter that can sync to an optional external sink.
pub struct Counter {
    name: &'static str,
    counter: OnceLock<Inner>,
    gauge: bool,
}

struct Inner {
    // In-memory counter value we increment eagerly.
    counter: AtomicUsize,
    // Last counter value we synced to the external sink.
    last_sync: AtomicUsize,
}

impl Counter {
    /// By convention metric name should be crate.metric_name
    /// Metrics without '.' in name are not allowed (cause compilation error)
    pub const fn new_counter(name: &'static str) -> Self {
        // Unfortunately we can't check name this here because of const restriction
        Self {
            name,
            counter: OnceLock::new(),
            gauge: false,
        }
    }

    pub const fn new_gauge(name: &'static str) -> Self {
        let mut counter = Self::new_counter(name);
        counter.gauge = true;
        counter
    }

    pub fn increment(&'static self) {
        self.add(1);
    }

    pub fn add(&'static self, val: usize) {
        let counter = &self.counter().counter;
        counter.fetch_add(val, Ordering::Relaxed);
        self.maybe_sync();
    }

    pub fn sub(&'static self, val: usize) {
        let counter = &self.counter().counter;
        counter.fetch_sub(val, Ordering::Relaxed);
        self.maybe_sync();
    }

    pub fn value(&'static self) -> usize {
        self.counter().counter.load(Ordering::Relaxed)
    }

    /// Increment counter by v and decrement it back by v when returned guard is dropped
    pub fn entrance_guard(&'static self, v: usize) -> EntranceGuard {
        self.add(v);
        EntranceGuard(self, v)
    }

    pub fn is_gauge(&'static self) -> bool {
        self.gauge
    }

    // Sync to the external sink 0.1% of the time.
    fn maybe_sync(&'static self) {
        if fastrand::f64() < 0.001 {
            self.sync();
        }
    }

    // Sync current counter value to the external sink, if necessary.
    fn sync(&'static self) {
        let Inner { counter, last_sync } = self.counter();

        loop {
            let current = counter.load(Ordering::Acquire);
            let last = last_sync.load(Ordering::Acquire);

            // Store `current` into `last_sync`, and then increment by the delta.
            if last_sync
                .compare_exchange(last, current, Ordering::AcqRel, Ordering::Relaxed)
                .is_err()
            {
                continue;
            }

            let delta = (current as i64) - (last as i64);
            if delta != 0 {
                if let Some(sink) = SINK.get() {
                    sink.increment(self.name, delta);
                }
            }
            break;
        }
    }

    fn counter(&'static self) -> &'static Inner {
        self.counter.get_or_init(|| {
            Registry::global().register_counter(self);
            Inner {
                counter: AtomicUsize::new(0),
                last_sync: AtomicUsize::new(0),
            }
        })
    }
}

pub struct EntranceGuard(&'static Counter, usize);

impl Drop for EntranceGuard {
    fn drop(&mut self) {
        self.0.sub(self.1);
    }
}

pub async fn wrap_future_keep_guards<F: Future>(
    future: F,
    _guards: Vec<EntranceGuard>,
) -> F::Output {
    future.await
}

#[derive(Default)]
pub struct Registry {
    counters: RwLock<HashMap<&'static str, &'static Counter>>,
}

impl Registry {
    pub fn global() -> &'static Self {
        static REGISTRY: LazyLock<Registry> = LazyLock::new(Registry::default);
        &REGISTRY
    }

    pub fn register_counter(&self, counter: &'static Counter) {
        if self
            .counters
            .write()
            .insert(counter.name, counter)
            .is_some()
        {
            panic!("Counter {} is duplicated", counter.name)
        }
    }

    pub fn counters(&self) -> HashMap<&'static str, &'static Counter> {
        self.counters.read().clone()
    }

    pub fn reset(&self) {
        for counter in self.counters.read().values() {
            counter.counter().counter.store(0, Ordering::Relaxed);
        }
    }

    // Sync all counters to the external sink.
    pub fn sync(&self) {
        for counter in self.counters.read().values() {
            counter.sync();
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn counters_test() {
        static COUNTER1: Counter = Counter::new_counter("COUNTER1");
        static COUNTER2: Counter = Counter::new_counter("COUNTER2");
        COUNTER1.increment();
        COUNTER2.add(5);
        let counters = Registry::global().counters();
        assert_eq!(1, counters.get("COUNTER1").unwrap().value());
        assert_eq!(5, counters.get("COUNTER2").unwrap().value());
    }

    #[test]
    fn entrance_test() {
        static COUNTER3: Counter = Counter::new_counter("COUNTER3");
        let guard1 = COUNTER3.entrance_guard(1);
        let counters = Registry::global().counters();
        assert_eq!(1, counters.get("COUNTER3").unwrap().value());
        std::mem::drop(guard1);
        let counters = Registry::global().counters();
        assert_eq!(0, counters.get("COUNTER3").unwrap().value());
    }
}
