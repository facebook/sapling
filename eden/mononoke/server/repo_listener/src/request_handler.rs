/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::errors::ErrorKind;
use crate::security_checker::ConnectionsSecurityChecker;
use std::collections::HashMap;

use anyhow::{anyhow, Context, Error, Result};
use async_limiter::AsyncLimiter;
use bytes::Bytes;
use cached_config::ConfigHandle;
use context::{
    is_quicksand, LoggingContainer, SessionClass, SessionContainer, SessionContainerBuilder,
    SessionId,
};
use failure_ext::SlogKVError;
use fbinit::FacebookInit;
use futures::compat::Future01CompatExt;
use futures_old::{sync::mpsc, Future, Stream};
use futures_stats::TimedFutureExt;
use hgproto::{sshproto, HgProtoHandler};
use lazy_static::lazy_static;
use limits::types::{MononokeThrottleLimit, MononokeThrottleLimits, RateLimits};
use live_commit_sync_config::CfgrLiveCommitSyncConfig;
use load_limiter::{LoadLimiterBuilder, Metric};
use maplit::{hashmap, hashset};
use ratelimit_meter::{algorithms::LeakyBucket, DirectRateLimiter};
use repo_client::RepoClient;
use scribe_ext::Scribe;
use scuba_ext::ScubaSampleBuilderExt;
use slog::{self, error, o, Drain, Level, Logger};
use slog_ext::SimpleFormatWithError;
use slog_kvfilter::KVFilter;
use sshrelay::{Metadata, Priority, SenderBytesWrite, Stdio};
use stats::prelude::*;
use std::convert::TryInto;
use std::mem;
use std::net::IpAddr;
use std::num::NonZeroU32;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use time_ext::DurationExt;
use tracing::{trace_args, TraceContext, TraceId, Traced};
use tunables::tunables;

use crate::repo_handlers::RepoHandler;

lazy_static! {
    static ref DATACENTER_REGION_PREFIX: String = {
        #[cfg(fbcode_build)]
        {
            ::fbwhoami::FbWhoAmI::get()
                .expect("failed to init fbwhoami")
                .region_datacenter_prefix
                .clone()
                .expect("failed to get region from fbwhoami")
        }
        #[cfg(not(fbcode_build))]
        {
            "global".to_owned()
        }
    };
}

const DEFAULT_PERCENTAGE: f64 = 100.0;

define_stats! {
    prefix = "mononoke.request_handler";
    wireproto_ms:
        histogram(500, 0, 100_000, Average, Sum, Count; P 5; P 25; P 50; P 75; P 95; P 97; P 99),
    request_success: timeseries(Rate, Sum),
    request_failure: timeseries(Rate, Sum),
    request_outcome_permille: timeseries(Average),
}

async fn set_blobstore_limiters(builder: &mut SessionContainerBuilder, priority: Priority) {
    fn maybe_qps(tunable: i64) -> Option<NonZeroU32> {
        let v = tunable.try_into().ok()?;
        NonZeroU32::new(v)
    }

    match priority {
        Priority::Wishlist => {
            if let Some(qps) = maybe_qps(tunables().get_wishlist_read_qps()) {
                builder.blobstore_read_limiter(
                    AsyncLimiter::new(DirectRateLimiter::<LeakyBucket>::per_second(qps)).await,
                );
            }

            if let Some(qps) = maybe_qps(tunables().get_wishlist_write_qps()) {
                builder.blobstore_write_limiter(
                    AsyncLimiter::new(DirectRateLimiter::<LeakyBucket>::per_second(qps)).await,
                );
            }
        }
        _ => {}
    }
}

