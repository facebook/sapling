/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]

use fbinit::FacebookInit;
use futures_ext::BoxFuture;
use futures_stats::{FutureStats, StreamStats};
use itertools::join;
use observability::{ObservabilityContext, ScubaLoggingDecisionFields, ScubaVerbosityLevel};
pub use scuba::ScubaValue;
use scuba::{builder::ServerData, Sampling, ScubaSample, ScubaSampleBuilder};
use sshrelay::{Metadata, Preamble};
use std::collections::hash_map::Entry;
use std::convert::TryInto;
use std::io::Error as IoError;
use std::num::NonZeroU64;
use std::path::Path;
use std::time::Duration;
use time_ext::DurationExt;
use tracing::TraceContext;
use tunables::tunables;

#[cfg(fbcode_build)]
mod facebook;

pub use scribe_ext::ScribeClientImplementation;

/// An extensible wrapper struct around `ScubaSampleBuilder`
#[derive(Clone)]
pub struct MononokeScubaSampleBuilder {
    inner: ScubaSampleBuilder,
    maybe_observability_context: Option<ObservabilityContext>,
}

impl std::fmt::Debug for MononokeScubaSampleBuilder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "MononokeScubaSampleBuilder({:?})", self.inner)
    }
}

impl MononokeScubaSampleBuilder {
    pub fn new(fb: FacebookInit, scuba_table: &str) -> Self {
        Self {
            inner: ScubaSampleBuilder::new(fb, scuba_table),
            maybe_observability_context: None,
        }
    }

    pub fn with_discard() -> Self {
        Self {
            inner: ScubaSampleBuilder::with_discard(),
            maybe_observability_context: None,
        }
    }

    pub fn with_opt_table(fb: FacebookInit, scuba_table: Option<String>) -> Self {
        match scuba_table {
            None => Self::with_discard(),
            Some(scuba_table) => Self::new(fb, &scuba_table),
        }
    }

    pub fn with_observability_context(self, octx: ObservabilityContext) -> Self {
        Self {
            maybe_observability_context: Some(octx),
            ..self
        }
    }

    fn get_logging_decision_fields(&self) -> ScubaLoggingDecisionFields {
        ScubaLoggingDecisionFields {
            maybe_session_id: self.get("session_uuid"),
            maybe_unix_username: self.get("unix_username"),
            maybe_source_hostname: self.get("source_hostname"),
        }
    }

    fn should_log_with_level(&self, level: ScubaVerbosityLevel) -> bool {
        match level {
            ScubaVerbosityLevel::Normal => true,
            ScubaVerbosityLevel::Verbose => self
                .maybe_observability_context
                .as_ref()
                .map_or(false, |octx| {
                    octx.should_log_scuba_sample(level, self.get_logging_decision_fields())
                }),
        }
    }

    pub fn add<K: Into<String>, V: Into<ScubaValue>>(&mut self, key: K, value: V) -> &mut Self {
        self.inner.add(key, value);
        self
    }

    pub fn add_preamble(&mut self, preamble: &Preamble) -> &mut Self {
        self.inner.add("repo", preamble.reponame.as_ref());
        for (key, value) in preamble.misc.iter() {
            self.inner.add(key, value.as_ref());
        }
        self
    }

    pub fn add_metadata(&mut self, metadata: &Metadata) -> &mut Self {
        self.inner
            .add("session_uuid", metadata.session_id().to_string());
        self.inner
            .add("client_identities", join(metadata.identities().iter(), ","));

        if let Some(client_ip) = metadata.client_ip() {
            self.inner.add("client_ip", client_ip.to_string());
        }
        if let Some(client_hostname) = metadata.client_hostname() {
            // "source_hostname" to remain compatible with historical logging
            self.inner
                .add("source_hostname", client_hostname.to_owned());
        }
        if let Some(unix_name) = metadata.unix_name() {
            // "unix_username" to remain compatible with historical logging
            self.inner.add("unix_username", unix_name);
        }

        self
    }

