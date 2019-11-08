/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use context::{CoreContext, SessionId};
use fbinit::FacebookInit;
use futures_stats::{FutureStats, StreamStats};
use metaconfig_types::WireprotoLoggingConfig;
use scribe::ScribeClient;
use scuba_ext::{
    ScribeClientImplementation, ScubaSampleBuilder, ScubaSampleBuilderExt, ScubaValue,
};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use time_ext::DurationExt;

pub struct WireprotoLogging {
    reponame: String,
    scribe_client: ScribeClientImplementation,
    config: WireprotoLoggingConfig,
}

impl WireprotoLogging {
    pub fn new(fb: FacebookInit, reponame: String, config: WireprotoLoggingConfig) -> Self {
        let scribe_client = ScribeClientImplementation::new(fb);

        Self {
            reponame,
            scribe_client,
            config,
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
    wireproto: Option<Arc<WireprotoLogging>>,
}

impl CommandLogger {
    pub fn new(
        ctx: CoreContext,
        command: String,
        wireproto: Option<Arc<WireprotoLogging>>,
    ) -> Self {
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

        if let Some(wireproto) = wireproto {
            do_wireproto_logging(wireproto, command, session_id, stats, args);
        }
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
    let sample_json = builder
        .add_common_server_data()
        .add("args", args)
        .add("command", command)
        .add("duration", stats.completion_time().as_micros_unchecked())
        .add("source_control_server_type", "mononoke")
        .add("mononoke_session_uuid", session_id.into_string())
        .add("reponame", wireproto.reponame.clone())
        .get_sample()
        .to_json();

    // We can't really do anything with the errors, so we ignore them.
    if let Ok(sample_json) = sample_json {
        let _ = wireproto
            .scribe_client
            .offer(&wireproto.config.scribe_category, &sample_json.to_string());
    }
}
