/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use bookmarks_movement::BookmarkKindRestrictions;
use borrowed::borrowed;
use context::CoreContext;
use hooks::CrossRepoPushSource;
use mononoke_api::ChangesetSpecifier;
use permission_checker::MononokeIdentity;
use source_control as thrift;

use crate::commit_id::CommitIdExt;
use crate::errors;
use crate::errors::ServiceErrorResultExt;
use crate::from_request::convert_pushvars;
use crate::from_request::FromRequest;
use crate::into_response::AsyncIntoResponseWith;
use crate::source_control_impl::SourceControlServiceImpl;

impl SourceControlServiceImpl {
    pub(crate) async fn repo_land_stack(
        &self,
        ctx: CoreContext,
        repo: thrift::RepoSpecifier,
        params: thrift::RepoLandStackParams,
    ) -> Result<thrift::RepoLandStackResponse, errors::ServiceError> {
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
}
