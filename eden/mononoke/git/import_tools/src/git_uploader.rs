/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::time::Duration;

use anyhow::Context;
use anyhow::Error;
use anyhow::format_err;
use async_trait::async_trait;
use auto_impl::auto_impl;
use bonsai_git_mapping::BonsaiGitMappingEntry;
use bonsai_git_mapping::BonsaiGitMappingRef;
use bonsai_git_mapping::BonsaisOrGitShas;
use bonsai_tag_mapping::BonsaiTagMappingRef;
use bulk_derivation::BulkDerivation;
use bytes::Bytes;
use changesets_creation::save_changesets;
use cloned::cloned;
use commit_graph::CommitGraphRef;
use commit_graph::CommitGraphWriterRef;
use context::CoreContext;
use filestore::FilestoreConfigRef;
use filestore::StoreRequest;
use futures::Stream;
use futures::stream;
use futures_retry::retry;
use futures_stats::TimedTryFutureExt;
use gix_hash::ObjectId;
use mononoke_types::BonsaiChangeset;
use mononoke_types::BonsaiChangesetMut;
use mononoke_types::ChangesetId;
use mononoke_types::DerivableType;
use mononoke_types::FileChange;
use mononoke_types::FileType;
use mononoke_types::GitLfs;
use mononoke_types::NonRootMPath;
use mononoke_types::hash;
use repo_blobstore::RepoBlobstoreRef;
use repo_derived_data::RepoDerivedDataRef;
use repo_identity::RepoIdentityRef;
use slog::debug;
use slog::info;
use sorted_vector_map::SortedVectorMap;

use crate::BackfillDerivation;
use crate::CommitMetadata;
use crate::GitImportLfs;
use crate::GitimportAccumulator;
use crate::HGGIT_COMMIT_ID_EXTRA;
use crate::HGGIT_MARKER_EXTRA;
use crate::HGGIT_MARKER_VALUE;
use crate::TagMetadata;

const BASE_RETRY_DELAY: Duration = Duration::from_secs(1);
const RETRY_ATTEMPTS: usize = 4;

#[derive(Clone, Copy, Debug)]
pub enum ReuploadCommits {
    Never,
    Always,
}

impl ReuploadCommits {
    pub fn reupload_commit(&self) -> bool {
        match self {
            ReuploadCommits::Never => false,
            ReuploadCommits::Always => true,
        }
    }
}

#[async_trait]
#[auto_impl(&, Arc, Box)]
pub trait GitUploader: Clone + Send + Sync + 'static {
    /// The type of a file change to be uploaded
    type Change: Clone + Send + Sync + 'static;

    /// The type of a changeset returned by generate_changeset
    type IntermediateChangeset: Send + Sync;

    /// Returns a change representing a deletion
    fn deleted() -> Self::Change;

    /// Preload a number of commits, allowing us to batch the
    /// lookups in the bonsai_git_mapping table, largely reducing
    /// the I/O load
    async fn preload_uploaded_commits(
        &self,
        ctx: &CoreContext,
        oids: &[gix_hash::ObjectId],
    ) -> Result<Vec<(gix_hash::ObjectId, ChangesetId)>, Error>;

    /// Looks to see if we can elide importing a commit
    /// If you can give us the ChangesetId for a given git object,
    /// then we assume that it's already imported and skip it
    async fn check_commit_uploaded(
        &self,
        ctx: &CoreContext,
        oid: &gix_hash::oid,
    ) -> Result<Option<ChangesetId>, Error>;

    /// Upload a single file to the repo
    async fn upload_file(
        &self,
        ctx: &CoreContext,
        lfs: &GitImportLfs,
        path: &NonRootMPath,
        ty: FileType,
        oid: ObjectId,
        git_bytes: Bytes,
    ) -> Result<Self::Change, Error>;

    /// Upload a single git object to the repo blobstore of the mercurial mirror.
    /// Use this method for uploading non-blob git objects (e.g. tree, commit, etc)
    async fn upload_object(
        &self,
        ctx: &CoreContext,
        oid: ObjectId,
        git_bytes: Bytes,
    ) -> Result<(), Error>;

    /// Upload a single packfile item corresponding to a git base object, i.e. commit,
    /// tree, blob or tag
    async fn upload_packfile_base_item(
        &self,
        ctx: &CoreContext,
        oid: ObjectId,
        git_bytes: Bytes,
    ) -> Result<(), Error>;

    /// Generate Git ref content mapping for a given ref name that points to
    /// either a blob or tree object, where `git_hash` is the hash of the object
    /// that is pointed to by the ref
    async fn generate_ref_content_mapping(
        &self,
        ctx: &CoreContext,
        ref_name: String,
        git_hash: ObjectId,
        is_tree: bool,
    ) -> Result<(), Error>;

    /// Generate the intermediate representation of a single Bonsai changeset
    ///
    /// The Bonsai Changeset ID will be generated later in finalize_batch when the
    /// actual changeset has been created.
    async fn generate_intermediate_changeset_for_commit(
        &self,
        ctx: &CoreContext,
        metadata: CommitMetadata,
        changes: SortedVectorMap<NonRootMPath, Self::Change>,
        acc: &GitimportAccumulator,
        dry_run: bool,
    ) -> Result<Self::IntermediateChangeset, Error>;

    /// Generate a single Bonsai changeset ID for corresponding Git
    /// annotated tag.
    async fn generate_changeset_for_annotated_tag(
        &self,
        ctx: &CoreContext,
        target_changeset_id: Option<ChangesetId>,
        tag: TagMetadata,
    ) -> Result<ChangesetId, Error>;

    /// Finalize a batch of generated changesets. The supplied batch is
    /// topologically sorted so that parents are all present before children
    /// If you did not finalize the changeset in generate_changeset,
    /// you must do so here.
    async fn finalize_batch(
        &self,
        ctx: &CoreContext,
        dry_run: bool,
        backfill_derivation: BackfillDerivation,
        changesets: Vec<(hash::GitSha1, Self::IntermediateChangeset)>,
        acc: &GitimportAccumulator,
    ) -> Result<Vec<(hash::GitSha1, ChangesetId)>, Error>;
}

