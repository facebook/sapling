/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fmt;

use context::CoreContext;
use context::SessionContainer;
use fbinit::FacebookInit;
use gotham::state::FromState;
use gotham::state::State;
use gotham_derive::StateData;
use gotham_ext::middleware::ClientIdentity;
use gotham_ext::middleware::Middleware;
use gotham_ext::state_ext::StateExt;
use hyper::body::Body;
use hyper::Response;
use scuba_ext::MononokeScubaSampleBuilder;
use slog::o;
use slog::Logger;

#[derive(Copy, Clone)]
pub enum LfsMethod {
    Upload,
    Download,
    DownloadSha256,
    Batch,
    // Methods below this are for pushing git objects, not for LFS
    // They do not correspond to any LFS protocol
    GitBlob,
}

impl fmt::Display for LfsMethod {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::Upload => "upload",
            Self::Download => "download",
            Self::DownloadSha256 => "download_sha256",
            Self::Batch => "batch",
            Self::GitBlob => "git_blob_upload",
        };
        write!(f, "{}", name)
    }
}

#[derive(StateData, Clone)]
pub struct RequestContext {
    pub ctx: CoreContext,
    pub repository: Option<String>,
    pub method: Option<LfsMethod>,
    pub error_msg: Option<String>,
    pub should_log: bool,
}

impl RequestContext {
    fn new(ctx: CoreContext, should_log: bool) -> Self {
        Self {
            ctx,
            repository: None,
            method: None,
            error_msg: None,
            should_log,
        }
    }

    pub fn set_request(&mut self, repository: String, method: LfsMethod) {
        self.repository = Some(repository);
        self.method = Some(method);
    }
}

#[derive(Clone)]
pub struct RequestContextMiddleware {
    fb: FacebookInit,
    logger: Logger,
}

impl RequestContextMiddleware {
    pub fn new(fb: FacebookInit, logger: Logger) -> Self {
        Self { fb, logger }
    }
}

#[async_trait::async_trait]
impl Middleware for RequestContextMiddleware {
    async fn inbound(&self, state: &mut State) -> Option<Response<Body>> {
        let request_id = state.short_request_id();

        let logger = self.logger.new(o!("request_id" => request_id.to_string()));
        let session = SessionContainer::new_with_defaults(self.fb);
        let ctx = session.new_context(logger, MononokeScubaSampleBuilder::with_discard());

        let should_log = ClientIdentity::try_borrow_from(state).map_or(true, |client_identity| {
            !client_identity.is_proxygen_test_identity()
        });

        state.put(RequestContext::new(ctx, should_log));

        None
    }
}
