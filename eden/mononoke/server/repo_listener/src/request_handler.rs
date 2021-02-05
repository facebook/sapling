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
use bytes::Bytes;
use context::{LoggingContainer, SessionClass, SessionContainer, SessionId};
use failure_ext::SlogKVError;
use fbinit::FacebookInit;
use futures::compat::Future01CompatExt;
use futures_old::{sync::mpsc, Future, Stream};
use futures_stats::TimedFutureExt;
use hgproto::{sshproto, HgProtoHandler};
use load_limiter::{LoadLimiterEnvironment, Metric};
use maplit::{hashmap, hashset};
use repo_client::RepoClient;
use scribe_ext::Scribe;
use slog::{self, error, o, Drain, Level, Logger};
use slog_ext::SimpleFormatWithError;
use slog_kvfilter::KVFilter;
use sshrelay::{Priority, SenderBytesWrite, Stdio};
use stats::prelude::*;
use std::mem;
use std::net::IpAddr;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use time_ext::DurationExt;
use tracing::{trace_args, TraceContext, TraceId, Traced};
use tunables::tunables;

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
    security_checker: &ConnectionsSecurityChecker,
    stdio: Stdio,
    load_limiter: Option<LoadLimiterEnvironment>,
    addr: IpAddr,
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

    let mut session_builder = SessionContainer::builder(fb)
        .trace(trace.clone())
        .metadata(metadata.clone())
        .load_limiter(
            load_limiter.map(|l| l.get(metadata.identities(), metadata.client_hostname())),
        );

    if priority == &Priority::Wishlist {
        session_builder = session_builder
            .session_class(SessionClass::Background)
            .blobstore_maybe_read_qps_limiter(tunables().get_wishlist_read_qps())
            .await
            .blobstore_maybe_write_qps_limiter(tunables().get_wishlist_write_qps())
            .await;
    }

    let session = session_builder.build();

    let mut logging = LoggingContainer::new(fb, conn_log.clone(), scuba.clone());
    logging.with_scribe(scribe);

    let repo_client = RepoClient::new(
        repo,
        session.clone(),
        logging,
        preserve_raw_bundle2,
        wireproto_logging,
        maybe_push_redirector_args,
        repo_client_knobs,
    );
    let request_perf_counters = repo_client.request_perf_counters();

    // Construct a hg protocol handler
    let proto_handler = HgProtoHandler::new(
        conn_log.clone(),
        stdin.map(bytes_ext::copy_from_new),
        repo_client,
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

    // Log request level perf counters
    request_perf_counters.insert_perf_counters(&mut scuba);

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
