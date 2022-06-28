/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::str::FromStr;

use gotham::state::State;

use clientinfo::ClientInfo;
use clientinfo::CLIENT_INFO_HEADER;
use gotham_ext::middleware::PostResponseInfo;
use gotham_ext::middleware::ScubaHandler;
use scuba_ext::MononokeScubaSampleBuilder;

use crate::middleware::RequestContext;
use crate::util::read_header_value_ignore_err;

struct ClientInfoHeader(ClientInfo);

impl FromStr for ClientInfoHeader {
    type Err = serde_json::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let client_info = serde_json::from_str(s)?;
        Ok(Self(client_info))
    }
}

#[derive(Copy, Clone, Debug)]
pub enum LfsScubaKey {
    /// The repository this request was for.
    Repository,
    /// The method this request matched for in our handlers.
    Method,
    /// If an error was encountered during processing, the error message.
    ErrorMessage,
    /// Total count fo errors that occurred during processing.
    ErrorCount,
    /// The order in which the response to a batch request was produced.
    BatchOrder,
    /// The number of objects in a batch request
    BatchObjectCount,
    /// The objects that could not be serviced by this LFS server in a batch request
    BatchInternalMissingBlobs,
    /// Timing checkpoints in batch requests
    BatchRequestContextReadyUs,
    BatchRequestReceivedUs,
    BatchRequestParsedUs,
    BatchResponseReadyUs,
    /// Whether the upload was a sync
    UploadSync,
    /// The actual size of the content being sent
    DownloadContentSize,
    /// The attempt information reported by the client
    ClientAttempt,
    ClientAttemptsLeft,
    ClientThrottleAttemptsLeft,
    /// Fields of ClientInfo
    SandcastleNonce,
    SandcastleAlias,
    ClientTwJob,
    ClientTwTask,
}

impl AsRef<str> for LfsScubaKey {
    fn as_ref(&self) -> &'static str {
        use LfsScubaKey::*;

        match self {
            Repository => "repository",
            Method => "method",
            ErrorMessage => "error_msg",
            ErrorCount => "error_count",
            BatchOrder => "batch_order",
            BatchObjectCount => "batch_object_count",
            BatchInternalMissingBlobs => "batch_internal_missing_blobs",
            BatchRequestContextReadyUs => "batch_context_ready_us",
            BatchRequestReceivedUs => "batch_request_received_us",
            BatchRequestParsedUs => "batch_request_parsed_us",
            BatchResponseReadyUs => "batch_response_ready_us",
            UploadSync => "upload_sync",
            DownloadContentSize => "download_content_size",
            ClientAttempt => "client_attempt",
            ClientAttemptsLeft => "client_attempts_left",
            ClientThrottleAttemptsLeft => "client_throttle_attempts_left",
            SandcastleNonce => "sandcastle_nonce",
            SandcastleAlias => "sandcastle_alias",
            ClientTwJob => "client_tw_job",
            ClientTwTask => "client_tw_task",
        }
    }
}

impl Into<String> for LfsScubaKey {
    fn into(self) -> String {
        self.as_ref().to_string()
    }
}

#[derive(Clone)]
pub struct LfsScubaHandler {
    ctx: Option<RequestContext>,
    client_attempt: Option<u64>,
    client_attempts_left: Option<u64>,
    client_throttle_attempts_left: Option<u64>,
    client_info: Option<ClientInfo>,
}

impl ScubaHandler for LfsScubaHandler {
    fn from_state(state: &State) -> Self {
        let client_attempt = read_header_value_ignore_err(state, "X-Attempt");

        let client_attempts_left = read_header_value_ignore_err(state, "X-Attempts-Left");

        let client_throttle_attempts_left =
            read_header_value_ignore_err(state, "X-Throttle-Attempts-Left");

        let client_info: Option<ClientInfo> =
            read_header_value_ignore_err(state, CLIENT_INFO_HEADER)
                .map(|ci: ClientInfoHeader| ci.0);

        Self {
            ctx: state.try_borrow::<RequestContext>().cloned(),
            client_attempt,
            client_attempts_left,
            client_throttle_attempts_left,
            client_info,
        }
    }

    fn populate_scuba(self, info: &PostResponseInfo, scuba: &mut MononokeScubaSampleBuilder) {
        scuba.add_opt(LfsScubaKey::ClientAttempt, self.client_attempt);
        scuba.add_opt(LfsScubaKey::ClientAttemptsLeft, self.client_attempts_left);
        scuba.add_opt(
            LfsScubaKey::ClientThrottleAttemptsLeft,
            self.client_throttle_attempts_left,
        );

        if let Some(ctx) = self.ctx {
            if let Some(repository) = ctx.repository {
                scuba.add(LfsScubaKey::Repository, repository.as_ref());
            }

            if let Some(method) = ctx.method {
                scuba.add(LfsScubaKey::Method, method.to_string());
            }

            if let Some(err) = info.first_error() {
                scuba.add(LfsScubaKey::ErrorMessage, format!("{:?}", err));
            }

            scuba.add(LfsScubaKey::ErrorCount, info.error_count());

            ctx.ctx.perf_counters().insert_perf_counters(scuba);
        }

        if let Some(client_info) = self.client_info {
            scuba.add_opt(
                LfsScubaKey::SandcastleNonce,
                client_info.fb.sandcastle_nonce(),
            );

            scuba.add_opt(
                LfsScubaKey::SandcastleAlias,
                client_info.fb.sandcastle_alias(),
            );

            scuba.add_opt(LfsScubaKey::ClientTwJob, client_info.fb.tw_job());

            scuba.add_opt(LfsScubaKey::ClientTwTask, client_info.fb.tw_task());
        }
    }
}
