/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeSet;
use std::sync::Arc;

use async_trait::async_trait;
use bookmarks_movement::describe_hook_rejections;
use bookmarks_movement::BookmarkMovementError;
use bookmarks_movement::HookRejection;
use fbinit::FacebookInit;
use land_service_if::server::LandService;
use land_service_if::services::land_service::*;
use land_service_if::types::*;
use metadata::Metadata;
use mononoke_api::CoreContext;
use mononoke_api::Mononoke;
use mononoke_api::SessionContainer;
use mononoke_api_types::InnerRepo;
use permission_checker::MononokeIdentitySet;
use pushrebase::PushrebaseConflict;
use pushrebase::PushrebaseError;
use pushrebase_client::LocalPushrebaseClient;
use pushrebase_client::PushrebaseClient;
use repo_authorization::AuthorizationContext;
use scribe_ext::Scribe;
use scuba_ext::MononokeScubaSampleBuilder;
use scuba_ext::ScubaValue;
use slog::Logger;
use srserver::RequestContext;

use crate::errors;

#[derive(Clone)]
#[allow(dead_code)]
pub(crate) struct LandServiceImpl {
    pub(crate) fb: FacebookInit,
    pub(crate) logger: Logger,
    pub(crate) scuba_builder: MononokeScubaSampleBuilder,
    pub(crate) mononoke: Arc<Mononoke>,
    pub(crate) scribe: Scribe,
}

pub(crate) struct LandServiceThriftImpl(LandServiceImpl);

impl LandServiceImpl {
    pub fn new(
        fb: FacebookInit,
        logger: Logger,
        mononoke: Arc<Mononoke>,
        mut scuba_builder: MononokeScubaSampleBuilder,
        scribe: Scribe,
    ) -> Self {
        scuba_builder.add_common_server_data();

        Self {
            fb,
            logger,
            mononoke,
            scuba_builder,
            scribe,
        }
    }

    pub(crate) fn thrift_server(&self) -> LandServiceThriftImpl {
        LandServiceThriftImpl(self.clone())
    }

    pub(crate) async fn create_ctx(
        &self,
        name: &str,
        req_ctxt: &RequestContext,
    ) -> Result<CoreContext, InternalError> {
        let session = self.create_session(req_ctxt).await?;
        let identities = session.metadata().identities();
        let scuba = self.create_scuba(name, req_ctxt, identities)?;
        let ctx = session.new_context_with_scribe(self.logger.clone(), scuba, self.scribe.clone());
        Ok(ctx)
    }

    /// Create and configure a scuba sample builder for a request.
    fn create_scuba(
        &self,
        name: &str,
        req_ctxt: &RequestContext,
        identities: &MononokeIdentitySet,
    ) -> Result<MononokeScubaSampleBuilder, InternalError> {
        let mut scuba = self.scuba_builder.clone().with_seq("seq");
        scuba.add("type", "thrift");
        scuba.add("method", name);

        const CLIENT_HEADERS: &[&str] = &[
            "client_id",
            "client_type",
            "client_correlator",
            "proxy_client_id",
        ];
        for &header in CLIENT_HEADERS.iter() {
            let value = req_ctxt.header(header).map_err(errors::internal_error)?;
            if let Some(value) = value {
                scuba.add(header, value);
            }
        }

        scuba.add(
            "identities",
            identities
                .iter()
                .map(|id| id.to_string())
                .collect::<ScubaValue>(),
        );

        Ok(scuba)
    }

    async fn create_metadata(&self, _req_ctxt: &RequestContext) -> Result<Metadata, InternalError> {
        Ok(Metadata::new(
            None,
            BTreeSet::new(), //TODO: tls_identities.union(&cats_identities).cloned().collect(),
            false,
            None,
        )
        .await)
    }

    /// Create and configure the session container for a request.
    async fn create_session(
        &self,
        req_ctxt: &RequestContext,
    ) -> Result<SessionContainer, InternalError> {
        let metadata = self.create_metadata(req_ctxt).await?;
        let session = SessionContainer::builder(self.fb)
            .metadata(Arc::new(metadata))
            .build();
        Ok(session)
    }
}

fn convert_rejection(rejection: HookRejection) -> land_service_if::HookRejection {
    land_service_if::HookRejection {
        hook_name: rejection.hook_name,
        cs_id: Vec::from(rejection.cs_id.as_ref()),
        reason: land_service_if::HookOutcomeRejected {
            description: rejection.reason.description.to_string(),
            long_description: rejection.reason.long_description,
        },
    }
}

fn reason_rejections(rejections: &Vec<HookRejection>) -> String {
    format!(
        "Hooks failed:\n{}",
        describe_hook_rejections(rejections.as_slice())
    )
}

fn reason_conflicts(conflicts: &Vec<PushrebaseConflict>) -> String {
    format!("Conflicts while pushrebasing: {:?}", conflicts)
}

fn convert_bookmark_movement_error(err: BookmarkMovementError) -> LandChangesetsExn {
    match err {
        BookmarkMovementError::HookFailure(rejections) => {
            LandChangesetsExn::hook_rejections(HookRejectionsException {
                reason: reason_rejections(&rejections),
                rejections: rejections.into_iter().map(convert_rejection).collect(),
            })
        }
        BookmarkMovementError::PushrebaseError(PushrebaseError::Conflicts(conflicts)) => {
            LandChangesetsExn::pushrebase_conflicts(PushrebaseConflictsException {
                reason: reason_conflicts(&conflicts),
                conflicts: conflicts
                    .into_iter()
                    .map(|c| land_service_if::PushrebaseConflicts {
                        left: c.left.to_string(),
                        right: c.right.to_string(),
                    })
                    .collect(),
            })
        }
        _ => todo!(),
    }
}

#[async_trait]
impl LandService for LandServiceThriftImpl {
    type RequestContext = RequestContext;

    async fn land_changesets(
        &self,
        req_ctxt: &RequestContext,
        _land_changesets: LandChangesetRequest,
    ) -> Result<LandChangesetsResponse, LandChangesetsExn> {
        #![allow(unreachable_code)]

        let ctx = self.0.create_ctx("land_changesets", req_ctxt).await;
        let _authz = AuthorizationContext::new(&ctx?);

        let _repo: InnerRepo = todo!(); //TODO: Get the right implementation

        let _outcome = LocalPushrebaseClient {
            ctx: &ctx?,
            authz: &_authz,
            repo: &_repo,
            pushrebase_params: todo!(),
            lca_hint: todo!(),
            infinitepush_params: todo!(),
            hook_manager: todo!(),
        }
        .pushrebase(
            todo!(), //bookmark,
            todo!(), //changesets,
            todo!(), //pushvars,
            todo!(), //cross_repo_push_source,
            todo!(), //bookmark_restrictions,
        )
        .await
        .map_err(convert_bookmark_movement_error)?;

        Ok(LandChangesetsResponse {
            pushrebase_outcome: todo!(),
        })
    }
}
