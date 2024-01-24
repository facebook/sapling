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
use blobrepo::save_bonsai_changesets;
use bonsai_git_mapping::BonsaiGitMappingEntry;
use bonsai_git_mapping::BonsaiGitMappingRef;
use bonsai_tag_mapping::BonsaiTagMappingRef;
use bytes::Bytes;
use changesets::ChangesetsRef;
use cloned::cloned;
use context::CoreContext;
use filestore::FilestoreConfigRef;
use filestore::StoreRequest;
use futures::stream;
use futures::stream::Stream;
use futures_stats::TimedTryFutureExt;
use gix_hash::ObjectId;
use import_tools::CommitMetadata;
use import_tools::GitImportLfs;
use import_tools::GitUploader;
use import_tools::TagMetadata;
use import_tools::HGGIT_COMMIT_ID_EXTRA;
use import_tools::HGGIT_MARKER_EXTRA;
use import_tools::HGGIT_MARKER_VALUE;
use mononoke_api::repo::git::create_annotated_tag;
use mononoke_api::repo::git::upload_packfile_base_item;
use mononoke_api::repo::upload_non_blob_git_object;
use mononoke_types::bonsai_changeset::BonsaiAnnotatedTag;
use mononoke_types::bonsai_changeset::BonsaiAnnotatedTagTarget;
use mononoke_types::hash;
use mononoke_types::BonsaiChangeset;
use mononoke_types::BonsaiChangesetMut;
use mononoke_types::ChangesetId;
use mononoke_types::FileChange;
use mononoke_types::FileType;
use mononoke_types::NonRootMPath;
use repo_blobstore::RepoBlobstoreRef;
use slog::debug;
use slog::info;
use sorted_vector_map::SortedVectorMap;

#[derive(Clone, Copy, Debug)]
pub enum ReuploadCommits {
    Never,
    Always,
}

impl ReuploadCommits {
    fn reupload_commit(&self) -> bool {
        match self {
            ReuploadCommits::Never => false,
            ReuploadCommits::Always => true,
        }
    }
}

#[derive(Clone)]
pub struct DirectUploader<R> {
    inner: Arc<R>,
    reupload_commits: ReuploadCommits,
}

impl<R> DirectUploader<R> {
    pub fn new(repo: R, reupload_commits: ReuploadCommits) -> Self {
        Self {
            inner: Arc::new(repo),
            reupload_commits,
        }
    }
}

