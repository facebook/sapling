/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use fbinit::FacebookInit;
use scuba_ext::MononokeScubaSampleBuilder;
use slog::Logger;
use std::sync::atomic::AtomicU32;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use crate::perf_counters::PerfCounters;
use crate::perf_counters_stack::PerfCountersStack;
use scribe_ext::Scribe;
use slog::o;

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
    scuba: Arc<MononokeScubaSampleBuilder>,
    perf_counters: PerfCountersStack,
    sampling_key: Option<SamplingKey>,
    scribe: Scribe,
}

impl LoggingContainer {
    pub fn new(fb: FacebookInit, logger: Logger, scuba: MononokeScubaSampleBuilder) -> Self {
        Self {
            logger,
            scuba: Arc::new(scuba),
            perf_counters: Default::default(),
            sampling_key: None,
            scribe: Scribe::new(fb),
        }
    }

    pub fn fork_perf_counters(&mut self) -> Arc<PerfCounters> {
        let (perf_counters, ret) = self.perf_counters.fork();
        self.perf_counters = perf_counters;
        ret
    }

    pub fn clone_and_sample(&self, sampling_key: SamplingKey) -> Self {
        Self {
            logger: self.logger.clone(),
            scuba: self.scuba.clone(),
            perf_counters: self.perf_counters.clone(),
            sampling_key: Some(sampling_key),
            scribe: self.scribe.clone(),
        }
    }

    pub fn clone_with_logger(&self, logger: Logger) -> Self {
        Self {
            logger,
            scuba: self.scuba.clone(),
            perf_counters: self.perf_counters.clone(),
            sampling_key: self.sampling_key.clone(),
            scribe: self.scribe.clone(),
        }
    }

    pub fn clone_with_repo_name(&self, repo_name: &str) -> Self {
        Self {
            logger: self.logger.new(o!("repo" => repo_name.to_string())),
            scuba: self.scuba.clone(),
            perf_counters: self.perf_counters.clone(),
            sampling_key: self.sampling_key.clone(),
            scribe: self.scribe.clone(),
        }
    }

    pub fn with_scribe(&mut self, scribe: Scribe) -> &mut Self {
        self.scribe = scribe;
        self
    }

    pub fn logger(&self) -> &Logger {
        &self.logger
    }

    pub fn scuba(&self) -> &MononokeScubaSampleBuilder {
        &self.scuba
    }

    pub fn perf_counters(&self) -> &PerfCountersStack {
        &self.perf_counters
    }

    pub fn sampling_key(&self) -> Option<&SamplingKey> {
        self.sampling_key.as_ref()
    }

    pub fn scribe(&self) -> &Scribe {
        &self.scribe
    }

    pub fn with_mutated_scuba(
        &self,
        mutator: impl FnOnce(MononokeScubaSampleBuilder) -> MononokeScubaSampleBuilder,
    ) -> Self {
        Self {
            logger: self.logger.clone(),
            scuba: Arc::new(mutator(self.scuba().clone())),
            perf_counters: self.perf_counters.clone(),
            sampling_key: self.sampling_key.clone(),
            scribe: self.scribe.clone(),
        }
    }
}
