/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use blobstore::{Blobstore, BlobstoreBytes};
use chrono::Utc;
use cloned::cloned;
use context::{CoreContext, PerfCounters, SessionId};
use fbinit::FacebookInit;
use futures::{compat::Future01CompatExt, FutureExt, TryFutureExt};
use futures_01_ext::FutureExt as _;
use futures_old::{future, Future};
use futures_stats::{FutureStats, StreamStats};
use hgproto::GettreepackArgs;
use iterhelpers::chunk_by_accumulation;
use mercurial_types::HgManifestId;
use mononoke_types::MPath;
use rand::{distributions::Alphanumeric, thread_rng, Rng};
#[cfg(fbcode_build)]
use scribe::ScribeClient;
use scuba_ext::ScubaVerbosityLevel;
use scuba_ext::{MononokeScubaSampleBuilder, ScribeClientImplementation, ScubaValue};
use stats::prelude::*;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use time_ext::DurationExt;

define_stats! {
    prefix = "mononoke.repo_client.logging";

    wireproto_blobstore_success: timeseries(Rate, Sum),
    wireproto_blobstore_failure: timeseries(Rate, Sum),
    wireproto_scribe_success: timeseries(Rate, Sum),
    wireproto_scribe_failure: timeseries(Rate, Sum),
    wireproto_serialization_failure: timeseries(Rate, Sum),
}

const COLUMN_SIZE_LIMIT: usize = 500_1000;
const FULL_ARGS_LOG_TAG: &str = "Full Command Args";

pub struct WireprotoLogging {
    reponame: String,
    scribe_args: Option<(ScribeClientImplementation, String)>,
    blobstore_and_threshold: Option<(Arc<dyn Blobstore>, u64)>,
    scuba_builder: MononokeScubaSampleBuilder,
}

impl WireprotoLogging {
    pub fn new(
        fb: FacebookInit,
        reponame: String,
        scribe_category: Option<String>,
        blobstore_and_threshold: Option<(Arc<dyn Blobstore>, u64)>,
        log_file: Option<&str>,
    ) -> Result<Self, Error> {
        let scribe_args = scribe_category.map(|cat| (ScribeClientImplementation::new(fb), cat));

        // We use a Scuba sample builder to produce samples to log. We also use that to allow
        // logging to a file: we never log to an actual Scuba category here.
        let mut scuba_builder = MononokeScubaSampleBuilder::with_discard();
        scuba_builder.add_common_server_data();
        if let Some(log_file) = log_file {
            scuba_builder = scuba_builder.with_log_file(log_file)?;
        }

        Ok(Self {
            reponame,
            scribe_args,
            blobstore_and_threshold,
            scuba_builder,
        })
    }
}

