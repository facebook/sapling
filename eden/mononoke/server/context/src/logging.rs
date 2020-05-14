/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use scuba_ext::ScubaSampleBuilder;
use slog::Logger;
use std::sync::{
    atomic::{AtomicU32, Ordering},
    Arc,
};

use crate::perf_counters::PerfCounters;

/// Used to correlation a high level action on a CoreContext
/// e.g. walk of a repo,  with low level actions using that context
/// e.g. which blobs are read
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub struct SamplingKey {
    inner_key: u32,
}

static NEXT_SAMPLING_KEY: AtomicU32 = AtomicU32::new(0);

impl SamplingKey {
    pub fn new() -> Self {
        let v = NEXT_SAMPLING_KEY.fetch_add(1, Ordering::Relaxed);
        Self { inner_key: v }
    }

    pub fn inner(&self) -> u32 {
        self.inner_key
    }
}

#[derive(Debug, Clone)]
pub struct LoggingContainer {
    logger: Logger,
    scuba: Arc<ScubaSampleBuilder>,
    perf_counters: Arc<PerfCounters>,
    sampling_key: Option<SamplingKey>,
}

impl LoggingContainer {
    pub fn new(logger: Logger, scuba: ScubaSampleBuilder) -> Self {
        Self {
            logger,
            scuba: Arc::new(scuba),
            perf_counters: Arc::new(PerfCounters::default()),
            sampling_key: None,
        }
    }

    pub fn clone_and_sample(&self, sampling_key: SamplingKey) -> Self {
        Self {
            logger: self.logger.clone(),
            scuba: self.scuba.clone(),
            perf_counters: self.perf_counters.clone(),
            sampling_key: Some(sampling_key),
        }
    }

    pub fn logger(&self) -> &Logger {
        &self.logger
    }

    pub fn scuba(&self) -> &ScubaSampleBuilder {
        &self.scuba
    }

    pub fn perf_counters(&self) -> &PerfCounters {
        &self.perf_counters
    }

    pub fn sampling_key(&self) -> Option<&SamplingKey> {
        self.sampling_key.as_ref()
    }
}