pub trait Repo = CommitGraphRef
    + CommitGraphWriterRef
    + RepoBlobstoreRef
    + BonsaiGitMappingRef
    + BonsaiTagMappingRef
    + FilestoreConfigRef
    + RepoDerivedDataRef
    + RepoIdentityRef
    + Clone
    + Send
    + Sync;

/// Preload a number of commits, allowing us to batch the
/// lookups in the bonsai_git_mapping table, largely reducing
/// the I/O load
///
/// Note that the order of oids must be maintained as they
/// are topologically sorted during repo_import.
pub async fn preload_uploaded_commits(
    repo: &impl Repo,
    ctx: &CoreContext,
    oids: &[gix_hash::ObjectId],
    reupload_commits: ReuploadCommits,
) -> Result<Vec<(gix_hash::ObjectId, ChangesetId)>, Error> {
    if reupload_commits.reupload_commit() {
        return Ok(Vec::new());
    }
    let git_sha1s = BonsaisOrGitShas::GitSha1(
        oids.iter()
            .map(|oid| hash::GitSha1::from_bytes(oid.as_bytes()))
            .collect::<Result<Vec<_>, _>>()?,
    );
    let (result, _) = retry(
        |_| repo.bonsai_git_mapping().get(ctx, git_sha1s.clone()),
        BASE_RETRY_DELAY,
    )
    .binary_exponential_backoff()
    .max_attempts(RETRY_ATTEMPTS)
    .inspect_err(|attempt, _err| {
        info!(ctx.logger(), "attempt {attempt} of {RETRY_ATTEMPTS} failed")
    })
    .await?;
    let map = result
        .into_iter()
        .map(|entry| {
            let oid = entry.git_sha1.to_object_id()?;
            anyhow::Ok((oid, entry.bcs_id))
        })
        .collect::<Result<HashMap<_, _>, _>>()?;
    Ok(oids
        .iter()
        .filter_map(|oid| Some((*oid, *map.get(oid)?)))
        .collect())
}

/// Looks to see if we can elide importing a commit
/// If you can give us the ChangesetId for a given git object,
/// then we assume that it's already imported and skip it
pub async fn check_commit_uploaded(
    repo: &impl Repo,
    ctx: &CoreContext,
    oid: &gix_hash::oid,
    reupload_commits: ReuploadCommits,
) -> Result<Option<ChangesetId>, Error> {
    if reupload_commits.reupload_commit() {
        return Ok(None);
    }
    repo.bonsai_git_mapping()
        .get_bonsai_from_git_sha1(ctx, hash::GitSha1::from_bytes(oid.as_bytes())?)
        .await
}

