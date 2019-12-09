/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use blobstore::{Blobstore, BlobstoreBytes};
use chrono::Utc;
use context::{CoreContext, SessionId};
use fbinit::FacebookInit;
use futures::{future, Future};
use futures_ext::FutureExt;
use futures_stats::{FutureStats, StreamStats};
use rand::{distributions::Alphanumeric, thread_rng, Rng};
use scribe::ScribeClient;
use scuba_ext::{
    ScribeClientImplementation, ScubaSampleBuilder, ScubaSampleBuilderExt, ScubaValue,
};
use stats::{define_stats, Timeseries};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use time_ext::DurationExt;

define_stats! {
    prefix = "mononoke.repo_client.logging";

    wireproto_blobstore_success: timeseries(RATE, SUM),
    wireproto_blobstore_failure: timeseries(RATE, SUM),
    wireproto_scribe_success: timeseries(RATE, SUM),
    wireproto_scribe_failure: timeseries(RATE, SUM),
    wireproto_serialization_failure: timeseries(RATE, SUM),
}

pub struct WireprotoLogging {
    reponame: String,
    scribe_args: Option<(ScribeClientImplementation, String)>,
    blobstore_and_threshold: Option<(Arc<dyn Blobstore>, u64)>,
}

impl WireprotoLogging {
    pub fn new(
        fb: FacebookInit,
        reponame: String,
        scribe_category: Option<String>,
        blobstore_and_threshold: Option<(Arc<dyn Blobstore>, u64)>,
    ) -> Self {
        let scribe_args = scribe_category.map(|cat| (ScribeClientImplementation::new(fb), cat));
        Self {
            reponame,
            scribe_args,
            blobstore_and_threshold,
        }
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

    fn insert_stats<'b>(&self, scuba: &'b mut ScubaSampleBuilder) -> &'b mut ScubaSampleBuilder {
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
    pub fn new(ctx: CoreContext, command: String, wireproto: Arc<WireprotoLogging>) -> Self {
        let inner = ScubaOnlyCommandLogger::new(ctx);

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

        let session_id = inner.ctx.session_id().clone();

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
    extra: HashMap<String, ScubaValue>,
}

impl ScubaOnlyCommandLogger {
    fn new(ctx: CoreContext) -> Self {
        Self {
            ctx,
            extra: HashMap::new(),
        }
    }

    pub fn finalize_command<'a>(self, stats: impl Into<CommandStats<'a>>) {
        self.log_command_processed(stats.into());
    }

    pub fn add_scuba_extra(&mut self, k: impl Into<String>, v: impl Into<ScubaValue>) {
        self.extra.insert(k.into(), v.into());
    }

    fn log_command_processed<'a>(self, stats: CommandStats<'a>) {
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

    // Use a ScubaSampleBuilder to build a sample to send in Scribe. Reach into the other Scuba
    // sample to grab a few datapoints from there as well.
    let mut builder = ScubaSampleBuilder::with_discard();
    builder
        .add_common_server_data()
        .add("command", command)
        .add("duration", stats.completion_time().as_micros_unchecked())
        .add("source_control_server_type", "mononoke")
        .add("mononoke_session_uuid", session_id.into_string())
        .add("reponame", wireproto.reponame.clone());

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

                    blobstore
                        .put(ctx.clone(), key.clone(), BlobstoreBytes::from_bytes(args))
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
            .map(move |builder| {
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
            })
            .or_else(|_| Ok(()))
    });
    tokio::spawn(f);
}

fn generate_random_string(len: usize) -> String {
    thread_rng().sample_iter(&Alphanumeric).take(len).collect()
}