#[derive(Copy, Clone)]
pub enum CommandStats<'a> {
    Future(&'a FutureStats),
    Stream(&'a StreamStats),
}

impl<'a> CommandStats<'a> {
    pub fn completion_time(&self) -> Duration {
        match self {
            Self::Future(ref stats) => stats.completion_time,
            Self::Stream(ref stats) => stats.completion_time,
        }
    }

    fn insert_stats<'b>(
        &self,
        scuba: &'b mut MononokeScubaSampleBuilder,
    ) -> &'b mut MononokeScubaSampleBuilder {
        match self {
            Self::Future(ref stats) => scuba.add_future_stats(stats),
            Self::Stream(ref stats) => scuba.add_stream_stats(stats),
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

/// Logs wireproto requests both to scuba and scribe.
/// Scuba logs are used for analysis of performance of both shadow and prod Mononoke tiers
/// Scribe logs are used for replaying prod wireproto requests on shadow tier. So
/// Scribe logging should be disabled on shadow tier.
#[must_use = "A CommandLogger does not do anything if you don't use it"]
pub struct CommandLogger {
    inner: ScubaOnlyCommandLogger,
    command: String,
    /// This scribe category main purpose is to tail the prod requests and replay them on the
    /// shadow tier.
    wireproto: Arc<WireprotoLogging>,
}

impl CommandLogger {
    pub fn new(
        ctx: CoreContext,
        command: String,
        wireproto: Arc<WireprotoLogging>,
        request_perf_counters: Arc<PerfCounters>,
    ) -> Self {
        let inner = ScubaOnlyCommandLogger::new(ctx, request_perf_counters);

        Self {
            inner,
            command,
            wireproto,
        }
    }

    /// Opts-out of replaying the wireproto request on the shadow tier.
    /// Returns a simplified logger that does Scuba-only logging.
    pub fn without_wireproto(self) -> ScubaOnlyCommandLogger {
        self.inner
    }

    pub fn finalize_command<'a>(
        self,
        ctx: CoreContext,
        stats: impl Into<CommandStats<'a>>,
        args: Option<&serde_json::Value>,
    ) {
        let stats = stats.into();
        let Self {
            inner,
            command,
            wireproto,
        } = self;

        let session_id = inner.ctx.metadata().session_id().clone();

        inner.log_command_processed(stats);

        do_wireproto_logging(ctx, wireproto, command, session_id, stats, args);
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

fn do_wireproto_logging<'a>(
    ctx: CoreContext,
    wireproto: Arc<WireprotoLogging>,
    command: String,
    session_id: SessionId,
    stats: CommandStats<'a>,
    args: Option<&serde_json::Value>,
) {
    let args = args
        .map(|a| a.to_string())
        .unwrap_or_else(|| "".to_string());

    // Use a MononokeScubaSampleBuilder to build a sample to send in Scribe. Reach into the other Scuba
    // sample to grab a few datapoints from there as well.
    let mut builder = wireproto.scuba_builder.clone();
    builder
        .add("command", command)
        .add("duration", stats.completion_time().as_micros_unchecked())
        .add("source_control_server_type", "mononoke")
        .add("mononoke_session_uuid", session_id.into_string())
        .add("reponame", wireproto.reponame.clone());

    if let Some(client_hostname) = ctx.session().metadata().client_hostname() {
        builder.add("client_hostname", client_hostname.clone());
    }

    let f = future::lazy(move || {
        let prepare_fut = match wireproto.blobstore_and_threshold {
            Some((ref blobstore, ref remote_arg_size_threshold)) => {
                if args.len() as u64 > *remote_arg_size_threshold {
                    // Key is generated randomly. Another option would be to
                    // take a hash of arguments, but I don't want to spend cpu cycles on
                    // computing hashes. Random string should be good enough.

                    let key = format!(
                        "wireproto_replay.{}.{}",
                        Utc::now().to_rfc3339(),
                        generate_random_string(16),
                    );

                    {
                        cloned!(ctx, blobstore, key);
                        async move {
                            blobstore
                                .put(&ctx, key, BlobstoreBytes::from_bytes(args))
                                .await
                        }
                    }
                    .boxed()
                    .compat()
                    .map(move |()| {
                        STATS::wireproto_blobstore_success.add_value(1);
                        builder.add("remote_args", key);
                        builder
                    })
                    .inspect_err(|_| {
                        STATS::wireproto_blobstore_failure.add_value(1);
                    })
                    .left_future()
                } else {
                    builder.add("args", args);
                    future::ok(builder).right_future()
                }
            }
            None => {
                builder.add("args", args);
                future::ok(builder).right_future()
            }
        };

        prepare_fut
            .map(move |mut builder| {
                // We use the Scuba sample and log it to Scribe, then we also log in the Scuba
                // sample, but this is built using discard(), so at most it'll log to a file for
                // debug / tests.

                let sample = builder.get_sample();
                // We can't really do anything with the errors, so let's just log them
                if let Some((ref scribe_client, ref scribe_category)) = wireproto.scribe_args {
                    if let Ok(sample_json) = sample.to_json() {
                        let res = scribe_client.offer(scribe_category, &sample_json.to_string());
                        if res.is_ok() {
                            STATS::wireproto_scribe_success.add_value(1);
                        } else {
                            STATS::wireproto_scribe_failure.add_value(1);
                        }
                    } else {
                        STATS::wireproto_serialization_failure.add_value(1);
                    }
                }

                builder.log();
            })
            .or_else(|_| Result::<_, Error>::Ok(()))
    });
    tokio::spawn(f.compat());
}

fn generate_random_string(len: usize) -> String {
    thread_rng()
        .sample_iter(&Alphanumeric)
        .map(char::from)
        .take(len)
        .collect()
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
