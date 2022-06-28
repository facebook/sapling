/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use scuba_ext::MononokeScubaSampleBuilder;
use std::iter;
use std::sync::Arc;

use crate::perf_counters::PerfCounterType;
use crate::perf_counters::PerfCounters;

#[derive(Debug, Clone)]
pub struct PerfCountersStack {
    inner: Arc<PerfCountersStackInner>,
}

impl Default for PerfCountersStack {
    fn default() -> Self {
        Self {
            inner: Arc::new(Default::default()),
        }
    }
}

impl PerfCountersStack {
    pub(crate) fn fork(&self) -> (Self, Arc<PerfCounters>) {
        let new = Arc::new(PerfCounters::default());

        let mut inner = PerfCountersStackInner {
            top: self.inner.top.clone(),
            rest: self.inner.rest.clone(),
        };

        inner.rest.push(new.clone());

        (
            Self {
                inner: Arc::new(inner),
            },
            new,
        )
    }
}

/// Helper macro to implement that methods that operate on each of the counters in the stack.
macro_rules! impl_counter_methods {
    ( $( $vis:vis fn $method_name:ident(&self, $( $param_name:ident : $param_type:ty ),*); )* ) => {
        $(
            $vis fn $method_name(
                &self,
                $( $param_name: $param_type ),*
            ) {
                for c in self.iter_counters() {
                    c.$method_name($( $param_name, )*);
                }
            }
        )*
    }
}

impl PerfCountersStack {
    fn iter_counters(&self) -> impl Iterator<Item = &Arc<PerfCounters>> {
        self.inner.rest.iter().chain(iter::once(&self.inner.top))
    }

    impl_counter_methods! {
        pub fn set_counter(&self, counter: PerfCounterType, val: i64);
        pub fn increment_counter(&self, counter: PerfCounterType);
        pub fn decrement_counter(&self, counter: PerfCounterType);
        pub fn add_to_counter(&self, counter: PerfCounterType, val: i64);
        pub fn set_max_counter(&self, counter: PerfCounterType, val: i64);
    }

    pub fn top(&self) -> &PerfCounters {
        &self.inner.top
    }

    pub fn get_counter(&self, counter: PerfCounterType) -> i64 {
        self.inner.top.get_counter(counter)
    }

    pub fn insert_perf_counters(&self, builder: &mut MononokeScubaSampleBuilder) {
        self.inner.top.insert_perf_counters(builder)
    }
}

#[derive(Default, Debug)]
struct PerfCountersStackInner {
    top: Arc<PerfCounters>,
    rest: Vec<Arc<PerfCounters>>,
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_perf_counter_stack() {
        let k = PerfCounterType::BlobGets;

        let s = PerfCountersStack::default();
        s.increment_counter(k);
        assert_eq!(s.get_counter(k), 1);

        let (s, f1) = s.fork();
        assert_eq!(s.get_counter(k), 1);
        assert_eq!(f1.get_counter(k), 0);

        s.increment_counter(k);
        assert_eq!(s.get_counter(k), 2);
        assert_eq!(f1.get_counter(k), 1);

        let (s, f2) = s.fork();
        assert_eq!(s.get_counter(k), 2);
        assert_eq!(f1.get_counter(k), 1);
        assert_eq!(f2.get_counter(k), 0);

        s.increment_counter(k);
        assert_eq!(s.get_counter(k), 3);
        assert_eq!(f1.get_counter(k), 2);
        assert_eq!(f2.get_counter(k), 1);
    }
}
