/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use scuba_ext::ScubaSampleBuilder;
use slog::Logger;
use std::sync::Arc;

use crate::perf_counters::PerfCounters;

#[derive(Debug, Clone)]
pub struct LoggingContainer {
    logger: Logger,
    scuba: Arc<ScubaSampleBuilder>,
    perf_counters: Arc<PerfCounters>,
}

impl LoggingContainer {
    pub fn new(logger: Logger, scuba: ScubaSampleBuilder) -> Self {
        Self {
            logger,
            scuba: Arc::new(scuba),
            perf_counters: Arc::new(PerfCounters::default()),
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
}
