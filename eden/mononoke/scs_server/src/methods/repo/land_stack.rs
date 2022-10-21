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
use cross_repo_sync::types::Large;
use hooks::CrossRepoPushSource;
use hooks::PushAuthoredBy;
use mononoke_api::ChangesetSpecifier;
use mononoke_api::MononokeError;
use mononoke_api::Repo;
use mononoke_api::RepoContext;
use permission_checker::MononokeIdentity;
use pushrebase::PushrebaseConflict;
use service::RepoLandStackExn;
use source_control as thrift;
use source_control::services::source_control_service as service;
use tunables::tunables;
use unbundle::PushRedirector;
use unbundle::PushRedirectorArgs;

use crate::commit_id::CommitIdExt;
use crate::errors;
use crate::errors::LoggableError;
use crate::errors::ServiceErrorResultExt;
use crate::errors::Status;
use crate::from_request::convert_pushvars;
use crate::from_request::FromRequest;
use crate::into_response::AsyncIntoResponseWith;
use crate::source_control_impl::SourceControlServiceImpl;

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

impl From<anyhow::Error> for LandStackError {
    fn from(e: anyhow::Error) -> Self {
        Self::Service(errors::internal_error(e).into())
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
    async fn maybe_push_redirector(
        &self,
        repo: &RepoContext,
    ) -> Result<Option<(PushRedirector<Repo>, Large<RepoContext>)>, LandStackError> {
        let base = match repo.maybe_push_redirector_base() {
            None => return Ok(None),
            Some(base) => base,
        };
        let live_commit_sync_config = repo.live_commit_sync_config();
        let enabled = live_commit_sync_config.push_redirector_enabled_for_public(repo.repoid());
        if enabled {
            let large_repo_id = base.common_commit_sync_config.large_repo_id;
            let target_repo = self
                .mononoke
                .raw_repo_by_id(large_repo_id.id())
                .ok_or_else(|| errors::repo_not_found(format!("Large repo {}", large_repo_id)))?;
            let large_repo_ctx = self
                .mononoke
                .repo_by_id(repo.ctx().clone(), large_repo_id)
                .await?
                .ok_or_else(|| {
                    errors::repo_not_found(format!("Large repo {} not found", large_repo_id))
                })?
                .with_authorization_context(repo.authorization_context().clone())
                .build()
                .await?;
            Ok(Some((
                PushRedirectorArgs::new(
                    target_repo,
                    repo.mononoke_api_repo(),
                    base.synced_commit_mapping.clone(),
                    base.target_repo_dbs.clone(),
                )
                .into_push_redirector(
                    repo.ctx(),
                    live_commit_sync_config,
                    repo.inner_repo().repo_cross_repo.sync_lease().clone(),
                )?,
                Large(large_repo_ctx),
            )))
        } else {
            Ok(None)
        }
    }

    async fn impl_repo_land_stack(
        &self,
        ctx: CoreContext,
        repo: thrift::RepoSpecifier,
        params: thrift::RepoLandStackParams,
    ) -> Result<thrift::RepoLandStackResponse, LandStackError> {
        let push_authored_by = if params.service_identity.is_some() {
            PushAuthoredBy::Service
        } else {
            PushAuthoredBy::User
        };
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

        let maybe_pushredirector = if tunables().get_disable_scs_pushredirect() {
            None
        } else {
            self.maybe_push_redirector(&repo).await?
        };

        let pushrebase_outcome = repo
            .land_stack(
                &params.bookmark,
                head.id(),
                base.id(),
                pushvars.as_ref(),
                push_source,
                bookmark_restrictions,
                maybe_pushredirector,
                push_authored_by,
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
