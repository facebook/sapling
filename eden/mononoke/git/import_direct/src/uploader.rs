/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use anyhow::Context;
use anyhow::Error;
use async_trait::async_trait;
use bonsai_git_mapping::BonsaiGitMappingRef;
use bonsai_tag_mapping::BonsaiTagMappingRef;
use bytes::Bytes;
use commit_graph::CommitGraphRef;
use commit_graph::CommitGraphWriterRef;
use context::CoreContext;
use filestore::FilestoreConfigRef;
use gix_hash::ObjectId;
use import_tools::git_uploader::check_commit_uploaded;
use import_tools::git_uploader::finalize_batch;
use import_tools::git_uploader::generate_changeset_for_commit;
use import_tools::git_uploader::preload_uploaded_commits;
use import_tools::git_uploader::upload_file;
use import_tools::git_uploader::ReuploadCommits;
use import_tools::BackfillDerivation;
use import_tools::CommitMetadata;
use import_tools::GitImportLfs;
use import_tools::GitUploader;
use import_tools::GitimportAccumulator;
use import_tools::TagMetadata;
use mononoke_api::repo::git::create_annotated_tag;
use mononoke_api::repo::git::upload_packfile_base_item;
use mononoke_api::repo::upload_non_blob_git_object;
use mononoke_types::bonsai_changeset::BonsaiAnnotatedTag;
use mononoke_types::bonsai_changeset::BonsaiAnnotatedTagTarget;
use mononoke_types::hash;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ChangesetId;
use mononoke_types::FileChange;
use mononoke_types::FileType;
use mononoke_types::NonRootMPath;
use repo_blobstore::RepoBlobstoreRef;
use repo_derived_data::RepoDerivedDataRef;
use repo_identity::RepoIdentityRef;
use sorted_vector_map::SortedVectorMap;

#[derive(Clone)]
pub struct DirectUploader<R> {
    inner: Arc<R>,
    reupload_commits: ReuploadCommits,
}

impl<R> DirectUploader<R> {
    pub fn new(repo: R, reupload_commits: ReuploadCommits) -> Self {
        Self::with_arc(Arc::new(repo), reupload_commits)
    }

    pub fn with_arc(repo: Arc<R>, reupload_commits: ReuploadCommits) -> Self {
        Self {
            inner: repo,
            reupload_commits,
        }
    }

    pub fn repo(&self) -> &R {
        &self.inner
    }
}

#[async_trait]
impl<R> GitUploader for DirectUploader<R>
where
    R: CommitGraphRef
        + CommitGraphWriterRef
        + RepoBlobstoreRef
        + BonsaiGitMappingRef
        + BonsaiTagMappingRef
        + FilestoreConfigRef
        + RepoDerivedDataRef
        + RepoIdentityRef
        + Clone
        + Send
        + Sync
        + 'static,
{
    type Change = FileChange;
    type IntermediateChangeset = BonsaiChangeset;

    fn deleted() -> Self::Change {
        FileChange::Deletion
    }

    async fn preload_uploaded_commits(
        &self,
        ctx: &CoreContext,
        oids: &[gix_hash::ObjectId],
    ) -> Result<Vec<(gix_hash::ObjectId, ChangesetId)>, Error> {
        preload_uploaded_commits(self.repo(), ctx, oids, self.reupload_commits).await
    }

    async fn check_commit_uploaded(
        &self,
        ctx: &CoreContext,
        oid: &gix_hash::oid,
    ) -> Result<Option<ChangesetId>, Error> {
        check_commit_uploaded(self.repo(), ctx, oid, self.reupload_commits).await
    }

    /// Upload a single packfile item corresponding to a git base object, i.e. commit,
    /// tree, blob or tag
    async fn upload_packfile_base_item(
        &self,
        ctx: &CoreContext,
        oid: ObjectId,
        git_bytes: Bytes,
    ) -> Result<(), Error> {
        upload_packfile_base_item(ctx, self.inner.repo_blobstore(), &oid, git_bytes.to_vec())
            .await
            .map_err(|e| {
                anyhow::anyhow!(
                    "Failure in uploading packfile base item for git object. Cause: {}",
                    e.to_string()
                )
            })
    }

    async fn upload_file(
        &self,
        ctx: &CoreContext,
        lfs: &GitImportLfs,
        path: &NonRootMPath,
        ty: FileType,
        oid: ObjectId,
        git_bytes: Bytes,
    ) -> Result<Self::Change, Error> {
        upload_file(self.repo(), ctx, lfs, path, ty, oid, git_bytes).await
    }

    async fn generate_intermediate_changeset_for_commit(
        &self,
        _ctx: &CoreContext,
        metadata: CommitMetadata,
        changes: SortedVectorMap<NonRootMPath, Self::Change>,
        acc: &GitimportAccumulator,
        _dry_run: bool,
    ) -> Result<Self::IntermediateChangeset, Error> {
        generate_changeset_for_commit(metadata, changes, acc).await
    }

    async fn finalize_batch(
        &self,
        ctx: &CoreContext,
        dry_run: bool,
        backfill_derivation: BackfillDerivation,
        changesets: Vec<(hash::GitSha1, Self::IntermediateChangeset)>,
        _acc: &GitimportAccumulator,
    ) -> Result<Vec<(hash::GitSha1, ChangesetId)>, Error> {
        finalize_batch(self.repo(), ctx, dry_run, backfill_derivation, changesets).await
    }

    async fn upload_object(
        &self,
        ctx: &CoreContext,
        oid: ObjectId,
        git_bytes: Bytes,
    ) -> Result<(), Error> {
        upload_non_blob_git_object(ctx, self.inner.repo_blobstore(), &oid, git_bytes.to_vec())
            .await
            .map_err(|e| {
                anyhow::anyhow!(
                    "Failure in uploading raw git object. Cause: {}",
                    e.to_string()
                )
            })
    }

    async fn generate_changeset_for_annotated_tag(
        &self,
        ctx: &CoreContext,
        target_changeset_id: ChangesetId,
        mut tag: TagMetadata,
    ) -> Result<ChangesetId, Error> {
        let annotated_tag = BonsaiAnnotatedTag {
            target: BonsaiAnnotatedTagTarget::Changeset(target_changeset_id),
            pgp_signature: tag.pgp_signature.take(),
        };
        create_annotated_tag(
            ctx,
            &*self.inner,
            Some(tag.oid),
            tag.name,
            tag.author.take(),
            tag.author_date.take().map(|date| date.into()),
            tag.message,
            annotated_tag,
            tag.target_is_tag,
        )
        .await
        .context("Failure in creating changeset for tag")
    }
}