/// Upload a single file to the repo
pub async fn upload_file(
    repo: &impl Repo,
    ctx: &CoreContext,
    lfs: &GitImportLfs,
    path: &NonRootMPath,
    ty: FileType,
    oid: ObjectId,
    git_bytes: Bytes,
) -> Result<FileChange, Error> {
    let (meta, git_lfs) = if ty == FileType::GitSubmodule {
        // The file is a git submodule.  In Mononoke, we store the commit
        // id of the submodule as the content of the file.
        let oid_bytes = Bytes::copy_from_slice(oid.as_slice());
        let meta = filestore::store(
            repo.repo_blobstore(),
            *repo.filestore_config(),
            ctx,
            &StoreRequest::new(oid_bytes.len() as u64),
            stream::once(async move { Ok(oid_bytes) }),
        )
        .await
        .context("filestore (upload submodule)")?;
        (meta, GitLfs::FullContent)
    } else if let Some(lfs_pointer_data) = lfs.is_lfs_file(&git_bytes, oid) {
        let blobstore = repo.repo_blobstore();
        let filestore_config = *repo.filestore_config();
        cloned!(lfs, blobstore, path);
        // We want to store both:
        // 1. actual file the pointer is pointing at
        let (meta, fetch_result) = lfs
            .with(ctx.clone(), lfs_pointer_data.clone(), {
                move |ctx, lfs_pointer_data, req, bstream, fetch_result| async move {
                    info!(
                        ctx.logger(),
                        "Uploading LFS {} sha256:{} size:{}",
                        path,
                        lfs_pointer_data.sha256.to_brief(),
                        lfs_pointer_data.size,
                    );
                    Ok((
                        filestore::store(&blobstore, filestore_config, &ctx, &req, bstream)
                            .await
                            .context("filestore (lfs contents)")?,
                        fetch_result,
                    ))
                }
            })
            .await?;

        if fetch_result.is_not_found() {
            // In case the pointer wasn't found (and we allow that), mark the pointer as full
            // content.
            (meta, GitLfs::FullContent)
        } else {
            // 3. Upload the Git LFS pointer itself
            let (req, bstream) =
                git_store_request(ctx, oid, git_bytes).context("git_store_request")?;
            let pointer_meta = filestore::store(
                repo.repo_blobstore(),
                *repo.filestore_config(),
                ctx,
                &req,
                bstream,
            )
            .await
            .context("filestore (lfs pointer)")?;
            // and return the contents of the actual file
            let pointer = if lfs_pointer_data.is_canonical {
                GitLfs::canonical_pointer()
            } else {
                GitLfs::non_canonical_pointer(pointer_meta.content_id)
            };
            (meta, pointer)
        }
    } else {
        let (req, bstream) = git_store_request(ctx, oid, git_bytes).context("git_store_request")?;
        let meta = filestore::store(
            repo.repo_blobstore(),
            *repo.filestore_config(),
            ctx,
            &req,
            bstream,
        )
        .await
        .context("filestore (upload regular)")?;
        (meta, GitLfs::FullContent)
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
        git_lfs,
    ))
}

/// Generate a single Bonsai changeset ID for corresponding Git commit
/// This should delay saving the changeset if possible
/// but may save it if required.
///
/// You are guaranteed that all parents of the given changeset
/// have been generated by this point.
pub async fn generate_changeset_for_commit(
    metadata: CommitMetadata,
    changes: SortedVectorMap<NonRootMPath, FileChange>,
    acc: &GitimportAccumulator,
) -> Result<BonsaiChangeset, Error> {
    let oid = metadata.oid;
    let bonsai_parents = metadata
        .parents
        .iter()
        .map(|p| {
            acc.get(p).ok_or_else(|| {
                format_err!(
                    "Couldn't find parent: {} in local list of imported commits",
                    p
                )
            })
        })
        .collect::<Result<Vec<_>, _>>()
        .with_context(|| format_err!("While looking for parents of {}", oid))?;

    let bcs = generate_bonsai_changeset(metadata, bonsai_parents, changes)?;
    acc.insert(oid, bcs.get_changeset_id());
    Ok(bcs)
}

