/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use gotham::state::State;

use gotham_ext::middleware::ScubaHandler;
use scuba::ScubaSampleBuilder;

use crate::middleware::RequestContext;
use crate::util::read_header_value;

#[derive(Copy, Clone, Debug)]
pub enum LfsScubaKey {
    /// The repository this request was for.
    Repository,
    /// The method this request matched for in our handlers.
    Method,
    /// If an error was encountered during processing, the error message.
    ErrorMessage,
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
    /// The attempt reported by the client
    ClientAttempt,
}

impl AsRef<str> for LfsScubaKey {
    fn as_ref(&self) -> &'static str {
        use LfsScubaKey::*;

        match self {
            Repository => "repository",
            Method => "method",
            ErrorMessage => "error_msg",
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
}

impl ScubaHandler for LfsScubaHandler {
    fn from_state(state: &State) -> Self {
        let client_attempt = read_header_value(state, "X-Attempt")
            .map(|r| r.ok())
            .flatten();

        Self {
            ctx: state.try_borrow::<RequestContext>().cloned(),
            client_attempt,
        }
    }

    fn add_stats(self, scuba: &mut ScubaSampleBuilder) {
        scuba.add_opt(LfsScubaKey::ClientAttempt, self.client_attempt);

        if let Some(ctx) = self.ctx {
            if let Some(repository) = ctx.repository {
                scuba.add(LfsScubaKey::Repository, repository.as_ref());
            }

            if let Some(method) = ctx.method {
                scuba.add(LfsScubaKey::Method, method.to_string());
            }

            if let Some(err_msg) = ctx.error_msg {
                scuba.add(LfsScubaKey::ErrorMessage, err_msg.as_ref());
            }

            ctx.ctx.perf_counters().insert_perf_counters(scuba);
        }
    }
}
