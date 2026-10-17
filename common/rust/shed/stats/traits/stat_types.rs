/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is dual-licensed under either the MIT license found in the
 * LICENSE-MIT file in the root directory of this source tree or the Apache
 * License, Version 2.0 found in the LICENSE-APACHE file in the root directory
 * of this source tree. You may select, at your option, one of the
 * above-listed licenses.
 */

use auto_impl::auto_impl;
#[cfg(not(stats_backend = "noop"))]
use fbinit::FacebookInit;

#[cfg(not(stats_backend = "noop"))]
pub type BoxSingletonCounter = Box<dyn SingletonCounter + Send + Sync>;

/// A boxed counter for backends that do not require native initialization.
#[cfg(stats_backend = "noop")]
pub struct BoxSingletonCounter {
    inner: Box<dyn SingletonCounter<()> + Send + Sync>,
}

#[cfg(stats_backend = "noop")]
impl BoxSingletonCounter {
    /// Wrap a counter that does not require native initialization.
    pub fn new(inner: Box<dyn SingletonCounter<()> + Send + Sync>) -> Self {
        Self { inner }
    }

    /// Set the value, ignoring the compatibility initialization argument.
    pub fn set_value<Init>(&self, _init: Init, value: i64) {
        self.inner.set_value((), value);
    }

    /// Increment the value, ignoring the compatibility initialization argument.
    pub fn increment_value<Init>(&self, _init: Init, value: i64) {
        self.inner.increment_value((), value);
    }

    /// Get the value, ignoring the compatibility initialization argument.
    pub fn get_value<Init>(&self, _init: Init) -> Option<i64> {
        self.inner.get_value(())
    }
}

#[cfg(stats_backend = "noop")]
impl<Init> SingletonCounter<Init> for BoxSingletonCounter {
    fn set_value(&self, init: Init, value: i64) {
        BoxSingletonCounter::set_value(self, init, value);
    }

    fn increment_value(&self, init: Init, value: i64) {
        BoxSingletonCounter::increment_value(self, init, value);
    }

    fn get_value(&self, init: Init) -> Option<i64> {
        BoxSingletonCounter::get_value(self, init)
    }
}

pub type BoxCounter = Box<dyn Counter + Send + Sync>;
pub type BoxTimeseries = Box<dyn Timeseries + Send + Sync>;
pub type BoxHistogram = Box<dyn Histogram + Send + Sync>;
pub type BoxLocalCounter = Box<dyn Counter>;
pub type BoxLocalTimeseries = Box<dyn Timeseries>;
pub type BoxLocalHistogram = Box<dyn Histogram>;

macro_rules! define_singleton_counter {
    ($default:ty) => {
        /// SingletonCounter is a non-aggregated, global counter. Use this if you don't want any aggregation,
        /// and just want to expose a value through stats.
        #[auto_impl(Box)]
        pub trait SingletonCounter<Init = $default> {
            /// Sets the value of the counter
            fn set_value(&self, init: Init, value: i64);

            /// Increment the value of the counter
            fn increment_value(&self, init: Init, value: i64);

            /// Gets the current value of the counter
            fn get_value(&self, init: Init) -> Option<i64>;
        }
    };
}

#[cfg(not(stats_backend = "noop"))]
define_singleton_counter!(FacebookInit);
#[cfg(stats_backend = "noop")]
define_singleton_counter!(());

/// Counter is the simplest type of aggregated stat, it behaves as a single number that can be
/// incremented.
#[auto_impl(Box)]
pub trait Counter {
    /// Increments the counter by the given amount.
    fn increment_value(&self, value: i64);
}

/// Timeseries is a type of stat that can aggregate data send to it into
/// predefined intervals of time. Example aggregations are average, sum or rate.
#[auto_impl(Box)]
pub trait Timeseries {
    /// Adds value to the timeseries. It is being aggregated based on ExportType
    fn add_value(&self, value: i64);

    /// You might want to call this method when you have a very hot counter to avoid some
    /// congestions on it.
    /// Value is the sum of values of the samples and nsamples is the number of samples.
    /// Please notice that difference in the value semantic compared to
    /// `Histogram::add_repeated_value`.
    fn add_value_aggregated(&self, value: i64, nsamples: u32);
}

/// Histogram is a type of stat that can aggregate data send to it into
/// predefined buckets. Example aggregations are average, sum or P50 (percentile).
/// The aggregation should also happen on an interval basis, since its rarely
/// useful to see aggregated all-time stats of a service running for many days.
#[auto_impl(Box)]
pub trait Histogram {
    /// Adds value to the histogram. It is being aggregated based on ExportType
    fn add_value(&self, value: i64);

