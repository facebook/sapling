/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use bytes::Bytes;
use context::CoreContext;
use everstore_client::cpp_client::ClientOptionsBuilder;
use everstore_client::cpp_client::EverstoreCppClient;
use everstore_client::write::WriteRequestOptionsBuilder;
use everstore_client::EverstoreClient;
use fbtypes::FBType;
use futures_util::try_join;
use git_types::GitError;
use mononoke_api::errors::MononokeError;
use mononoke_api::ChangesetId;
use mononoke_types::bonsai_changeset::BonsaiAnnotatedTag;
use mononoke_types::bonsai_changeset::BonsaiAnnotatedTagTarget;
use source_control as thrift;

use crate::errors::internal_error;
use crate::errors::invalid_request;
use crate::errors::ServiceErrorResultExt;
use crate::errors::{self};
use crate::source_control_impl::SourceControlServiceImpl;

const EVERSTORE_CONTEXT: &str = "mononoke/scs";

impl SourceControlServiceImpl {
    /// Upload raw git object to Mononoke data store for back-and-forth translation.
    /// Not to be used for uploading raw file content blobs.
    pub(crate) async fn repo_upload_non_blob_git_object(
        &self,
        ctx: CoreContext,
        repo: thrift::RepoSpecifier,
        params: thrift::RepoUploadNonBlobGitObjectParams,
    ) -> Result<thrift::RepoUploadNonBlobGitObjectResponse, errors::ServiceError> {
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
        let git_hash = gix_hash::oid::try_from_bytes(&params.git_hash)
            .map_err(|_| GitError::InvalidHash(format!("{:x?}", params.git_hash)))?;
        repo_ctx
            .upload_non_blob_git_object(git_hash, params.raw_content)
            .await?;
        Ok(thrift::RepoUploadNonBlobGitObjectResponse {
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
        let git_tree_hash = gix_hash::oid::try_from_bytes(&params.git_tree_hash)
            .map_err(|_| GitError::InvalidHash(format!("{:x?}", params.git_tree_hash)))?;
        repo_ctx.create_git_tree(git_tree_hash).await?;
        Ok(thrift::CreateGitTreeResponse {
            ..Default::default()
        })
    }

    /// Create Mononoke counterpart of git tag object in the form of a bonsai changeset.
    /// The raw git tag object must already be stored in Mononoke stores before invoking
    /// this endpoint.
    pub(crate) async fn create_git_tag(
        &self,
        ctx: CoreContext,
        repo: thrift::RepoSpecifier,
        params: thrift::CreateGitTagParams,
    ) -> Result<thrift::CreateGitTagResponse, errors::ServiceError> {
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
        let author_date = params
            .author_date
            .as_ref()
            .map(|date| mononoke_types::DateTime::from_timestamp(date.timestamp, date.tz))
            .transpose()
            .map_err(|err| {
                invalid_request(format!(
                    "Error in creating author date from {:?}. Cause: {:#}",
                    params.author_date, err
                ))
            })?
            .map(|date| date.into());

        let target_changeset_id =
            ChangesetId::from_bytes(&params.target_changeset).map_err(|err| {
                invalid_request(format!(
                    "Error in creating ChangesetId from {:?}. Cause: {:#}",
                    params.target_changeset, err
                ))
            })?;
        let annotated_tag = BonsaiAnnotatedTag {
            target: BonsaiAnnotatedTagTarget::Changeset(target_changeset_id),
            pgp_signature: params.pgp_signature.map(Bytes::from),
        };
        let tag_hash = params
            .tag_hash
            .as_ref()
            .map(|hash| {
                gix_hash::oid::try_from_bytes(hash)
                    .map(|oid| oid.to_owned())
                    .map_err(|err| {
                        invalid_request(format!(
                            "Error in creating Git ObjectId from {:?}. Cause: {:#}",
                            hash, err
                        ))
                    })
            })
            .transpose()?;
        let changeset_context = repo_ctx
            .create_annotated_tag(
                tag_hash,
                params.tag_name,
                params.author,
                author_date,
                params.annotation,
                annotated_tag,
            )
            .await?;
        Ok(thrift::CreateGitTagResponse {
            created_changeset_id: changeset_context.id().as_ref().to_vec(),
            ..Default::default()
        })
    }

    /// Generate Git bundle for the given stack of commits with the ref BUNDLE_HEAD
    /// pointing to the top of the stack. Store the bundle in everstore and return
    /// the everstore handle associated with it.
    pub(crate) async fn repo_stack_git_bundle_store(
        &self,
        ctx: CoreContext,
        repo: thrift::RepoSpecifier,
        params: thrift::RepoStackGitBundleStoreParams,
    ) -> Result<thrift::RepoStackGitBundleStoreResponse, errors::ServiceError> {
        let repo_ctx = self
            .repo_for_service(ctx, &repo, params.service_identity.clone())
            .await
            .with_context(|| format!("Error in opening repo using specifier {:?}", repo))?;
        // Parse the input as appropriate types
        let (base_changeset_id, head_changeset_id) = try_join!(
            self.changeset_id(&repo_ctx, &params.base),
            self.changeset_id(&repo_ctx, &params.head),
        )?;

        // Generate the bundle
        let bundle_content = repo_ctx
            .repo_stack_git_bundle(head_changeset_id, base_changeset_id)
            .await?;

        // Store the contents of the bundle in everstore
        let client_options = ClientOptionsBuilder::default().build().map_err(|err| {
            internal_error(format!(
                "Error in building Everstore client options. Cause: {:#}",
                err
            ))
        })?;
        let client = EverstoreCppClient::from_options(repo_ctx.ctx().fb, &client_options).map_err(
            |err| {
                internal_error(format!(
                    "Error in building Everstore client. Cause: {:#}",
                    err
                ))
            },
        )?;
        let fbtype = FBType::EVERSTORE_SOURCE_BUNDLE.0 as u32;
        let write_req_opts = WriteRequestOptionsBuilder::default()
            .fbtype(fbtype)
            .lower_bound(10) // We should be able to store even small bundles
            .build()
            .map_err(|err| {
                internal_error(format!(
                    "Error in building Everstore write request options. Cause: {:#}",
                    err
                ))
            })?;
        let mut write_req = client
            .create_write_request(&write_req_opts)
            .map_err(|err| {
                internal_error(format!(
                    "Error in creating Everstore write request. Cause: {:#}",
                    err
                ))
            })?;
        let everstore_handle = write_req
            .write(EVERSTORE_CONTEXT, bundle_content.into())
            .await
            .map_err(|err| {
                internal_error(format!(
                    "Error in storing Git bundle in Everstore. Cause: {:#}",
                    err
                ))
            })?
            .to_string();
        Ok(thrift::RepoStackGitBundleStoreResponse {
            everstore_handle,
            ..Default::default()
        })
    }

    /// Upload packfile base item corresponding to git object to Mononoke data store
    pub(crate) async fn repo_upload_packfile_base_item(
        &self,
        ctx: CoreContext,
        repo: thrift::RepoSpecifier,
        params: thrift::RepoUploadPackfileBaseItemParams,
    ) -> Result<thrift::RepoUploadPackfileBaseItemResponse, errors::ServiceError> {
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
        let git_hash = gix_hash::oid::try_from_bytes(&params.git_hash)
            .map_err(|_| GitError::InvalidHash(format!("{:x?}", params.git_hash)))?;
        repo_ctx
            .repo_upload_packfile_base_item(git_hash, params.raw_content)
            .await?;
        Ok(thrift::RepoUploadPackfileBaseItemResponse {
            ..Default::default()
        })
    }
}
