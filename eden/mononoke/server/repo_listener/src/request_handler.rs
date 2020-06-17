/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use async_limiter::{AsyncLimiter, TokioFlavor};
use cached_config::ConfigHandle;
use context::{
    generate_session_id, is_quicksand, LoggingContainer, SessionContainer, SessionContainerBuilder,
    SessionId,
};
use failure_ext::SlogKVError;
use fbinit::FacebookInit;
use futures::compat::Future01CompatExt;
use futures_old::{Future, Sink, Stream};
use futures_stats::TimedFutureExt;
use hgproto::{sshproto, HgProtoHandler};
use lazy_static::lazy_static;
use limits::types::{MononokeThrottleLimit, MononokeThrottleLimits, RateLimits};
use load_limiter::{LoadLimiterBuilder, Metric};
use maplit::{hashmap, hashset};
use pushredirect_enable::types::MononokePushRedirectEnable;
use ratelimit_meter::{algorithms::LeakyBucket, DirectRateLimiter};
use repo_client::RepoClient;
use scuba_ext::ScubaSampleBuilderExt;
use slog::{self, error, info, o, warn, Drain, Level, Logger};
use slog_ext::SimpleFormatWithError;
use slog_kvfilter::KVFilter;
use sshrelay::{Priority, SenderBytesWrite, SshEnvVars, Stdio};
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
                    AsyncLimiter::new(
                        DirectRateLimiter::<LeakyBucket>::per_second(qps),
                        TokioFlavor::V01,
                    )
                    .await,
                );
            }

            if let Some(qps) = maybe_qps(tunables().get_wishlist_write_qps()) {
                builder.blobstore_write_limiter(
                    AsyncLimiter::new(
                        DirectRateLimiter::<LeakyBucket>::per_second(qps),
                        TokioFlavor::V01,
                    )
                    .await,
                );
            }
        }
        _ => {}
    }
}

pub async fn request_handler(
    fb: FacebookInit,
    RepoHandler {
        logger,
        mut scuba,
        wireproto_logging,
        repo,
        hash_validation_percentage,
        preserve_raw_bundle2,
        pure_push_allowed,
        maybe_push_redirector_args,
    }: RepoHandler,
    stdio: Stdio,
    load_limiting_config: Option<(ConfigHandle<MononokeThrottleLimits>, String)>,
    pushredirect_config: Option<ConfigHandle<MononokePushRedirectEnable>>,
    addr: IpAddr,
) {
    let Stdio {
        stdin,
        stdout,
        stderr,
        mut preamble,
    } = stdio;

    let session_id = match preamble
        .misc
        .get("session_uuid")
        .map(SessionId::from_string)
    {
        Some(session_id) => session_id,
        None => {
            let session_id = generate_session_id();
            preamble
                .misc
                .insert("session_uuid".to_owned(), session_id.to_string());
            session_id
        }
    };

    // Info per wireproto command within this session
    let wireproto_calls = Arc::new(Mutex::new(Vec::new()));
    let trace = TraceContext::new(TraceId::from_string(session_id.to_string()), Instant::now());

    // Per-connection logging drain that forks output to normal log and back to client stderr
    let conn_log = {
        let stderr_write = SenderBytesWrite {
            chan: stderr.wait(),
        };
        let client_drain = slog_term::PlainSyncDecorator::new(stderr_write);
        let client_drain = SimpleFormatWithError::new(client_drain);
        let client_drain = KVFilter::new(client_drain, Level::Critical).only_pass_any_on_all_keys(
            (hashmap! {
                "remote".into() => hashset!["true".into(), "remote_only".into()],
            })
            .into(),
        );

        let server_drain = KVFilter::new(logger, Level::Critical).always_suppress_any(
            (hashmap! {
                "remote".into() => hashset!["remote_only".into()],
            })
            .into(),
        );

        // Don't fail logging if the client goes away
        let drain = slog::Duplicate::new(client_drain, server_drain).ignore_res();
        Logger::root(drain, o!("session_uuid" => format!("{}", session_id)))
    };

    let priority = match Priority::extract_from_preamble(&preamble) {
        Ok(Some(p)) => {
            info!(&conn_log, "Using priority: {}", p; "remote" => "true");
            p
        }
        Ok(None) => Priority::Default,
        Err(e) => {
            warn!(&conn_log, "Could not parse priority: {}", e; "remote" => "true");
            Priority::Default
        }
    };

    scuba.add("priority", priority.to_string());
    scuba.log_with_msg("Connection established", None);

    let client_hostname = preamble
        .misc
        .get("source_hostname")
        .cloned()
        .unwrap_or("".to_string());

    let ssh_env_vars = SshEnvVars::from_map(&preamble.misc);
    let load_limiter = load_limiting_config.map(|(config, category)| {
        let (throttle_limits, rate_limits) =
            loadlimiting_configs(config, &client_hostname, &ssh_env_vars);
        LoadLimiterBuilder::build(fb, throttle_limits, rate_limits, category)
    });

    let client_ip = ssh_env_vars
        .ssh_client
        .as_ref()
        .and_then(|ssh_client| {
            // Parse SSH_CLIENT, if provided by the trusted peer.
            let raw = ssh_client.split_whitespace().next()?;
            let ip = raw.parse().ok()?;
            Some(ip)
        })
        .unwrap_or(addr); // Fallback to the peer's IP.

    let mut session_builder = SessionContainer::builder(fb)
        .session_id(session_id)
        .trace(trace.clone())
        .user_unix_name(preamble.misc.get("unix_username").cloned())
        .source_hostname(client_hostname)
        .ssh_env_vars(ssh_env_vars)
        .load_limiter(load_limiter)
        .user_ip(client_ip);

    set_blobstore_limiters(&mut session_builder, priority).await;

    let session = session_builder.build();

    let logging = LoggingContainer::new(conn_log.clone(), scuba.clone());

    // Construct a hg protocol handler
    let proto_handler = HgProtoHandler::new(
        conn_log.clone(),
        stdin,
        RepoClient::new(
            repo,
            session.clone(),
            logging,
            hash_validation_percentage,
            preserve_raw_bundle2,
            pure_push_allowed,
            wireproto_logging,
            maybe_push_redirector_args,
            pushredirect_config,
        ),
        sshproto::HgSshCommandDecode,
        sshproto::HgSshCommandEncode,
        wireproto_calls.clone(),
    );

    // send responses back
    let endres = proto_handler
        .inspect(move |bytes| session.bump_load(Metric::EgressBytes, bytes.len() as f64))
        .map_err(Error::from)
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
}

fn loadlimiting_configs(
    config: ConfigHandle<MononokeThrottleLimits>,
    client_hostname: &str,
    ssh_env_vars: &SshEnvVars,
) -> (MononokeThrottleLimit, RateLimits) {
    let is_quicksand = is_quicksand(&ssh_env_vars);

    let config = config.get();
    let region_percentage = config
        .datacenter_prefix_capacity
        .get(&*DATACENTER_REGION_PREFIX)
        .copied()
        .unwrap_or(DEFAULT_PERCENTAGE);
    let host_scheme = hostname_scheme(client_hostname);
    let limit = config
        .hostprefixes
        .get(host_scheme)
        .unwrap_or(&config.defaults);

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
