/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::default::Default;
use std::fmt;
use std::sync::Arc;

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
use hyper::StatusCode;
use metadata::Metadata;
use permission_checker::MononokeIdentitySet;
use permission_checker::MononokeIdentitySetExt;
use scuba_ext::MononokeScubaSampleBuilder;
use slog::error;
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

impl LfsMethod {
    pub fn is_read_only(&self) -> bool {
        match self {
            Self::Download | Self::DownloadSha256 | Self::Batch => true,
            Self::Upload | Self::GitBlob => false,
        }
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
    enforce_authentication: bool,
    readonly: bool,
}

impl RequestContextMiddleware {
    pub fn new(
        fb: FacebookInit,
        logger: Logger,
        enforce_authentication: bool,
        readonly: bool,
    ) -> Self {
        Self {
            fb,
            logger,
            enforce_authentication,
            readonly,
        }
    }
}

#[async_trait::async_trait]
impl Middleware for RequestContextMiddleware {
    async fn inbound(&self, state: &mut State) -> Option<Response<Body>> {
        let request_id = state.short_request_id();

        let logger = self.logger.new(o!("request_id" => request_id.to_string()));
        let (identities, address) =
            if let Some(client_identity) = ClientIdentity::try_borrow_from(state) {
                (
                    client_identity.identities().clone(),
                    client_identity.address().clone(),
                )
            } else {
                (None, None)
            };

        let should_log = !identities
            .as_ref()
            .map_or(false, |id| id.is_proxygen_test_identity());

        let identities: MononokeIdentitySet = match identities {
            Some(identities) => identities,
            None => {
                if self.enforce_authentication {
                    let msg = "Client not authenticated".to_string();
                    error!(self.logger, "{}", &msg);
                    let response = Response::builder()
                        .status(StatusCode::FORBIDDEN)
                        .body(
                            format!(
                                "{{\"message:\"{}\", \"request_id\":\"{}\"}}",
                                msg,
                                state.short_request_id()
                            )
                            .into(),
                        )
                        .expect("Couldn't build http response");

                    return Some(response);
                } else {
                    Default::default()
                }
            }
        };
        let metadata = Metadata::new(None, identities, false, address).await;
        let session = SessionContainer::builder(self.fb)
            .metadata(Arc::new(metadata))
            .readonly(self.readonly)
            .build();
        let ctx = session.new_context(logger, MononokeScubaSampleBuilder::with_discard());

        state.put(RequestContext::new(ctx, should_log));

        None
    }
}
