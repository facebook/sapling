/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Arc;
use std::sync::Mutex;

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use bytes::Bytes;
use connection_security_checker::ConnectionSecurityChecker;
use context::LoggingContainer;
use context::SessionContainer;
use context::SessionId;
use failure_ext::SlogKVError;
use fbinit::FacebookInit;
use futures::compat::Future01CompatExt;
use futures_old::sync::mpsc;
use futures_old::Future;
use futures_old::Stream;
use futures_stats::TimedFutureExt;
use hgproto::sshproto;
use hgproto::HgProtoHandler;
use maplit::hashmap;
use maplit::hashset;
use qps::Qps;
use rate_limiting::Metric;
use rate_limiting::RateLimitEnvironment;
use repo_client::RepoClient;
use repo_identity::RepoIdentityRef;
use scribe_ext::Scribe;
use slog::error;
use slog::o;
use slog::Drain;
use slog::Level;
use slog::Logger;
use slog_ext::SimpleFormatWithError;
use slog_kvfilter::KVFilter;
use sshrelay::SenderBytesWrite;
use sshrelay::Stdio;
use stats::prelude::*;
use time_ext::DurationExt;

use crate::errors::ErrorKind;
use crate::repo_handlers::RepoHandler;

define_stats! {
    prefix = "mononoke.request_handler";
    wireproto_ms:
        histogram(500, 0, 100_000, Average, Sum, Count; P 5; P 25; P 50; P 75; P 95; P 97; P 99),
    request_success: timeseries(Rate, Sum),
    request_failure: timeseries(Rate, Sum),
    request_outcome_permille: timeseries(Average),
}

pub async fn request_handler(
    fb: FacebookInit,
    reponame: String,
    repo_handlers: &HashMap<String, RepoHandler>,
    _security_checker: &ConnectionSecurityChecker,
    stdio: Stdio,
    rate_limiter: Option<RateLimitEnvironment>,
    addr: IpAddr,
    scribe: Scribe,
    qps: Option<Arc<Qps>>,
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
        repo,
        maybe_push_redirector_args,
        repo_client_knobs,
        maybe_backup_repo_source,
    } = handler;

    // Upgrade log to include server drain
    let conn_log = create_conn_logger(stderr.clone(), Some(logger), Some(session_id));

    scuba = scuba.with_seq("seq");
    scuba.add("repo", reponame);
    scuba.add_metadata(&metadata);
    scuba.sample_for_identities(metadata.identities());

    let reponame = repo.inner_repo().repo_identity().name();

    let rate_limiter = rate_limiter.map(|r| r.get_rate_limiter());
    if let Some(ref rate_limiter) = rate_limiter {
        if let Err(err) = rate_limiter.check_load_shed(metadata.identities()) {
            scuba.log_with_msg("Request rejected due to load shedding", format!("{}", err));
            error!(conn_log, "Request rejected due to load shedding: {}", err; "remote" => "true");

            return Err(err.into());
        }
    }

    let is_allowed_to_repo = repo.blob_repo().permission_checker()
        .check_if_read_access_allowed(metadata.identities())
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

    // Info per wireproto command within this session
    let wireproto_calls = Arc::new(Mutex::new(Vec::new()));

    scuba.log_with_msg("Connection established", None);

    let session_builder = SessionContainer::builder(fb)
        .metadata(metadata.clone())
        .rate_limiter(rate_limiter);

    let session = session_builder.build();

    let mut logging = LoggingContainer::new(fb, conn_log.clone(), scuba.clone());
    logging.with_scribe(scribe);

    let repo_client = RepoClient::new(
        repo,
        session.clone(),
        logging,
        maybe_push_redirector_args,
        repo_client_knobs,
        maybe_backup_repo_source,
    );
    let request_perf_counters = repo_client.request_perf_counters();

    // Construct a hg protocol handler
    let proto_handler = HgProtoHandler::new(
        conn_log.clone(),
        stdin.map(|b| bytes_old::Bytes::from(b.as_ref())),
        repo_client,
        sshproto::HgSshCommandDecode,
        sshproto::HgSshCommandEncode,
        wireproto_calls.clone(),
        qps.clone(),
        metadata.revproxy_region().clone(),
    );

    // send responses back
    let endres = proto_handler
        .inspect(move |bytes| session.bump_load(Metric::EgressBytes, bytes.len() as f64))
        .map_err(Error::from)
        .map(|b| Bytes::copy_from_slice(b.as_ref()))
        .forward(stdout)
        .map(|_| ());

    // If we got an error at this point, then catch it and print a message
    let (stats, result) = endres.compat().timed().await;

    let wireproto_calls = {
        let mut wireproto_calls = wireproto_calls.lock().expect("lock poisoned");
        std::mem::take(&mut *wireproto_calls)
    };

    STATS::wireproto_ms.add_value(stats.completion_time.as_millis_unchecked() as i64);

    let mut scuba = scuba.clone();

    scuba
        .add_future_stats(&stats)
        .add("wireproto_commands", wireproto_calls);

    // Populate stats no matter what to avoid dead detectors firing.
    STATS::request_success.add_value(0);
    STATS::request_failure.add_value(0);

    // Log request level perf counters
    request_perf_counters.insert_perf_counters(&mut scuba);

    match &result {
        Ok(_) => {
            STATS::request_success.add_value(1);
            STATS::request_outcome_permille.add_value(1000);
            scuba.log_with_msg("Request finished - Success", None)
        }
        Err(err) => {
            if err.is::<mpsc::SendError<Bytes>>() {
                STATS::request_outcome_permille.add_value(0);
                scuba.log_with_msg("Request finished - Client Disconnected", format!("{}", err));
            } else {
                STATS::request_failure.add_value(1);
                STATS::request_outcome_permille.add_value(0);
                scuba.log_with_msg("Request finished - Failure", format!("{:#?}", err));
            }
        }
    }

    if let Err(err) = result {
        error!(&conn_log, "Command failed";
            SlogKVError(err),
            "remote" => "true"
        );
    }

    Ok(())
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