pub async fn request_handler(
    fb: FacebookInit,
    reponame: String,
    repo_handlers: Arc<HashMap<String, RepoHandler>>,
    security_checker: Arc<ConnectionsSecurityChecker>,
    stdio: Stdio,
    load_limiting_config: Option<(ConfigHandle<MononokeThrottleLimits>, String)>,
    addr: IpAddr,
    maybe_live_commit_sync_config: Option<CfgrLiveCommitSyncConfig>,
    scribe: Scribe,
) -> Result<()> {
    let Stdio {
        stdin,
        stdout,
        stderr,
        metadata,
    } = stdio;

    let session_id = metadata.session_id();

    // We don't have a repository yet, so create without server drain
    let conn_log = create_conn_logger(stderr.clone(), None, Some(session_id));

    let handler = repo_handlers.get(&reponame).cloned().ok_or_else(|| {
        error!(
            conn_log,
            "Requested repo \"{}\" does not exist or is disabled", reponame;
            "remote" => "true"
        );

        anyhow!("unknown repo: {}", reponame)
    })?;

    let RepoHandler {
        logger,
        mut scuba,
        wireproto_logging,
        repo,
        preserve_raw_bundle2,
        maybe_push_redirector_args,
        maybe_warm_bookmarks_cache,
        repo_client_knobs,
    } = handler;

    // Upgrade log to include server drain
    let conn_log = create_conn_logger(stderr.clone(), Some(logger), Some(session_id));

    scuba = scuba.with_seq("seq");
    scuba.add("repo", reponame);
    scuba.add_metadata(&metadata);

    let reponame = repo.reponame();

    if !metadata.is_trusted_client() {
        let is_allowed_to_repo = security_checker
            .check_if_repo_access_allowed(reponame, metadata.identities())
            .await
            .with_context(|| {
                format!(
                    "failed to check if access to repo '{}' is allowed for client '{}' with identity set '{:#?}'.",
                    reponame,
                    addr,
                    metadata.identities(),
                )
            })?;

        if !is_allowed_to_repo {
            let err: Error = ErrorKind::AuthorizationFailed.into();
            scuba.log_with_msg("Authorization failed", format!("{}", err));
            error!(conn_log, "Authorization failed: {}", err; "remote" => "true");

            return Err(err);
        }
    }

    // Info per wireproto command within this session
    let wireproto_calls = Arc::new(Mutex::new(Vec::new()));
    let trace = TraceContext::new(TraceId::from_string(session_id.to_string()), Instant::now());

    let priority = metadata.priority();
    scuba.add("priority", priority.to_string());
    scuba.log_with_msg("Connection established", None);

    let load_limiter = load_limiting_config.map(|(config, category)| {
        let (throttle_limits, rate_limits) = loadlimiting_configs(config, &metadata);
        LoadLimiterBuilder::build(fb, throttle_limits, rate_limits, category)
    });

    let mut session_builder = SessionContainer::builder(fb)
        .trace(trace.clone())
        .metadata(metadata.clone())
        .load_limiter(load_limiter);

    if priority == &Priority::Wishlist {
        session_builder = session_builder.session_class(SessionClass::Background);
    }
    set_blobstore_limiters(&mut session_builder, *priority).await;

    let session = session_builder.build();

    let mut logging = LoggingContainer::new(fb, conn_log.clone(), scuba.clone());
    logging.with_scribe(scribe);

    // Construct a hg protocol handler
    let proto_handler = HgProtoHandler::new(
        conn_log.clone(),
        stdin.map(bytes_ext::copy_from_new),
        RepoClient::new(
            repo,
            session.clone(),
            logging,
            preserve_raw_bundle2,
            wireproto_logging,
            maybe_push_redirector_args,
            maybe_live_commit_sync_config,
            maybe_warm_bookmarks_cache,
            repo_client_knobs,
        ),
        sshproto::HgSshCommandDecode,
        sshproto::HgSshCommandEncode,
        wireproto_calls.clone(),
    );

    // send responses back
    let endres = proto_handler
        .inspect(move |bytes| session.bump_load(Metric::EgressBytes, bytes.len() as f64))
        .map_err(Error::from)
        .map(bytes_ext::copy_from_old)
        .forward(stdout)
        .map(|_| ());

    // If we got an error at this point, then catch it and print a message
    let (stats, result) = endres
        .traced(&trace, "wireproto request", trace_args!())
        .compat()
        .timed()
        .await;

    let wireproto_calls = {
        let mut wireproto_calls = wireproto_calls.lock().expect("lock poisoned");
        mem::replace(&mut *wireproto_calls, Vec::new())
    };

    STATS::wireproto_ms.add_value(stats.completion_time.as_millis_unchecked() as i64);

    let mut scuba = scuba.clone();

    scuba
        .add_future_stats(&stats)
        .add("wireproto_commands", wireproto_calls);

    // Populate stats no matter what to avoid dead detectors firing.
    STATS::request_success.add_value(0);
    STATS::request_failure.add_value(0);

    match &result {
        Ok(_) => {
            STATS::request_success.add_value(1);
            STATS::request_outcome_permille.add_value(1000);
            scuba.log_with_msg("Request finished - Success", None)
        }
        Err(err) => {
            STATS::request_failure.add_value(1);
            STATS::request_outcome_permille.add_value(0);
            scuba.log_with_msg("Request finished - Failure", format!("{:#?}", err));
        }
    }

    if let Err(err) = result {
        error!(&conn_log, "Command failed";
            SlogKVError(err),
            "remote" => "true"
        );
    }

    // NOTE: This results a Result that we ignore here. There isn't really anything we can (or
    // should) do if this errors out.
    let _ = scuba.log_with_trace(fb, &trace).compat().await;

    Ok(())
}

