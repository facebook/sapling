/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use gotham::state::State;
use gotham_ext::middleware::request_context::RequestContext;
use gotham_ext::middleware::MetadataState;
use gotham_ext::middleware::PostResponseInfo;
use gotham_ext::middleware::ScubaHandler;
use permission_checker::MononokeIdentitySetExt;
use scuba_ext::MononokeScubaSampleBuilder;

use crate::model::GitMethodInfo;

#[derive(Copy, Clone, Debug)]
pub enum MononokeGitScubaKey {
    Repo,
    Method,
    MethodVariants,
    User,
    Error,
    ErrorCount,
}

impl AsRef<str> for MononokeGitScubaKey {
    fn as_ref(&self) -> &'static str {
        match self {
            Self::Repo => "repo",
            Self::Method => "method",
            Self::MethodVariants => "method_variants",
            Self::User => "user",
            Self::Error => "error",
            Self::ErrorCount => "error_count",
        }
    }
}

impl From<MononokeGitScubaKey> for String {
    fn from(key: MononokeGitScubaKey) -> Self {
        key.as_ref().to_string()
    }
}

#[derive(Clone)]
pub struct MononokeGitScubaHandler {
    request_context: Option<RequestContext>,
    method_info: Option<GitMethodInfo>,
    client_username: Option<String>,
}

impl ScubaHandler for MononokeGitScubaHandler {
    fn from_state(state: &State) -> Self {
        Self {
            request_context: state.try_borrow::<RequestContext>().cloned(),
            method_info: state.try_borrow::<GitMethodInfo>().cloned(),
            client_username: state
                .try_borrow::<MetadataState>()
                .and_then(|metadata_state| metadata_state.metadata().identities().username())
                .map(ToString::to_string),
        }
    }

    fn populate_scuba(self, info: &PostResponseInfo, scuba: &mut MononokeScubaSampleBuilder) {
        scuba.add_opt(MononokeGitScubaKey::User, self.client_username);

        if let Some(info) = self.method_info {
            scuba.add(MononokeGitScubaKey::Repo, info.repo.clone());
            scuba.add(MononokeGitScubaKey::Method, info.method.to_string());
            scuba.add(
                MononokeGitScubaKey::MethodVariants,
                info.variants_to_string(),
            );
        }

        if let Some(ctx) = self.request_context {
            ctx.ctx.perf_counters().insert_perf_counters(scuba);
        }

        if let Some(err) = info.first_error() {
            scuba.add(MononokeGitScubaKey::Error, format!("{:?}", err));
        }

        scuba.add(MononokeGitScubaKey::ErrorCount, info.error_count());
    }
}
