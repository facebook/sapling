/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use context::CoreContext;
use mononoke_api::errors::MononokeError;
use mononoke_api::repo::git::GitError;
use source_control as thrift;

use crate::errors::ServiceErrorResultExt;
use crate::errors::{self};
use crate::source_control_impl::SourceControlServiceImpl;

impl SourceControlServiceImpl {
    /// Upload raw git object to Mononoke data store for back-and-forth translation.
    /// Not to be used for uploading raw file content blobs.
    pub(crate) async fn upload_git_object(
        &self,
        ctx: CoreContext,
        repo: thrift::RepoSpecifier,
        params: thrift::UploadGitObjectParams,
    ) -> Result<thrift::UploadGitObjectResponse, errors::ServiceError> {
        let repo_ctx = self
            .repo_for_service(ctx, &repo, params.service_identity.clone())
            .await
            .with_context(|| format!("Error in opening repo using specifier {:?}", repo))?;
        // Validate that the request sender has an internal service identity with the right permission.
        repo_ctx
            .authorization_context()
            .require_git_import_operations(repo_ctx.ctx(), repo_ctx.inner_repo())
            .await
            .map_err(MononokeError::from)?;
        // Validate that the bytes correspond to a valid git hash.
        let git_hash = git_hash::oid::try_from_bytes(&params.git_hash)
            .map_err(|_| GitError::InvalidHash(format!("{:x?}", params.git_hash)))?;
        repo_ctx
            .upload_git_object(git_hash, params.raw_content)
            .await?;
        Ok(thrift::UploadGitObjectResponse {
            ..Default::default()
        })
    }

    /// Create Mononoke counterpart of git tree object in the form of a bonsai changeset.
    /// The raw git tree object must already be stored in Mononoke stores before invoking
    /// this endpoint.
    pub(crate) async fn create_git_tree(
        &self,
        ctx: CoreContext,
        repo: thrift::RepoSpecifier,
        params: thrift::CreateGitTreeParams,
    ) -> Result<thrift::CreateGitTreeResponse, errors::ServiceError> {
        let repo_ctx = self
            .repo_for_service(ctx, &repo, params.service_identity.clone())
            .await
            .with_context(|| format!("Error in opening repo using specifier {:?}", repo))?;
        // Validate that the request sender has an internal service identity with the right permission.
        repo_ctx
            .authorization_context()
            .require_git_import_operations(repo_ctx.ctx(), repo_ctx.inner_repo())
            .await
            .map_err(MononokeError::from)?;
        // Validate that the provided byte content constitutes a hash.
        let git_tree_hash = git_hash::oid::try_from_bytes(&params.git_tree_hash)
            .map_err(|_| GitError::InvalidHash(format!("{:x?}", params.git_tree_hash)))?;
        repo_ctx.create_git_tree(git_tree_hash).await?;
        Ok(thrift::CreateGitTreeResponse {
            ..Default::default()
        })
    }
}