fn loadlimiting_configs(
    config: ConfigHandle<MononokeThrottleLimits>,
    metadata: &Metadata,
) -> (MononokeThrottleLimit, RateLimits) {
    let is_quicksand = is_quicksand(&metadata);

    let config = config.get();
    let region_percentage = config
        .datacenter_prefix_capacity
        .get(&*DATACENTER_REGION_PREFIX)
        .copied()
        .unwrap_or(DEFAULT_PERCENTAGE);
    let limit = match metadata.client_hostname() {
        Some(client_hostname) => {
            let host_scheme = hostname_scheme(client_hostname);
            config
                .hostprefixes
                .get(host_scheme)
                .unwrap_or(&config.defaults)
        }
        None => &config.defaults,
    };

    let multiplier = if is_quicksand {
        region_percentage / 100.0 * config.quicksand_multiplier
    } else {
        region_percentage / 100.0
    };

    let throttle_limits = MononokeThrottleLimit {
        egress_bytes: limit.egress_bytes * multiplier,
        ingress_blobstore_bytes: limit.ingress_blobstore_bytes * multiplier,
        total_manifests: limit.total_manifests * multiplier,
        quicksand_manifests: limit.quicksand_manifests * multiplier,
        getfiles_files: limit.getfiles_files * multiplier,
        getpack_files: limit.getpack_files * multiplier,
        commits: limit.commits * multiplier,
    };

    (throttle_limits, config.rate_limits.clone())
}

/// Translates a hostname in to a host scheme:
///   devvm001.lla1.facebook.com -> devvm
///   hg001.lla1.facebook.com -> hg
fn hostname_scheme(hostname: &str) -> &str {
    let index = hostname.find(|c: char| !c.is_ascii_alphabetic());
    match index {
        Some(index) => hostname.split_at(index).0,
        None => hostname,
    }
}

pub fn create_conn_logger(
    stderr: mpsc::UnboundedSender<Bytes>,
    server_logger: Option<Logger>,
    session_id: Option<&SessionId>,
) -> Logger {
    let session_id = match session_id {
        Some(session_id) => session_id.to_string(),
        None => "".to_string(),
    };
    let decorator = o!("session_uuid" => format!("{}", session_id));

    let stderr_write = SenderBytesWrite { chan: stderr };
    let client_drain = slog_term::PlainSyncDecorator::new(stderr_write);
    let client_drain = SimpleFormatWithError::new(client_drain);
    let client_drain = KVFilter::new(client_drain, Level::Critical).only_pass_any_on_all_keys(
        (hashmap! {
            "remote".into() => hashset!["true".into(), "remote_only".into()],
        })
        .into(),
    );

    if let Some(logger) = server_logger {
        let server_drain = KVFilter::new(logger, Level::Critical).always_suppress_any(
            (hashmap! {
                "remote".into() => hashset!["remote_only".into()],
            })
            .into(),
        );

        // Don't fail logging if the client goes away
        let drain = slog::Duplicate::new(client_drain, server_drain).ignore_res();
        Logger::root(drain, decorator)
    } else {
        Logger::root(client_drain.ignore_res(), decorator)
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_hostname_scheme() {
        assert_eq!(hostname_scheme("devvm001.lla1.facebook.com"), "devvm");
        assert_eq!(hostname_scheme("hg001.lla1.facebook.com"), "hg");
        assert_eq!(hostname_scheme("ololo"), "ololo");
        assert_eq!(hostname_scheme(""), "");
    }
}
