/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use scuba_ext::MononokeScubaSampleBuilder;
use std::sync::atomic::AtomicI64;
use std::sync::atomic::Ordering;

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
        BlobGetsNotFound,
        BlobGetsAccessWait,
        BlobGetsShardAccessWait,
        BlobGetsMaxLatency,
        BlobGetsNotFoundMaxLatency,
        BlobGetsDeduplicated,
        BlobGetsTotalSize,
        BlobPresenceChecks,
        BlobPresenceChecksMaxLatency,
        BlobPuts,
        BlobPutsAccessWait,
        BlobPutsShardAccessWait,
        BlobPutsMaxLatency,
        BlobPutsDeduplicated,
        BlobPutsTotalSize,
        BytesSent,
        CachelibHits,
        CachelibMisses,
        EdenapiFiles,
        EdenapiTrees,
        GetbundleFilenodesTotalWeight,
        GetbundleNumCommits,
        GetbundleNumDrafts,
        GetbundleNumFilenodes,
        GetbundleNumManifests,
        GetbundlePartialTraversal,
        GetcommitdataNumCommits,
        GetcommitdataResponseSize,
        GetpackMaxFileSize,
        GetpackNumFiles,
        GetpackNumPossibleLFSFiles,
        GetpackPossibleLFSFilesSumSize,
        GetpackResponseSize,
        FilenodesTooBigHistory,
        GettreepackDesignatedNodes,
        GettreepackNumTreepacks,
        GettreepackResponseSize,
        HgMutationStoreNumAdded,
        HgMutationStoreNumFetched,
        MemcacheHits,
        MemcacheMisses,
        NullLinknode,
        NumKnown,
        NumKnownRequested,
        NumUnknown,
        SegmentedChangelogServerSideOpsHits,
        SegmentedChangelogServerSideOpsFallbacks,
        SkiplistAncestorGen,
        SkiplistDescendantGen,
        SkiplistNoskipIterations,
        SkiplistSkipIterations,
        SkiplistSkippedGenerations,
        SqlReadsMaster,
        SqlReadsReplica,
        SqlWrites,
        SumManifoldPollTime,
        UndesiredTreeFetch,
        ManifoldBlobSumDelay,
        ManifoldBlobRetries,
        ManifoldBlobConflicts,
        S3BlobRetries,
        S3BlobSumDelay,
        S3AccessWait,
    }
}

enum PerfCounterTypeUpdateFunc {
    Add,
    Max,
}

impl PerfCounterType {
    /// Whether to log the zero value of the counter
    fn always_log(&self) -> bool {
        use PerfCounterType::*;

        match self {
            BlobGets
            | BlobGetsNotFound
            | BlobGetsMaxLatency
            | BlobGetsNotFoundMaxLatency
            | BlobGetsTotalSize
            | BlobPresenceChecks
            | BlobPresenceChecksMaxLatency
            | BlobPuts
            | BlobPutsMaxLatency
            | BlobPutsTotalSize
            | CachelibHits
            | CachelibMisses
            | GetpackNumPossibleLFSFiles
            | GetpackPossibleLFSFilesSumSize
            | GettreepackDesignatedNodes
            | MemcacheHits
            | MemcacheMisses
            | SqlReadsMaster
            | SqlReadsReplica
            | SqlWrites => true,
            _ => false,
        }
    }

    fn expected_update_func(&self) -> PerfCounterTypeUpdateFunc {
        use PerfCounterType::*;

        match self {
            BlobGets
            | BlobGetsNotFound
            | BlobGetsAccessWait
            | BlobGetsShardAccessWait
            | BlobGetsDeduplicated
            | BlobGetsTotalSize
            | BlobPresenceChecks
            | BlobPuts
            | BlobPutsAccessWait
            | BlobPutsShardAccessWait
            | BlobPutsDeduplicated
            | BlobPutsTotalSize
            | BytesSent
            | CachelibHits
            | CachelibMisses
            | EdenapiFiles
            | EdenapiTrees
            | GetbundleFilenodesTotalWeight
            | GetbundleNumCommits
            | GetbundleNumDrafts
            | GetbundleNumFilenodes
            | GetbundleNumManifests
            | GetbundlePartialTraversal
            | GetcommitdataNumCommits
            | GetcommitdataResponseSize
            | GetpackNumFiles
            | GetpackNumPossibleLFSFiles
            | GetpackPossibleLFSFilesSumSize
            | GetpackResponseSize
            | FilenodesTooBigHistory
            | GettreepackDesignatedNodes
            | GettreepackNumTreepacks
            | GettreepackResponseSize
            | HgMutationStoreNumAdded
            | HgMutationStoreNumFetched
            | MemcacheHits
            | MemcacheMisses
            | NullLinknode
            | NumKnown
            | NumKnownRequested
            | NumUnknown
            | SegmentedChangelogServerSideOpsHits
            | SegmentedChangelogServerSideOpsFallbacks
            | SkiplistAncestorGen
            | SkiplistDescendantGen
            | SkiplistNoskipIterations
            | SkiplistSkipIterations
            | SkiplistSkippedGenerations
            | SqlReadsMaster
            | SqlReadsReplica
            | SqlWrites
            | SumManifoldPollTime
            | UndesiredTreeFetch
            | ManifoldBlobSumDelay
            | ManifoldBlobRetries
            | ManifoldBlobConflicts
            | S3BlobRetries
            | S3BlobSumDelay
            | S3AccessWait => PerfCounterTypeUpdateFunc::Add,
            BlobGetsMaxLatency
            | BlobGetsNotFoundMaxLatency
            | BlobPresenceChecksMaxLatency
            | BlobPutsMaxLatency
            | GetpackMaxFileSize => PerfCounterTypeUpdateFunc::Max,
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

    pub fn update_with_counters(&self, counters: &PerfCounters) {
        for key in PERF_COUNTERS.iter() {
            match key.expected_update_func() {
                PerfCounterTypeUpdateFunc::Add => {
                    self.add_to_counter(*key, counters.get_counter(*key))
                }
                PerfCounterTypeUpdateFunc::Max => {
                    self.set_max_counter(*key, counters.get_counter(*key))
                }
            };
        }
    }

    pub fn insert_nonzero_perf_counters(&self, builder: &mut MononokeScubaSampleBuilder) {
        for key in PERF_COUNTERS.iter() {
            let value = self.get_counter(*key);
            if value != 0 {
                builder.add(key.name(), value);
            }
        }
    }

    pub fn insert_perf_counters(&self, builder: &mut MononokeScubaSampleBuilder) {
        // NOTE: we log 0 mainly so that we can distinguish
        // nulls i.e. "not logged" and 0 as in "zero calls to blobstore".
        // Logging 0 allows counting avg, p50 etc statistic.
        for key in PERF_COUNTERS.iter() {
            let value = self.get_counter(*key);
            if value != 0 || key.always_log() {
                builder.add(key.name(), value);
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
