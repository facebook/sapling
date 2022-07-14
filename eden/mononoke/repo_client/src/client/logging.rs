/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use context::CoreContext;
use context::PerfCounters;
use futures_stats::FutureStats;
use futures_stats::StreamStats;
use hgproto::GettreepackArgs;
use iterhelpers::chunk_by_accumulation;
use mercurial_types::HgManifestId;
use mononoke_types::MPath;
use scuba_ext::MononokeScubaSampleBuilder;
use scuba_ext::ScubaValue;
use scuba_ext::ScubaVerbosityLevel;
use std::collections::HashMap;
use std::sync::Arc;

const COLUMN_SIZE_LIMIT: usize = 500_1000;
const FULL_ARGS_LOG_TAG: &str = "Full Command Args";

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

    pub fn finalize_command<'a>(self, stats: impl Into<CommandStats<'a>>) {
        self.inner.log_command_processed(stats.into());
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

fn debug_format_directory<T: AsRef<[u8]>>(directory: &T) -> String {
    String::from_utf8_lossy(&hgproto::batch::escape(directory)).to_string()
}

pub fn debug_format_manifest(node: &HgManifestId) -> String {
    format!("{}", node)
}

pub fn debug_format_path(path: &Option<MPath>) -> String {
    match path {
        Some(p) => format!("{}", p),
        None => String::new(),
    }
}

fn greater_than_column_size(a: usize) -> bool {
    a > COLUMN_SIZE_LIMIT
}

pub fn log_gettreepack_params_verbose(ctx: &CoreContext, args: &GettreepackArgs) {
    if !ctx
        .scuba()
        .should_log_with_level(ScubaVerbosityLevel::Verbose)
    {
        return;
    }

    let mut sample = ctx.scuba().clone();
    sample.add("gettreepack_rootdir", debug_format_path(&args.rootdir));

    if let Some(depth) = args.depth {
        sample.add("gettreepack_depth", depth);
    }
    let msg = "gettreepack rootdir and depth".to_string();
    sample.log_with_msg_verbose(FULL_ARGS_LOG_TAG, msg);

    let mfnode_chunks = chunk_by_accumulation(
        args.mfnodes.iter().map(debug_format_manifest),
        0,
        |acc, s| acc + s.len(),
        greater_than_column_size,
    );

    let msg = "gettreepack mfnodes".to_string();
    for (i, mfnode_chunk) in mfnode_chunks.into_iter().enumerate() {
        ctx.scuba()
            .clone()
            .add("gettreepack_mfnode_chunk_idx", i)
            .add("gettreepack_mfnode_chunk", mfnode_chunk)
            .log_with_msg_verbose(FULL_ARGS_LOG_TAG, msg.clone());
    }

    let basemfnode_chunks = chunk_by_accumulation(
        args.basemfnodes.iter().map(debug_format_manifest),
        0,
        |acc, s| acc + s.len(),
        greater_than_column_size,
    );

    let msg = "gettreepack basemfnodes".to_string();
    for (i, basemfnode_chunk) in basemfnode_chunks.into_iter().enumerate() {
        ctx.scuba()
            .clone()
            .add("gettreepack_basemfnode_chunk_idx", i)
            .add("gettreepack_basemfnode_chunk", basemfnode_chunk)
            .log_with_msg_verbose(FULL_ARGS_LOG_TAG, msg.clone());
    }

    let directory_chunks = chunk_by_accumulation(
        args.directories.iter().map(debug_format_directory),
        0,
        |acc, s| acc + s.len(),
        greater_than_column_size,
    );
    let msg = "gettreepack directories".to_string();
    for (i, directory_chunk) in directory_chunks.into_iter().enumerate() {
        ctx.scuba()
            .clone()
            .add("gettreepack_directory_chunk_idx", i)
            .add("gettreepack_directory_chunk", directory_chunk)
            .log_with_msg_verbose(FULL_ARGS_LOG_TAG, msg.clone());
    }
}

pub fn log_getpack_params_verbose(ctx: &CoreContext, encoded_params: &[(String, Vec<String>)]) {
    if !ctx
        .scuba()
        .should_log_with_level(ScubaVerbosityLevel::Verbose)
    {
        return;
    }

    for (mpath, filenodes) in encoded_params {
        let filenode_chunks = chunk_by_accumulation(
            filenodes.iter().cloned(),
            0,
            |acc, s| acc + s.len(),
            greater_than_column_size,
        );
        for (i, filenode_chunk) in filenode_chunks.into_iter().enumerate() {
            ctx.scuba()
                .clone()
                .add("getpack_mpath", mpath.clone())
                .add("getpack_filenode_chunk_idx", i)
                .add("getpack_filenode_chunk", filenode_chunk)
                .log_with_msg_verbose(FULL_ARGS_LOG_TAG, None);
        }
    }
}