/// Finalize a batch of generated changesets. The supplied batch is
/// topologically sorted so that parents are all present before children
/// If you did not finalize the changeset in generate_changeset,
/// you must do so here.
pub async fn finalize_batch(
    repo: &impl Repo,
    ctx: &CoreContext,
    dry_run: bool,
    backfill_derivation: BackfillDerivation,
    changesets: Vec<(hash::GitSha1, BonsaiChangeset)>,
) -> Result<Vec<(hash::GitSha1, ChangesetId)>, Error> {
    let oid_to_bcsid = changesets
        .iter()
        .map(|(git_sha1, bcs)| BonsaiGitMappingEntry::new(*git_sha1, bcs.get_changeset_id()))
        .collect::<Vec<BonsaiGitMappingEntry>>();
    let vbcs = changesets
        .into_iter()
        .map(|(_, bcsid)| bcsid)
        .collect::<Vec<_>>();
    let ret = oid_to_bcsid
        .iter()
        .map(|entry| (entry.git_sha1, entry.bcs_id))
        .collect();

    if dry_run {
        // Short circuit the steps that write
        return Ok(ret);
    }

    // We know that the commits are in order (this is guaranteed by the Walk), so we
    // can insert them as-is, one by one, without extra dependency / ordering checks.
    let ((stats, ()), num_attempts) = retry(
        |_| save_changesets(ctx, repo, vbcs.clone()).try_timed(),
        BASE_RETRY_DELAY,
    )
    .binary_exponential_backoff()
    .max_attempts(RETRY_ATTEMPTS)
    .inspect_err(|attempt, _err| {
        info!(ctx.logger(), "attempt {attempt} of {RETRY_ATTEMPTS} failed")
    })
    .await?;

    debug!(
        ctx.logger(),
        "save_changesets for {} commits in {:?} after {} attempts",
        oid_to_bcsid.len(),
        stats.completion_time,
        num_attempts
    );

    let csids = oid_to_bcsid
        .iter()
        .map(|entry| entry.bcs_id)
        .collect::<Vec<_>>();
    let batch_size = csids.len() as u64;
    let config = repo.repo_derived_data().active_config();

    // Derive all types that don't depend on GitCommit
    let non_git_types = backfill_derivation
        .types(&config.types)
        .into_iter()
        .filter(|dt| match dt {
            DerivableType::GitCommits
            | DerivableType::GitDeltaManifestsV2
            | DerivableType::GitDeltaManifestsV3 => false,
            _ => true,
        })
        .collect::<Vec<_>>();
    repo.repo_derived_data()
        .manager()
        .derive_bulk_locally(ctx, &csids, None, &non_git_types, Some(batch_size))
        .await?;

    // Upload all bonsai git mappings.
    // This is done instead of deriving git commits. It is not equivalent as roundtrip from
    // git to bonsai and back is not guaranteed.
    // We want to do this as late as possible.
    // Ideally, we would want to do it last as this is what is used to determine whether
    // it is safe to proceed from there.
    // We can't actually do it last as it must be done before deriving `GitDeltaManifest`
    // since that depends on git commits.
    retry(
        |_| repo.bonsai_git_mapping().bulk_add(ctx, &oid_to_bcsid),
        BASE_RETRY_DELAY,
    )
    .binary_exponential_backoff()
    .max_attempts(RETRY_ATTEMPTS)
    .inspect_err(|attempt, _err| {
        info!(ctx.logger(), "attempt {attempt} of {RETRY_ATTEMPTS} failed")
    })
    .await?;
    // derive git delta manifests: note: GitCommit don't need to be explicitly
    // derived as they were already imported
    let delta_manifests = backfill_derivation
        .types(&config.types)
        .into_iter()
        .filter(|dt| match dt {
            DerivableType::GitDeltaManifestsV2 | DerivableType::GitDeltaManifestsV3 => true,
            _ => false,
        })
        .collect::<Vec<_>>();
    repo.repo_derived_data()
        .manager()
        .derive_bulk_locally(ctx, &csids, None, &delta_manifests, Some(batch_size))
        .await?;

    Ok(ret)
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
        git_tree_hash: None,
        file_changes,
        git_annotated_tag: None,
        ..Default::default()
    }
    .freeze()
}

fn git_store_request(
    ctx: &CoreContext,
    git_id: ObjectId,
    git_bytes: Bytes,
) -> Result<
    (
        StoreRequest,
        impl Stream<Item = Result<Bytes, Error>> + use<>,
    ),
    Error,
> {
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
