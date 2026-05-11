/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::sync::Arc;

use context::CoreContext;
use context::PerfCounters;
use futures_stats::FutureStats;
use futures_stats::StreamStats;
use scuba_ext::MononokeScubaSampleBuilder;
use scuba_ext::ScubaValue;

#[derive(Copy, Clone)]
pub enum CommandStats<'a> {
    Future(&'a FutureStats),
    Stream(&'a StreamStats),
}

impl<'a> CommandStats<'a> {
    fn insert_stats<'b>(
        &self,
        scuba: &'b mut MononokeScubaSampleBuilder,
    ) -> &'b mut MononokeScubaSampleBuilder {
        match self {
            Self::Future(stats) => scuba.add_future_stats(stats),
            Self::Stream(stats) => scuba.add_stream_stats(stats),
        }
    }
}

impl<'a> From<&'a FutureStats> for CommandStats<'a> {
    fn from(stats: &'a FutureStats) -> Self {
        Self::Future(stats)
    }
}

impl<'a> From<&'a StreamStats> for CommandStats<'a> {
    fn from(stats: &'a StreamStats) -> Self {
        Self::Stream(stats)
    }
}

/// Logs wireproto requests both to scuba.
/// Scuba logs are used for analysis of performance.
#[must_use = "A CommandLogger does not do anything if you don't use it"]
pub struct CommandLogger {
    inner: ScubaOnlyCommandLogger,
}

impl CommandLogger {
    pub fn new(ctx: CoreContext, request_perf_counters: Arc<PerfCounters>) -> Self {
        let inner = ScubaOnlyCommandLogger::new(ctx, request_perf_counters);

        Self { inner }
    }

    /// Opts-out of replaying the wireproto request on the shadow tier.
    /// Returns a simplified logger that does Scuba-only logging.
    pub fn without_wireproto(self) -> ScubaOnlyCommandLogger {
        self.inner
    }

    pub fn add_scuba_extra(&mut self, k: impl Into<String>, v: impl Into<ScubaValue>) {
        self.inner.add_scuba_extra(k, v);
    }

    pub fn add_trimmed_scuba_extra(&mut self, k: impl Into<String>, args: &serde_json::Value) {
        if let Ok(args) = serde_json::to_string(args) {
            let limit = ::std::cmp::min(args.len(), 1000);
            self.add_scuba_extra(k, &args[..limit]);
        }
    }
}

#[must_use = "A CommandLogger does not do anything if you don't use it"]
pub struct ScubaOnlyCommandLogger {
    ctx: CoreContext,
    request_perf_counters: Arc<PerfCounters>,
    extra: HashMap<String, ScubaValue>,
}

impl ScubaOnlyCommandLogger {
    fn new(ctx: CoreContext, request_perf_counters: Arc<PerfCounters>) -> Self {
        Self {
            ctx,
            request_perf_counters,
            extra: HashMap::new(),
        }
    }

    pub fn finalize_command<'a>(self, stats: impl Into<CommandStats<'a>>) {
        self.log_command_processed(stats.into());
    }

    pub fn add_scuba_extra(&mut self, k: impl Into<String>, v: impl Into<ScubaValue>) {
        self.extra.insert(k.into(), v.into());
    }

    fn log_command_processed(self, stats: CommandStats) {
        self.request_perf_counters
            .update_with_counters(self.ctx.perf_counters().top());
        let mut scuba = self.ctx.scuba().clone();
        stats.insert_stats(&mut scuba);
        self.ctx.perf_counters().insert_perf_counters(&mut scuba);

        for (k, v) in self.extra.into_iter() {
            scuba.add(k, v);
        }

        scuba.log_with_msg("Command processed", None);
    }
}