#[async_trait]
impl<R> GitUploader for DirectUploader<R>
where
    R: ChangesetsRef
        + RepoBlobstoreRef
        + BonsaiGitMappingRef
        + BonsaiTagMappingRef
        + FilestoreConfigRef
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

    async fn check_commit_uploaded(
        &self,
        ctx: &CoreContext,
        oid: &gix_hash::oid,
    ) -> Result<Option<ChangesetId>, Error> {
        if self.reupload_commits.reupload_commit() {
            return Ok(None);
        }

        self.inner
            .bonsai_git_mapping()
            .get_bonsai_from_git_sha1(ctx, hash::GitSha1::from_bytes(oid.as_bytes())?)
            .await
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
        let meta = if ty == FileType::GitSubmodule {
            // The file is a git submodule.  In Mononoke, we store the commit
            // id of the submodule as the content of the file.
            let oid_bytes = Bytes::copy_from_slice(oid.as_slice());
            filestore::store(
                self.inner.repo_blobstore(),
                *self.inner.filestore_config(),
                ctx,
                &StoreRequest::new(oid_bytes.len() as u64),
                stream::once(async move { Ok(oid_bytes) }),
            )
            .await?
        } else if let Some(lfs_meta) = lfs.is_lfs_file(&git_bytes, oid) {
            let blobstore = self.inner.repo_blobstore();
            let filestore_config = *self.inner.filestore_config();
            cloned!(ctx, lfs, blobstore, path);
            lfs.with(
                ctx,
                lfs_meta,
                move |ctx, lfs_meta, req, bstream| async move {
                    info!(
                        ctx.logger(),
                        "Uploading LFS {} sha256:{} size:{}",
                        path,
                        lfs_meta.sha256.to_brief(),
                        lfs_meta.size,
                    );
                    filestore::store(&blobstore, filestore_config, &ctx, &req, bstream).await
                },
            )
            .await?
        } else {
            let (req, bstream) = git_store_request(ctx, oid, git_bytes)?;
            filestore::store(
                self.inner.repo_blobstore(),
                *self.inner.filestore_config(),
                ctx,
                &req,
                bstream,
            )
            .await?
        };
        debug!(
            ctx.logger(),
            "Uploaded {} blob {}",
            path,
            oid.to_hex_with_len(8),
        );
        Ok(FileChange::tracked(
            meta.content_id,
            ty,
            meta.total_size,
            None,
        ))
    }

    async fn generate_changeset_for_commit(
        &self,
        _ctx: &CoreContext,
        bonsai_parents: Vec<ChangesetId>,
        metadata: CommitMetadata,
        changes: SortedVectorMap<NonRootMPath, Self::Change>,
        _dry_run: bool,
    ) -> Result<(Self::IntermediateChangeset, ChangesetId), Error> {
        let bcs = generate_bonsai_changeset(metadata, bonsai_parents, changes)?;
        let bcs_id = bcs.get_changeset_id();
        Ok((bcs, bcs_id))
    }

    async fn finalize_batch(
        &self,
        ctx: &CoreContext,
        dry_run: bool,
        changesets: Vec<(Self::IntermediateChangeset, hash::GitSha1)>,
    ) -> Result<(), Error> {
        let oid_to_bcsid = changesets
            .iter()
            .map(|(bcs, git_sha1)| BonsaiGitMappingEntry::new(*git_sha1, bcs.get_changeset_id()))
            .collect::<Vec<BonsaiGitMappingEntry>>();
        let vbcs = changesets.into_iter().map(|x| x.0).collect();

        // We know that the commits are in order (this is guaranteed by the Walk), so we
        // can insert them as-is, one by one, without extra dependency / ordering checks.
        let (stats, ()) = save_bonsai_changesets(vbcs, ctx.clone(), &*self.inner)
            .try_timed()
            .await?;
        debug!(
            ctx.logger(),
            "save_bonsai_changesets for {} commits in {:?}",
            oid_to_bcsid.len(),
            stats.completion_time
        );

        if !dry_run {
            self.inner
                .bonsai_git_mapping()
                .bulk_add(ctx, &oid_to_bcsid)
                .await?;
        }

        Ok(())
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
        let tag_name = format!("tags/{}", tag.name);
        create_annotated_tag(
            ctx,
            &*self.inner,
            Some(tag.oid),
            tag_name,
            tag.author.take(),
            tag.author_date.take().map(|date| date.into()),
            tag.message,
            annotated_tag,
        )
        .await
        .context("Failure in creating changeset for tag")
    }
}

fn git_store_request(
    ctx: &CoreContext,
    git_id: ObjectId,
    git_bytes: Bytes,
) -> Result<(StoreRequest, impl Stream<Item = Result<Bytes, Error>>), Error> {
    let size = git_bytes.len().try_into()?;
    let git_sha1 =
        hash::RichGitSha1::from_bytes(Bytes::copy_from_slice(git_id.as_bytes()), "blob", size)?;
    let req = StoreRequest::with_git_sha1(size, git_sha1);
    debug!(
        ctx.logger(),
        "Uploading git-blob:{} size:{}",
        git_sha1.sha1().to_brief(),
        size
    );
    Ok((req, stream::once(async move { Ok(git_bytes) })))
}

fn generate_bonsai_changeset(
    metadata: CommitMetadata,
    parents: Vec<ChangesetId>,
    file_changes: SortedVectorMap<NonRootMPath, FileChange>,
) -> Result<BonsaiChangeset, Error> {
    let CommitMetadata {
        oid,
        message,
        author,
        author_date,
        committer,
        committer_date,
        git_extra_headers,
        ..
    } = metadata;
    let git_extra_headers = if git_extra_headers.is_empty() {
        None
    } else {
        Some(git_extra_headers)
    };
    let mut extra = SortedVectorMap::new();
    extra.insert(
        HGGIT_COMMIT_ID_EXTRA.to_string(),
        oid.to_string().into_bytes(),
    );
    extra.insert(HGGIT_MARKER_EXTRA.to_string(), HGGIT_MARKER_VALUE.to_vec());

    BonsaiChangesetMut {
        parents,
        author,
        author_date,
        committer: Some(committer),
        committer_date: Some(committer_date),
        message,
        hg_extra: extra,
        git_extra_headers,
        // TODO(rajshar): Populate these fields correctly instead of using empty values
        git_tree_hash: None,
        file_changes,
        is_snapshot: false,
        git_annotated_tag: None,
    }
    .freeze()
}
