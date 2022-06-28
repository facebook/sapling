/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use gotham::state::State;

use gotham_ext::middleware::ClientIdentity;
use gotham_ext::middleware::PostResponseInfo;
use gotham_ext::middleware::ScubaHandler;
use scuba_ext::MononokeScubaSampleBuilder;

use crate::handlers::HandlerInfo;
use crate::middleware::RequestContext;

#[derive(Copy, Clone, Debug)]
pub enum EdenApiScubaKey {
    Repo,
    Method,
    User,
    HandlerError,
    HandlerErrorCount,
}

impl AsRef<str> for EdenApiScubaKey {
    fn as_ref(&self) -> &'static str {
        match self {
            Self::Repo => "repo",
            Self::Method => "edenapi_method",
            Self::User => "edenapi_user",
            Self::HandlerError => "edenapi_error",
            Self::HandlerErrorCount => "edenapi_error_count",
        }
    }
}

impl Into<String> for EdenApiScubaKey {
    fn into(self) -> String {
        self.as_ref().to_string()
    }
}

#[derive(Clone)]
pub struct EdenApiScubaHandler {
    request_context: Option<RequestContext>,
    handler_info: Option<HandlerInfo>,
    client_username: Option<String>,
}

impl ScubaHandler for EdenApiScubaHandler {
    fn from_state(state: &State) -> Self {
        Self {
            request_context: state.try_borrow::<RequestContext>().cloned(),
            handler_info: state.try_borrow::<HandlerInfo>().cloned(),
            client_username: state
                .try_borrow::<ClientIdentity>()
                .and_then(|id| id.username())
                .map(ToString::to_string),
        }
    }

    fn populate_scuba(self, info: &PostResponseInfo, scuba: &mut MononokeScubaSampleBuilder) {
        scuba.add_opt(EdenApiScubaKey::User, self.client_username);

        if let Some(info) = self.handler_info {
            scuba.add_opt(EdenApiScubaKey::Repo, info.repo.clone());
            scuba.add_opt(EdenApiScubaKey::Method, info.method.map(|m| m.to_string()));
        }

        if let Some(ctx) = self.request_context {
            ctx.ctx.perf_counters().insert_perf_counters(scuba);
        }

        if let Some(err) = info.first_error() {
            scuba.add(EdenApiScubaKey::HandlerError, format!("{:?}", err));
        }

        scuba.add(EdenApiScubaKey::HandlerErrorCount, info.error_count());
    }
}
