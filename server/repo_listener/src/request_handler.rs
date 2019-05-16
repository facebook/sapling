// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::mem;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use failure::{prelude::*, SlogKVError};
use futures::{Future, Sink, Stream};
use futures_stats::Timed;
use slog::{self, Drain, Level, Logger};
use slog_ext::SimpleFormatWithError;
use slog_kvfilter::KVFilter;
use slog_term;
use stats::Histogram;
use time_ext::DurationExt;
use tracing::{TraceContext, Traced};
use uuid::Uuid;

use hgproto::{sshproto, HgProtoHandler};
use repo_client::RepoClient;
use scuba_ext::ScubaSampleBuilderExt;
use sshrelay::{SenderBytesWrite, SshEnvVars, Stdio};

use repo_handlers::RepoHandler;

use context::CoreContext;
use hooks::HookManager;

define_stats! {
    prefix = "mononoke.request_handler";
    wireproto_ms:
        histogram(500, 0, 100_000, AVG, SUM, COUNT; P 5; P 25; P 50; P 75; P 95; P 97; P 99),
}

pub fn request_handler(
    RepoHandler {
        logger,
        scuba,
        wireproto_scribe_category,
        repo,
        hash_validation_percentage,
        lca_hint,
        phases_hint,
        preserve_raw_bundle2,
    }: RepoHandler,
    stdio: Stdio,
    hook_manager: Arc<HookManager>,
) -> impl Future<Item = (), Error = ()> {
    let mut scuba_logger = scuba;
    let Stdio {
        stdin,
        stdout,
        stderr,
        mut preamble,
    } = stdio;

    let session_uuid = match preamble
        .misc
        .get("session_uuid")
        .and_then(|v| Uuid::parse_str(v).ok())
    {
        Some(session_uuid) => session_uuid,
        None => {
            let session_uuid = Uuid::new_v4();
            preamble
                .misc
                .insert("session_uuid".to_owned(), format!("{}", session_uuid));
            session_uuid
        }
    };

    // Info per wireproto command within this session
    let wireproto_calls = Arc::new(Mutex::new(Vec::new()));
    let trace = TraceContext::new(session_uuid, Instant::now());

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
        Logger::root(drain, o!("session_uuid" => format!("{}", session_uuid)))
    };

    scuba_logger.log_with_msg("Connection established", None);

    let ctx = CoreContext::new(
        session_uuid,
        conn_log,
        scuba_logger.clone(),
        wireproto_scribe_category,
        trace.clone(),
        preamble.misc.get("unix_username").cloned(),
        SshEnvVars::from_map(&preamble.misc),
    );

    // Construct a hg protocol handler
    let proto_handler = HgProtoHandler::new(
        ctx.clone(),
        stdin,
        RepoClient::new(
            repo.clone(),
            ctx.clone(),
            hash_validation_percentage,
            lca_hint,
            phases_hint,
            preserve_raw_bundle2,
            hook_manager,
        ),
        sshproto::HgSshCommandDecode,
        sshproto::HgSshCommandEncode,
        wireproto_calls.clone(),
    );

    // send responses back
    let endres = proto_handler
        .map_err(Error::from)
        .forward(stdout)
        .map(|_| ());

    // If we got an error at this point, then catch it and print a message
    endres
        .traced(&trace, "wireproto request", trace_args!())
        .timed(move |stats, result| {
            let mut wireproto_calls = wireproto_calls.lock().expect("lock poisoned");
            let wireproto_calls = mem::replace(&mut *wireproto_calls, Vec::new());

            STATS::wireproto_ms.add_value(stats.completion_time.as_millis_unchecked() as i64);
            scuba_logger
                .add_future_stats(&stats)
                .add("wireproto_commands", wireproto_calls);

            match result {
                Ok(_) => scuba_logger.log_with_msg("Request finished - Success", None),
                Err(err) => {
                    scuba_logger.log_with_msg("Request finished - Failure", format!("{:#?}", err));
                }
            }
            scuba_logger.log_with_trace(&trace)
        })
        .map_err(move |err| {
            error!(ctx.logger(), "Command failed";
                SlogKVError(err),
                "remote" => "true"
            );
        })
}
