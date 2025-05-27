/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::borrow::Borrow;
use std::collections::HashMap;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;

use once_cell::sync::Lazy;
use parking_lot::RwLock;

pub fn increment_counter(key: impl Key, value: u64) {
    METRICS.increment_counter(key, value)
}

pub fn max_counter(key: impl Key, value: u64) {
    METRICS.max_counter(key, value)
}

pub fn summarize() -> HashMap<String, u64> {
    METRICS.summarize()
}

pub fn reset() {
    METRICS.reset();
}

pub trait Key: Into<String> + Borrow<str> {}
impl<T> Key for T where T: Into<String> + Borrow<str> {}

pub static METRICS: Lazy<Metrics> = Lazy::new(Metrics::new);

pub struct Metrics {
    counters: RwLock<HashMap<String, AtomicU64>>,
}

impl Metrics {
    fn new() -> Self {
        let counters = RwLock::new(HashMap::new());

        Self { counters }
    }

    fn increment_counter(&self, key: impl Key, value: u64) {
        self.get_or_create_counter(key, |counter| {
            // We could use Relaxed ordering but it makes tests awkward if we were to run on a
            // weakly ordered system, (stress) tests are nice for this code.
            counter.fetch_add(value, Ordering::Release);
        });
    }

    fn max_counter(&self, key: impl Key, new_value: u64) {
        self.get_or_create_counter(key, |counter| {
            let mut current_value = counter.load(Ordering::Relaxed);
            while current_value < new_value {
                match counter.compare_exchange(
                    current_value,
                    new_value,
                    Ordering::Relaxed,
                    Ordering::Relaxed,
                ) {
                    Ok(_) => break,
                    Err(updated_value) => current_value = updated_value,
                }
            }
        });
    }

    fn get_or_create_counter(&self, key: impl Key, cb: impl Fn(&AtomicU64)) {
        let counters = self.counters.read();
        if let Some(counter) = counters.get(key.borrow()) {
            cb(counter);
            return;
        }
        drop(counters);

        let mut counters = self.counters.write();
        cb(counters.entry(key.into()).or_default());
    }

    fn summarize(&self) -> HashMap<String, u64> {
        let counters = self.counters.read();
        counters
            .iter()
            .map(|(k, v)| (k.into(), v.load(Ordering::Acquire)))
            .collect()
    }

    fn reset(&self) {
        self.counters.write().clear();
    }
}

#[cfg(test)]
mod tests {
    use std::thread;

    use super::*;

    #[test]
    fn test_increment_string_key() {
        let metrics = Metrics::new();
        metrics.increment_counter(String::from("hello"), 2);
        metrics.increment_counter(String::from("world"), 3);
        metrics.increment_counter(String::from("hello"), 4);
        assert_eq!(
            metrics.summarize(),
            HashMap::from([(String::from("hello"), 6), (String::from("world"), 3)]),
        );
    }

    #[test]
    fn test_increment_str_key() {
        let metrics = Metrics::new();
        metrics.increment_counter("hello", 2);
        metrics.increment_counter("world", 3);
        metrics.increment_counter("hello", 4);
        assert_eq!(
            metrics.summarize(),
            HashMap::from([(String::from("hello"), 6), (String::from("world"), 3)])
        );
    }

    #[test]
    fn test_increment_on_many_threads() {
        static MY_METRICS: Lazy<Metrics> = Lazy::new(Metrics::new);
        let handle = thread::spawn(move || {
            for _i in 0..10000 {
                MY_METRICS.increment_counter("key", 2);
            }
        });
        for _i in 0..10000 {
            MY_METRICS.increment_counter("key", 3);
        }
        handle.join().expect("waiting for spawned thread");
        assert_eq!(
            MY_METRICS.summarize(),
            HashMap::from([(String::from("key"), 50000)])
        );
    }

    #[test]
    fn test_max_counter() {
        let metrics = Metrics::new();

        metrics.max_counter(String::from("hello"), 2);
        assert_eq!(
            metrics.summarize(),
            HashMap::from([(String::from("hello"), 2)]),
        );

        metrics.max_counter(String::from("hello"), 4);
        assert_eq!(
            metrics.summarize(),
            HashMap::from([(String::from("hello"), 4)]),
        );

        metrics.max_counter(String::from("hello"), 3);
        assert_eq!(
            metrics.summarize(),
            HashMap::from([(String::from("hello"), 4)]),
        );
    }
}
