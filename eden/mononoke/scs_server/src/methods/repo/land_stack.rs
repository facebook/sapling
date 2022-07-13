/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use bookmarks_movement::describe_hook_rejections;
use bookmarks_movement::BookmarkKindRestrictions;
use bookmarks_movement::HookRejection;
use borrowed::borrowed;
use context::CoreContext;
use hooks::CrossRepoPushSource;
use mononoke_api::ChangesetSpecifier;
use mononoke_api::MononokeError;
use permission_checker::MononokeIdentity;
use pushrebase::PushrebaseConflict;
use source_control as thrift;

use crate::commit_id::CommitIdExt;
use crate::errors;
use crate::errors::LoggableError;
use crate::errors::ServiceErrorResultExt;
use crate::errors::Status;
use crate::from_request::convert_pushvars;
use crate::from_request::FromRequest;
use crate::into_response::AsyncIntoResponseWith;
use crate::source_control_impl::SourceControlServiceImpl;
use service::RepoLandStackExn;
use source_control::services::source_control_service as service;

enum LandStackError {
    Service(errors::ServiceError),
    PushrebaseConflicts(Vec<PushrebaseConflict>),
    HookRejections(Vec<HookRejection>),
}

impl From<errors::ServiceError> for LandStackError {
    fn from(e: errors::ServiceError) -> Self {
        Self::Service(e)
    }
}

impl From<MononokeError> for LandStackError {
    fn from(e: MononokeError) -> Self {
        match e {
            MononokeError::HookFailure(rejections) => Self::HookRejections(rejections),
            MononokeError::PushrebaseConflicts(conflicts) => Self::PushrebaseConflicts(conflicts),
            e => Self::Service(e.into()),
        }
    }
}

impl From<thrift::RequestError> for LandStackError {
    fn from(e: thrift::RequestError) -> Self {
        Self::Service(e.into())
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

fn convert_rejection(rejection: HookRejection) -> thrift::HookRejection {
    thrift::HookRejection {
        hook_name: rejection.hook_name,
        cs_id: Vec::from(rejection.cs_id.as_ref()),
        reason: thrift::HookOutcomeRejected {
            description: rejection.reason.description.to_string(),
            long_description: rejection.reason.long_description,
            ..Default::default()
        },
        ..Default::default()
    }
}

impl From<LandStackError> for RepoLandStackExn {
    fn from(e: LandStackError) -> RepoLandStackExn {
        match e {
            LandStackError::Service(e) => e.into(),
            LandStackError::HookRejections(rejections) => {
                RepoLandStackExn::hook_rejections(thrift::HookRejectionsException {
                    reason: reason_rejections(&rejections),
                    rejections: rejections.into_iter().map(convert_rejection).collect(),
                    ..Default::default()
                })
            }
            LandStackError::PushrebaseConflicts(conflicts) => {
                RepoLandStackExn::pushrebase_conflicts(thrift::PushrebaseConflictsException {
                    reason: reason_conflicts(&conflicts),
                    conflicts: conflicts
                        .into_iter()
                        .map(|c| thrift::PushrebaseConflict {
                            left: c.left.to_string(),
                            right: c.right.to_string(),
                            ..Default::default()
                        })
                        .collect(),
                    ..Default::default()
                })
            }
        }
    }
}

impl LoggableError for LandStackError {
    fn status_and_description(&self) -> (Status, String) {
        match self {
            Self::Service(svc) => svc.status_and_description(),
            Self::HookRejections(rejections) => {
                (Status::RequestError, reason_rejections(rejections))
            }
            Self::PushrebaseConflicts(conflicts) => {
                (Status::RequestError, reason_conflicts(conflicts))
            }
        }
    }
}

impl SourceControlServiceImpl {
    async fn impl_repo_land_stack(
        &self,
        ctx: CoreContext,
        repo: thrift::RepoSpecifier,
        params: thrift::RepoLandStackParams,
    ) -> Result<thrift::RepoLandStackResponse, LandStackError> {
        let repo = self
            .repo_for_service(ctx, &repo, params.service_identity)
            .await?;
        borrowed!(params.head, params.base);
        let head = repo
            .changeset(ChangesetSpecifier::from_request(head)?)
            .await
            .context("failed to resolve head commit")?
            .ok_or_else(|| errors::commit_not_found(head.to_string()))?;
        let base = repo
            .changeset(ChangesetSpecifier::from_request(base)?)
            .await
            .context("failed to resolve base commit")?
            .ok_or_else(|| errors::commit_not_found(base.to_string()))?;
        let pushvars = convert_pushvars(params.pushvars);
        let push_source = CrossRepoPushSource::from_request(&params.__internal_only_push_source)?;
        if push_source != CrossRepoPushSource::NativeToThisRepo {
            // TODO: Once we move to a land service, this internal_only argument can be removed
            let original_identities = repo.ctx().metadata().original_identities();
            if !original_identities.map_or(false, |ids| {
                ids.contains(&MononokeIdentity::from_identity(&self.identity))
            }) {
                return Err(errors::invalid_request(format!(
                    "Insufficient permissions to use internal only option. Identities: {}",
                    original_identities
                        .map_or_else(|| "<none>".to_string(), permission_checker::pretty_print)
                ))
                .into());
            }
        }
        let bookmark_restrictions =
            BookmarkKindRestrictions::from_request(&params.bookmark_restrictions)?;

        let pushrebase_outcome = repo
            .land_stack(
                &params.bookmark,
                head.id(),
                base.id(),
                pushvars.as_ref(),
                push_source,
                bookmark_restrictions,
            )
            .await?
            .into_response_with(&(
                repo.clone(),
                params.identity_schemes,
                params.old_identity_schemes,
            ))
            .await?;

        Ok(thrift::RepoLandStackResponse {
            pushrebase_outcome,
            ..Default::default()
        })
    }

    pub(crate) async fn repo_land_stack(
        &self,
        ctx: CoreContext,
        repo: thrift::RepoSpecifier,
        params: thrift::RepoLandStackParams,
    ) -> Result<thrift::RepoLandStackResponse, impl Into<service::RepoLandStackExn> + LoggableError>
    {
        self.impl_repo_land_stack(ctx, repo, params).await
    }
}
