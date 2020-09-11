/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use gotham::state::State;

use gotham_ext::middleware::ScubaHandler;
use scuba::ScubaSampleBuilder;

use crate::handlers::HandlerInfo;
use crate::middleware::RequestContext;

#[derive(Copy, Clone, Debug)]
pub enum EdenApiScubaKey {
    Repo,
    Method,
}

impl AsRef<str> for EdenApiScubaKey {
    fn as_ref(&self) -> &'static str {
        match self {
            Self::Repo => "repo",
            Self::Method => "method",
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
}

impl ScubaHandler for EdenApiScubaHandler {
    fn from_state(state: &State) -> Self {
        Self {
            request_context: state.try_borrow::<RequestContext>().cloned(),
            handler_info: state.try_borrow::<HandlerInfo>().cloned(),
        }
    }

    fn add_stats(self, scuba: &mut ScubaSampleBuilder) {
        if let Some(info) = self.handler_info {
            scuba.add_opt(EdenApiScubaKey::Repo, info.repo.clone());
            scuba.add_opt(EdenApiScubaKey::Method, info.method.map(|m| m.to_string()));
        }

        if let Some(ctx) = self.request_context {
            ctx.ctx.perf_counters().insert_perf_counters(scuba);
        }
    }
}
