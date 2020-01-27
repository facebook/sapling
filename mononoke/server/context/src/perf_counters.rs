/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use scuba_ext::ScubaSampleBuilder;
use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, Ordering};

macro_rules! define_perf_counters {
    (enum $enum_name:ident {
        $($variant:ident),*,
    }) => {
        #[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
        pub enum $enum_name {
            $($variant),*
        }

        impl $enum_name {
            pub fn name(&self) -> &'static str {
                match self {
                    $($enum_name::$variant => stringify!($variant)),*
                }
            }
        }

        pub const PERF_COUNTERS: &[PerfCounterType] = &[
            $($enum_name::$variant),*
        ];

        #[allow(non_snake_case)]
        #[derive(Debug, Default)]
        pub struct PerfCounters {
            $($variant: AtomicI64),*
        }

        impl PerfCounters {
            fn get_counter_atomic(&self, counter: $enum_name) -> &AtomicI64 {
                match counter {
                    $($enum_name::$variant => &self.$variant),*
                }
            }
        }
    };
}

define_perf_counters! {
    enum PerfCounterType {
        BlobGets,
        BlobGetsMaxLatency,
        BlobPresenceChecks,
        BlobPresenceChecksMaxLatency,
        BlobPuts,
        BlobPutsMaxLatency,
        CachelibHits,
        CachelibMisses,
        GetbundleNumCommits,
        GetfilesMaxFileSize,
        GetfilesMaxLatency,
        GetfilesNumFiles,
        GetfilesResponseSize,
        GetpackMaxFileSize,
        GetpackNumFiles,
        GetpackResponseSize,
        GettreepackNumTreepacks,
        GettreepackResponseSize,
        MemcacheHits,
        MemcacheMisses,
        SkiplistAncestorGen,
        SkiplistDescendantGen,
        SkiplistNoskipIterations,
        SkiplistSkipIterations,
        SkiplistSkippedGenerations,
        SqlReadsMaster,
        SqlReadsReplica,
        SqlWrites,
        SumManifoldPollTime,
        NullLinknode,
    }
}

impl PerfCounterType {
    pub(crate) fn log_in_separate_column(&self) -> bool {
        use PerfCounterType::*;

        match self {
            BlobGets
            | BlobGetsMaxLatency
            | BlobPresenceChecks
            | BlobPresenceChecksMaxLatency
            | BlobPuts
            | BlobPutsMaxLatency
            | CachelibHits
            | CachelibMisses
            | MemcacheHits
            | MemcacheMisses
            | SqlReadsMaster
            | SqlReadsReplica
            | SqlWrites => true,
            _ => false,
        }
    }
}

impl PerfCounters {
    pub fn set_counter(&self, counter: PerfCounterType, val: i64) {
        self.get_counter_atomic(counter)
            .store(val, Ordering::Relaxed);
    }

    pub fn increment_counter(&self, counter: PerfCounterType) {
        self.get_counter_atomic(counter)
            .fetch_add(1, Ordering::Relaxed);
    }

    pub fn decrement_counter(&self, counter: PerfCounterType) {
        self.get_counter_atomic(counter)
            .fetch_sub(1, Ordering::Relaxed);
    }

    pub fn add_to_counter(&self, counter: PerfCounterType, val: i64) {
        self.get_counter_atomic(counter)
            .fetch_add(val, Ordering::Relaxed);
    }

    pub fn set_max_counter(&self, counter: PerfCounterType, val: i64) {
        self.get_counter_atomic(counter)
            .fetch_max(val, Ordering::Relaxed);
    }

    pub fn get_counter(&self, counter: PerfCounterType) -> i64 {
        self.get_counter_atomic(counter).load(Ordering::Relaxed)
    }

    pub fn insert_perf_counters(&self, builder: &mut ScubaSampleBuilder) {
        let mut extra = HashMap::new();

        // NOTE: we log 0 to separate scuba columns mainly so that we can distinguish
        // nulls i.e. "not logged" and 0 as in "zero calls to blobstore". Logging 0 allows
        // counting avg, p50 etc statistic.
        // However we do not log 0 in extras to save space
        for key in PERF_COUNTERS.iter() {
            let value = self.get_counter(*key);

            if key.log_in_separate_column() {
                builder.add(key.name(), value);
            } else {
                if value != 0 {
                    extra.insert(key.name(), value);
                }
            }
        }

        if !extra.is_empty() {
            if let Ok(extra) = serde_json::to_string(&extra) {
                // Scuba does not support columns that are too long, we have to trim it
                let limit = ::std::cmp::min(extra.len(), 1000);
                builder.add("extra_context", &extra[..limit]);
            }
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_perf_counter() {
        // NOTE: This test doesn't try to do anything fancy or test concurrency. It does however
        // check that we pass valid values for Ordering (invalid values panic on atomics).
        let ctrs = PerfCounters::default();
        let k = PerfCounterType::BlobGets;

        ctrs.set_counter(k, 1);
        assert_eq!(ctrs.get_counter(k), 1);

        ctrs.increment_counter(k);
        assert_eq!(ctrs.get_counter(k), 2);

        ctrs.decrement_counter(k);
        assert_eq!(ctrs.get_counter(k), 1);

        ctrs.add_to_counter(k, 1);
        assert_eq!(ctrs.get_counter(k), 2);

        ctrs.set_max_counter(k, 3);
        assert_eq!(ctrs.get_counter(k), 3);

        ctrs.set_max_counter(k, 2);
        assert_eq!(ctrs.get_counter(k), 3);
    }
}
