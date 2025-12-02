/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use gotham::state::FromState;
use gotham::state::State;
use gotham_ext::middleware::MetadataState;
use gotham_ext::middleware::Middleware;
use http::Response;
use http::Uri;
use hyper::Body;
use hyper::StatusCode;
use metadata::Metadata;
use rate_limiting::LoadShedResult;
use rate_limiting::RateLimitEnvironment;
use scuba_ext::MononokeScubaSampleBuilder;
use tracing::error;

use crate::scuba::MononokeGitScubaHandler;

const GIT_UPLOAD_PACK: &str = "/git-upload-pack";
const SERVER_PATH_PREFIX: &str = "/repos/git/";

#[derive(Clone)]
pub struct UploadPackRateLimitingMiddleware {
    scuba: MononokeScubaSampleBuilder,
    rate_limiter: Option<RateLimitEnvironment>,
}

impl UploadPackRateLimitingMiddleware {
    pub fn new(
        scuba: MononokeScubaSampleBuilder,
        rate_limiter: Option<RateLimitEnvironment>,
    ) -> Self {
        Self {
            scuba,
            rate_limiter,
        }
    }
}

#[async_trait::async_trait]
impl Middleware for UploadPackRateLimitingMiddleware {
    async fn inbound(&self, state: &mut State) -> Option<Response<Body>> {
        let rate_limiter = if let Some(rate_limiter) = &self.rate_limiter {
            rate_limiter.get_rate_limiter()
        } else {
            return None;
        };
        if let Some(uri) = Uri::try_borrow_from(state)
            && uri.path().contains(GIT_UPLOAD_PACK)
        {
            let repo_name = Uri::try_borrow_from(state)
                .and_then(|uri| uri.path().strip_suffix(GIT_UPLOAD_PACK))
                .and_then(|path| path.strip_prefix(SERVER_PATH_PREFIX))
                .and_then(|path| path.split_once('/').map(|(_, repo_name)| repo_name))
                .unwrap_or("");
            let metadata = if let Some(metadata_state) = MetadataState::try_borrow_from(state) {
                metadata_state.metadata().clone()
            } else {
                Metadata::default()
            };
            let main_client_id = metadata
                .client_info()
                .and_then(|client_info| client_info.request_info.clone())
                .and_then(|request_info| request_info.main_id);
            let atlas = metadata.clientinfo_atlas();
            let mut scuba = self.scuba.clone();
            if let LoadShedResult::Fail(err) = rate_limiter.check_load_shed(
                metadata.identities(),
                main_client_id.as_deref(),
                &mut scuba,
                atlas,
            ) {
                MononokeGitScubaHandler::log_rejected(
                    scuba,
                    repo_name,
                    main_client_id,
                    metadata.identities(),
                    format!(
                        "Upload pack request rejected due to load shedding / rate limiting: {:?}",
                        err
                    ),
                );
                error!(
                    "Upload pack request rejected due to load shedding / rate limiting: {:?}",
                    err
                );
                return Some(
                    Response::builder()
                        .status(StatusCode::TOO_MANY_REQUESTS)
                        .body(
                            format!(
                                "Upload pack request rejected due to load shedding / rate limiting: {:?}",
                                err
                            )
                            .into(),
                        )
                        .expect("Failed to build a response"),
                );
            }
        }
        None
    }
}