    /// You might want to call this method when you have a very hot counter to avoid some
    /// congestions on it. The default implementation simply calls add_value O(nsamples) times.
    /// If you have a performance-sensitive use case, check whether your Stats type has an O(1)
    /// implementation.
    /// Value is the value of a single samples and nsamples is the number of samples.
    /// Please notice that difference in the value semantic compared to
    /// `Timeseries::add_value_aggregated`.
    fn add_repeated_value(&self, value: i64, nsamples: u32) {
        for _ in 0..nsamples {
            self.add_value(value);
        }
    }

    /// Flush any buffered data so that it is observable externally. Should only
    /// be used for testing.
    fn flush(&self) {}
}

mod localkey_impls {
    use std::thread::LocalKey;

    use super::*;

    pub trait CounterStatic {
        fn increment_value(&'static self, value: i64);
    }

    impl<T: Counter> CounterStatic for LocalKey<T> {
        fn increment_value(&'static self, value: i64) {
            self.with(|s| T::increment_value(s, value));
        }
    }

    pub trait TimeseriesStatic {
        fn add_value(&'static self, value: i64);
        fn add_value_aggregated(&'static self, value: i64, nsamples: u32);
    }

    impl<T: Timeseries> TimeseriesStatic for LocalKey<T> {
        fn add_value(&'static self, value: i64) {
            self.with(|s| s.add_value(value));
        }

        fn add_value_aggregated(&'static self, value: i64, nsamples: u32) {
            self.with(|s| s.add_value_aggregated(value, nsamples));
        }
    }

    pub trait HistogramStatic {
        fn add_value(&'static self, value: i64);
        fn add_repeated_value(&'static self, value: i64, nsamples: u32);
    }

    impl<T: Histogram> HistogramStatic for LocalKey<T> {
        fn add_value(&'static self, value: i64) {
            self.with(|s| s.add_value(value));
        }

        fn add_repeated_value(&'static self, value: i64, nsamples: u32) {
            self.with(|s| s.add_repeated_value(value, nsamples));
        }
    }
}
pub use localkey_impls::*;

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_add_repeated_value() {
        // Arrange
        struct DummyHistogram {
            n_added: std::sync::atomic::AtomicU32,
        }

        impl Histogram for DummyHistogram {
            fn add_value(&self, _value: i64) {
                self.n_added
                    .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
            }
        }
        let dummy_histogram = DummyHistogram {
            n_added: std::sync::atomic::AtomicU32::new(0),
        };
        let n_to_add: u32 = 3;
        // Act
        dummy_histogram.add_repeated_value(0, n_to_add);
        // Assert
        assert_eq!(dummy_histogram.n_added.into_inner(), n_to_add);
    }

    #[test]
    fn boxed_counter_preserves_custom_initialization() {
        use std::sync::atomic::AtomicI64;
        use std::sync::atomic::Ordering;

        struct CustomCounter(AtomicI64);

        impl SingletonCounter<String> for CustomCounter {
            fn set_value(&self, init: String, value: i64) {
                assert_eq!(init, "set");
                self.0.store(value, Ordering::Relaxed);
            }

            fn increment_value(&self, init: String, value: i64) {
                assert_eq!(init, "increment");
                self.0.fetch_add(value, Ordering::Relaxed);
            }

            fn get_value(&self, init: String) -> Option<i64> {
                assert_eq!(init, "get");
                Some(self.0.load(Ordering::Relaxed))
            }
        }

        let counter: Box<dyn SingletonCounter<String> + Send + Sync> =
            Box::new(CustomCounter(AtomicI64::new(0)));
        SingletonCounter::set_value(&counter, String::from("set"), 40);
        SingletonCounter::increment_value(&counter, String::from("increment"), 2);
        assert_eq!(
            SingletonCounter::get_value(&counter, String::from("get")),
            Some(42)
        );
    }

    #[cfg(stats_backend = "noop")]
    #[test]
    fn boxed_glue_counter_forwards_to_inner() {
        use std::sync::atomic::AtomicI64;
        use std::sync::atomic::Ordering;

        struct UnitCounter(AtomicI64);

        impl SingletonCounter<()> for UnitCounter {
            fn set_value(&self, (): (), value: i64) {
                self.0.store(value, Ordering::Relaxed);
            }

            fn increment_value(&self, (): (), value: i64) {
                self.0.fetch_add(value, Ordering::Relaxed);
            }

            fn get_value(&self, (): ()) -> Option<i64> {
                Some(self.0.load(Ordering::Relaxed))
            }
        }

        let counter = BoxSingletonCounter::new(Box::new(UnitCounter(AtomicI64::new(0))));
        counter.set_value(String::from("set"), 40);
        counter.increment_value((), 2);
        assert_eq!(counter.get_value(String::from("get")), Some(42));

        let dynamic: &dyn SingletonCounter<String> = &counter;
        dynamic.increment_value(String::from("increment"), 3);
        assert_eq!(dynamic.get_value(String::from("get")), Some(45));
    }
}
