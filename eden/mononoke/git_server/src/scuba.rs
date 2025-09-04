/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use context::CoreContext;
use edenapi_service::handlers::HandlerInfo as SlapiHandlerInfo;
use gotham::state::State;
use gotham_ext::middleware::MetadataState;
use gotham_ext::middleware::PostResponseInfo;
use gotham_ext::middleware::ScubaHandler;
use gotham_ext::middleware::request_context::RequestContext;
use permission_checker::MononokeIdentitySet;
use permission_checker::MononokeIdentitySetExt;
use scuba_ext::MononokeScubaSampleBuilder;

use crate::model::BundleUriOutcome;
use crate::model::GitMethodInfo;
use crate::model::PushData;
use crate::model::PushValidationErrors;

#[derive(Copy, Clone, Debug)]
pub enum MononokeGitScubaKey {
    Repo,
    Method,
    MethodVariants,
    User,
    Error,
    ErrorCount,
    PushValidationErrors,
    BundleUriError,
    BundleUriSuccess,
    PackfileReadError,
    PackfileSize,
    PackfileCommitCount,
    PackfileTreeAndBlobCount,
    PackfileTagCount,
    RequestSignature,
    NHaves,
    NWants,
    ClientMainId,
    ClientIdentities,
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
            Self::PushValidationErrors => "push_validation_errors",
            Self::BundleUriError => "bundle_uri_error_msg",
            Self::BundleUriSuccess => "bundle_uri_success_msg",
            Self::PackfileReadError => "packfile_read_error",
            Self::PackfileSize => "packfile_size",
            Self::PackfileCommitCount => "packfile_commit_count",
            Self::PackfileTreeAndBlobCount => "packfile_tree_and_blob_count",
            Self::PackfileTagCount => "packfile_tag_count",
            Self::RequestSignature => "request_signature",
            Self::NWants => "n_wants",
            Self::NHaves => "n_haves",
            Self::ClientMainId => "client_main_id",
            Self::ClientIdentities => "client_identities",
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
    push_validation_errors: Option<PushValidationErrors>,
    bundle_uri_outcome: Option<BundleUriOutcome>,
    client_username: Option<String>,
    slapi_handler_info: Option<SlapiHandlerInfo>,
    push_data: Option<PushData>,
}

pub(crate) fn scuba_from_state(ctx: &CoreContext, state: &State) -> MononokeScubaSampleBuilder {
    let scuba = ctx.scuba().clone();
    let user = state
        .try_borrow::<MetadataState>()
        .and_then(|metadata_state| metadata_state.metadata().identities().username())
        .map(ToString::to_string);
    scuba_with_basic_info(user, state.try_borrow::<GitMethodInfo>().cloned(), scuba)
}

fn scuba_with_basic_info(
    user: Option<String>,
    info: Option<GitMethodInfo>,
    mut scuba: MononokeScubaSampleBuilder,
) -> MononokeScubaSampleBuilder {
    scuba.add_opt(MononokeGitScubaKey::User, user);
    if let Some(info) = info {
        scuba.add(MononokeGitScubaKey::Repo, info.repo.clone());
        scuba.add(MononokeGitScubaKey::Method, info.method.to_string());
        scuba.add(
            MononokeGitScubaKey::MethodVariants,
            info.variants_to_string(),
        );
        scuba.add(
            MononokeGitScubaKey::MethodVariants,
            info.variants_to_string_vector(),
        );
    }
    scuba
}

impl MononokeGitScubaHandler {
    pub fn from_state(state: &State) -> Self {
        Self {
            request_context: state.try_borrow::<RequestContext>().cloned(),
            method_info: state.try_borrow::<GitMethodInfo>().cloned(),
            bundle_uri_outcome: state.try_borrow::<BundleUriOutcome>().cloned(),
            push_validation_errors: state.try_borrow::<PushValidationErrors>().cloned(),
            client_username: state
                .try_borrow::<MetadataState>()
                .and_then(|metadata_state| metadata_state.metadata().identities().username())
                .map(ToString::to_string),
            slapi_handler_info: state.try_borrow::<SlapiHandlerInfo>().cloned(),
            push_data: state.try_borrow::<PushData>().cloned(),
        }
    }

    pub(crate) fn to_scuba(&self, ctx: &CoreContext) -> MononokeScubaSampleBuilder {
        let scuba = ctx.scuba().clone();
        scuba_with_basic_info(
            self.client_username.clone(),
            self.method_info.clone(),
            scuba,
        )
    }

    fn log_processed(self, info: &PostResponseInfo, mut scuba: MononokeScubaSampleBuilder) {
        scuba = scuba_with_basic_info(self.client_username, self.method_info, scuba);
        if let Some(ctx) = self.request_context {
            ctx.ctx.perf_counters().insert_perf_counters(&mut scuba);
        }

        if let Some(push_validation_errors) = self.push_validation_errors {
            scuba.add(
                MononokeGitScubaKey::PushValidationErrors,
                push_validation_errors.to_string(),
            );
        }

        if let Some(slapi_handler_info) = self.slapi_handler_info {
            scuba.add_opt(MononokeGitScubaKey::Repo, slapi_handler_info.repo.clone());
            scuba.add_opt(
                MononokeGitScubaKey::Method,
                slapi_handler_info.method.map(|m| m.to_string()),
            );
        }

        if let Some(outcome) = self.bundle_uri_outcome {
            match outcome {
                BundleUriOutcome::Success(success_msg) => {
                    scuba.add(MononokeGitScubaKey::BundleUriSuccess, success_msg);
                }
                BundleUriOutcome::Error(error_msg) => {
                    scuba.add(MononokeGitScubaKey::BundleUriError, error_msg);
                }
            }
        }
        if let Some(push_data) = self.push_data {
            scuba.add(MononokeGitScubaKey::PackfileSize, push_data.packfile_size);
        }
        if let Some(err) = info.first_error() {
            scuba.add(MononokeGitScubaKey::Error, format!("{:?}", err));
        }
        scuba.add(MononokeGitScubaKey::ErrorCount, info.error_count());
        scuba.add("log_tag", "MononokeGit Request Processed");
        scuba.unsampled();
        scuba.log();
    }

    fn log_cancelled(mut scuba: MononokeScubaSampleBuilder) {
        scuba.add("log_tag", "MononokeGit Request Cancelled");
        scuba.unsampled();
        scuba.log();
    }

    pub(crate) fn log_rejected(
        mut scuba: MononokeScubaSampleBuilder,
        repo_name: &str,
        main_client_id: Option<String>,
        identities: &MononokeIdentitySet,
        error: String,
    ) {
        scuba.add(MononokeGitScubaKey::Repo, repo_name.to_string());
        scuba.add(MononokeGitScubaKey::Error, error);
        scuba.add_opt(MononokeGitScubaKey::ClientMainId, main_client_id);
        scuba.add(
            MononokeGitScubaKey::ClientIdentities,
            identities.to_string(),
        );
        scuba.add("log_tag", "MononokeGit Request Rejected");
        scuba.unsampled();
        scuba.log();
    }
}

impl ScubaHandler for MononokeGitScubaHandler {
    fn from_state(state: &State) -> Self {
        Self::from_state(state)
    }

    fn log_processed(self, info: &PostResponseInfo, scuba: MononokeScubaSampleBuilder) {
        Self::log_processed(self, info, scuba)
    }

    fn log_cancelled(scuba: MononokeScubaSampleBuilder) {
        Self::log_cancelled(scuba)
    }
}