    pub fn log_with_msg<S: Into<Option<String>>>(&mut self, log_tag: &str, msg: S) {
        self.inner.add("log_tag", log_tag);
        if let Some(mut msg) = msg.into() {
            match tunables().get_max_scuba_msg_length().try_into() {
                Ok(size) if size > 0 && msg.len() > size => {
                    msg.truncate(size);
                    msg.push_str(" (...)");
                }
                _ => {}
            };

            self.inner.add("msg", msg);
        }
        self.inner.log();
    }

    /// Same as `log_with_msg`, but sample is assumed to be verbose and is only logged
    /// if verbose logging conditions are met
    pub fn log_with_msg_verbose<S: Into<Option<String>>>(&mut self, log_tag: &str, msg: S) {
        if !self.should_log_with_level(ScubaVerbosityLevel::Verbose) {
            return;
        }

        self.log_with_msg(log_tag, msg)
    }

    pub fn add_stream_stats(&mut self, stats: &StreamStats) -> &mut Self {
        self.inner
            .add("poll_count", stats.poll_count)
            .add("poll_time_us", stats.poll_time.as_micros_unchecked())
            .add("count", stats.count)
            .add(
                "completion_time_us",
                stats.completion_time.as_micros_unchecked(),
            );

        self
    }

    pub fn add_future_stats(&mut self, stats: &FutureStats) -> &mut Self {
        self.inner
            .add("poll_count", stats.poll_count)
            .add("poll_time_us", stats.poll_time.as_micros_unchecked())
            .add(
                "completion_time_us",
                stats.completion_time.as_micros_unchecked(),
            );

        self
    }

    pub fn log_with_trace(&mut self, fb: FacebookInit, trace: &TraceContext) -> BoxFuture<(), ()> {
        #[cfg(not(fbcode_build))]
        {
            use futures_ext::FutureExt;
            let _ = (fb, trace);
            futures::future::ok(()).boxify()
        }
        #[cfg(fbcode_build)]
        {
            facebook::log_with_trace(self, fb, trace)
        }
    }

    pub fn is_discard(&self) -> bool {
        self.inner.is_discard()
    }

    pub fn sampled(&mut self, sample_rate: NonZeroU64) -> &mut Self {
        self.inner.sampled(sample_rate);
        self
    }

    pub fn unsampled(&mut self) -> &mut Self {
        self.inner.unsampled();
        self
    }

    pub fn log(&mut self) -> bool {
        self.inner.log()
    }

    /// Same as `log`, but sample is assumed to be verbose and is only logged
    /// if verbose logging conditions are met
    pub fn log_verbose(&mut self) -> bool {
        if !self.should_log_with_level(ScubaVerbosityLevel::Verbose) {
            // Return value of the `log` function indicates whether
            // the sample passed sampling. If it's too verbose, let's
            // return false
            return false;
        }

        self.log()
    }

    pub fn add_common_server_data(&mut self) -> &mut Self {
        self.inner.add_common_server_data();
        self
    }

    pub fn sampling(&self) -> &Sampling {
        self.inner.sampling()
    }

    pub fn add_mapped_common_server_data<F>(&mut self, mapper: F) -> &mut Self
    where
        F: Fn(ServerData) -> &'static str,
    {
        self.inner.add_mapped_common_server_data(mapper);
        self
    }

    pub fn with_log_file<L: AsRef<Path>>(mut self, log_file: L) -> Result<Self, IoError> {
        self.inner = self.inner.with_log_file(log_file)?;
        Ok(self)
    }

    pub fn with_seq(mut self, key: impl Into<String>) -> Self {
        self.inner = self.inner.with_seq(key);
        self
    }

    pub fn log_with_time(&mut self, time: u64) -> bool {
        self.inner.log_with_time(time)
    }

    pub fn entry<K: Into<String>>(&mut self, key: K) -> Entry<String, ScubaValue> {
        self.inner.entry(key)
    }

    pub fn flush(&self, timeout: Duration) {
        self.inner.flush(timeout)
    }

    pub fn get_sample(&self) -> &ScubaSample {
        self.inner.get_sample()
    }

    pub fn add_opt<K: Into<String>, V: Into<ScubaValue>>(
        &mut self,
        key: K,
        value: Option<V>,
    ) -> &mut Self {
        self.inner.add_opt(key, value);
        self
    }

    pub fn get<K: Into<String>>(&self, key: K) -> Option<&ScubaValue> {
        self.inner.get(key)
    }
}
