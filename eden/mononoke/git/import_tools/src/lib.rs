/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![feature(try_blocks)]
#![feature(trait_alias)]
#![feature(string_from_utf8_lossy_owned)]

pub mod bookmark;
pub mod git_reader;
pub mod git_uploader;
mod gitimport_objects;
mod gitlfs;

use std::collections::BTreeMap;
use std::collections::HashMap;
use std::path::Path;
use std::process::Stdio;
use std::str;
use std::sync::Arc;
use std::sync::RwLock;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use anyhow::bail;
use anyhow::format_err;
use bytes::Bytes;
use cloned::cloned;
use context::CoreContext;
use futures::FutureExt;
use futures::Stream;
use futures::StreamExt;
use futures::TryFutureExt;
use futures::TryStreamExt;
use futures::future::BoxFuture;
use futures::future::try_join3;
use futures::stream;
use futures::try_join;
use futures_stats::TimedTryFutureExt;
use git_symbolic_refs::GitSymbolicRefsEntry;
pub use git_types::git_lfs::LfsPointerData;
use gix_hash::ObjectId;
use gix_object::Kind;
use gix_object::ObjectRef;
use linked_hash_map::LinkedHashMap;
use manifest::BonsaiDiffFileChange;
use manifest::find_intersection_of_diffs;
use mononoke_macros::mononoke;
use mononoke_types::ChangesetId;
use mononoke_types::FileType;
use mononoke_types::NonRootMPath;
use scuba_ext::FutureStatsScubaExt;
use slog::debug;
use slog::info;
use sorted_vector_map::SortedVectorMap;
use tokio::io::AsyncBufReadExt;
use tokio::io::BufReader;
use tokio::process::Command;
use tokio::sync::mpsc;

pub use crate::bookmark::BookmarkOperation;
pub use crate::bookmark::set_bookmark;
use crate::git_reader::GitReader;
pub use crate::git_reader::GitRepoReader;
pub use crate::git_uploader::GitUploader;
pub use crate::git_uploader::ReuploadCommits;
pub use crate::gitimport_objects::BackfillDerivation;
pub use crate::gitimport_objects::CommitMetadata;
pub use crate::gitimport_objects::ExtractedCommit;
pub use crate::gitimport_objects::GitLeaf;
pub use crate::gitimport_objects::GitManifest;
pub use crate::gitimport_objects::GitTree;
pub use crate::gitimport_objects::GitimportPreferences;
pub use crate::gitimport_objects::GitimportTarget;
pub use crate::gitimport_objects::TagMetadata;
pub use crate::gitimport_objects::oid_to_sha1;
pub use crate::gitlfs::GitImportLfs;

pub const HGGIT_MARKER_EXTRA: &str = "hg-git-rename-source";
pub const HGGIT_MARKER_VALUE: &[u8] = b"git";
pub const HGGIT_COMMIT_ID_EXTRA: &str = "convert_revision";
pub const BRANCH_REF: &str = "branch";
pub const TAG_REF: &str = "tag";
pub const BRANCH_REF_PREFIX: &str = "refs/heads/";
pub const TAG_REF_PREFIX: &str = "refs/tags/";

// TODO: Try to produce copy-info?
async fn find_file_changes<S, U, R>(
    ctx: &CoreContext,
    lfs: &GitImportLfs,
    reader: Arc<R>,
    uploader: Arc<U>,
    changes: S,
) -> Result<SortedVectorMap<NonRootMPath, U::Change>>
where
    S: Stream<Item = Result<BonsaiDiffFileChange<(FileType, GitLeaf)>>>,
    U: GitUploader,
    R: GitReader,
{
    changes
        .map_ok(|change| async {
            cloned!(ctx, reader, uploader, lfs);
            mononoke::spawn_task({
                async move {
                    match change {
                        BonsaiDiffFileChange::Changed(path, (ty, GitLeaf(oid)))
                        | BonsaiDiffFileChange::ChangedReusedId(path, (ty, GitLeaf(oid))) => {
                            if ty == FileType::GitSubmodule {
                                // The OID for a submodule is a commit in another repository, so there is no data to
                                // store.
                                uploader
                                    .upload_file(&ctx, &lfs, &path, ty, oid, Bytes::new())
                                    .await
                                    .map(|change| (path, change))
                            } else {
                                let object =
                                    reader.get_object(&oid).await.context("reader.get_object")?;
                                let blob = object
                                    .with_parsed(|parsed| {
                                        parsed.as_blob().map(|blob_ref| blob_ref.into_owned())
                                    })
                                    .ok_or_else(|| format_err!("{} is not a blob", oid))?;

                                let upload_packfile = uploader.upload_packfile_base_item(
                                    &ctx,
                                    oid,
                                    object.raw().clone(),
                                );
                                let upload_git_blob = uploader.upload_file(
                                    &ctx,
                                    &lfs,
                                    &path,
                                    ty,
                                    oid,
                                    Bytes::from(blob.data),
                                );
                                let (_, change) = try_join!(upload_packfile, upload_git_blob)?;
                                anyhow::Ok((path, change))
                            }
                        }
                        BonsaiDiffFileChange::Deleted(path) => Ok((path, U::deleted())),
                    }
                }
            })
            .await?
        })
        .try_buffer_unordered(100)
        .try_collect()
        .await
}

// A running tally of mappings for the imported commits, starting from the roots
// Uses a RwLock internally to allow writing in concurrent settings
pub struct GitimportAccumulator {
    roots: HashMap<ObjectId, ChangesetId>,
    inner: RwLock<LinkedHashMap<ObjectId, ChangesetId>>,
}

impl GitimportAccumulator {
    // Create a new accumulator from these known roots
    pub fn from_roots(roots: HashMap<ObjectId, ChangesetId>) -> Self {
        Self {
            roots,
            inner: RwLock::new(LinkedHashMap::new()),
        }
    }

    // How many new commit mappings were accumulated
    pub fn len(&self) -> usize {
        self.inner.read().expect("lock poisoned").len()
    }

    // No new commit mappings were accumulated
    pub fn is_empty(&self) -> bool {
        self.inner.read().expect("lock poisoned").is_empty()
    }

    // Insert a new commit mapping
    pub fn insert(&self, oid: ObjectId, cs_id: ChangesetId) {
        self.inner
            .write()
            .expect("lock poisoned")
            .insert(oid, cs_id);
    }

    // Insert many new commit mappings
    pub fn extend(&self, mappings: Vec<(ObjectId, ChangesetId)>) {
        self.inner.write().expect("lock poisoned").extend(mappings);
    }

    // Get a commit mapping from the roots or the inserted mappings
    pub fn get(&self, oid: &gix_hash::oid) -> Option<ChangesetId> {
        self.roots
            .get(oid)
            .copied()
            .or_else(|| self.inner.read().expect("lock poisoned").get(oid).copied())
    }

    // Extract the newly imported mappings from this accumulator, ending its life
    pub fn into_inner(self) -> LinkedHashMap<ObjectId, ChangesetId> {
        self.inner.into_inner().expect("lock poisoned")
    }

    pub fn roots(&self) -> &'_ HashMap<ObjectId, ChangesetId> {
        &self.roots
    }
}

pub fn stored_tag_name(tag_name: String) -> String {
    tag_name
        .strip_prefix("refs/")
        .map(|s| s.to_string())
        .unwrap_or(tag_name)
}

pub async fn create_changeset_for_annotated_tag<Uploader: GitUploader, Reader: GitReader>(
    ctx: &CoreContext,
    uploader: Arc<Uploader>,
    reader: Arc<Reader>,
    tag_id: &ObjectId,
    maybe_tag_name: Option<String>,
    original_changeset_id: Option<ChangesetId>,
) -> Result<ChangesetId> {
    // Get the parsed Git Tag
    let tag_metadata = TagMetadata::new(ctx, *tag_id, maybe_tag_name, &reader)
        .await
        .with_context(|| format_err!("Failed to create TagMetadata from git tag {}", tag_id))?;
    // Create the corresponding changeset for the Git Tag at Mononoke end
    let changeset_id = uploader
        .generate_changeset_for_annotated_tag(ctx, original_changeset_id, tag_metadata)
        .await
        .with_context(|| format_err!("Failed to generate changeset for git tag {}", tag_id))?;
    Ok(changeset_id)
}

async fn upload_git_object_by_content<Uploader: GitUploader>(
    ctx: &CoreContext,
    uploader: Arc<Uploader>,
    object_id: &ObjectId,
    object_bytes: Bytes,
) -> Result<()> {
    let raw_object_bytes = object_bytes.clone();
    // Upload Packfile Item for the Git object
    let upload_packfile = async {
        uploader
            .upload_packfile_base_item(ctx, *object_id, object_bytes)
            .await
            .with_context(|| {
                format_err!(
                    "Failed to upload packfile item for git object {}",
                    object_id
                )
            })
    };
    // Upload Git object
    let upload_git_object = async {
        uploader
            .upload_object(ctx, *object_id, raw_object_bytes)
            .await
            .with_context(|| format_err!("Failed to upload raw git object {}", object_id))
    };
    try_join!(upload_packfile, upload_git_object)?;
    Ok(())
}

pub async fn upload_git_object<Uploader: GitUploader, Reader: GitReader>(
    ctx: &CoreContext,
    uploader: Arc<Uploader>,
    reader: Arc<Reader>,
    object_id: &ObjectId,
) -> Result<()> {
    let object_bytes = reader
        .read_raw_object(object_id)
        .await
        .with_context(|| format_err!("Failed to fetch git object {}", object_id))?;
    upload_git_object_by_content(ctx, uploader, object_id, object_bytes).await?;
    Ok(())
}

pub async fn upload_git_tree_recursively<Uploader: GitUploader, Reader: GitReader>(
    ctx: &CoreContext,
    uploader: Arc<Uploader>,
    reader: Arc<Reader>,
    tree_id: &ObjectId,
) -> Result<()> {
    // First upload the base tree
    upload_git_object(ctx, uploader.clone(), reader.clone(), tree_id).await?;
    let tree = GitTree::<true>(*tree_id);
    // Then upload all subtrees and blobs within it
    find_intersection_of_diffs(ctx.clone(), reader.clone(), tree, vec![])
        .map_ok(|(_, entry)| {
            cloned!(uploader, reader);
            async move {
                match entry {
                    manifest::Entry::Tree(GitTree(oid))
                    | manifest::Entry::Leaf((_, GitLeaf(oid))) => {
                        upload_git_object(ctx, uploader, reader, &oid).await
                    }
                }
            }
        })
        .try_buffer_unordered(100)
        .try_collect()
        .await
}

pub fn upload_git_tag<'a, Uploader: GitUploader, Reader: GitReader>(
    ctx: &'a CoreContext,
    uploader: Arc<Uploader>,
    reader: Arc<Reader>,
    tag_id: &'a ObjectId,
) -> BoxFuture<'a, Result<()>> {
    async move {
        if let Some((target_kind, target)) = reader
            .get_object(tag_id)
            .await
            .with_context(|| format_err!("Invalid object {:?}", tag_id))?
            .with_parsed_as_tag(|tag| (tag.target_kind, tag.target()))
        {
            upload_git_object(ctx, uploader.clone(), reader.clone(), tag_id).await?;
            // Note: If we support tags pointing to blobs and trees later, we'll need to upload the
            // appropriate git objects here too
            if target_kind == Kind::Tag {
                upload_git_tag(ctx, uploader.clone(), reader.clone(), &target).await?;
            }
        } else {
            bail!("Not a tag: {:?}", tag_id);
        }
        Ok(())
    }
    .boxed()
}

fn repo_name(prefs: &GitimportPreferences, path: &Path) -> String {
    if let Some(name) = &prefs.gitrepo_name {
        String::from(name)
    } else {
        let name_path = if path.ends_with(".git") {
            path.parent().unwrap_or(path)
        } else {
            path
        };
        String::from(name_path.to_string_lossy())
    }
}

pub async fn gitimport<Uploader: GitUploader>(
    ctx: &CoreContext,
    path: &Path,
    uploader: Arc<Uploader>,
    target: &GitimportTarget,
    prefs: &GitimportPreferences,
) -> Result<LinkedHashMap<ObjectId, ChangesetId>> {
    let repo_name = repo_name(prefs, path);
    let reader = Arc::new(
        GitRepoReader::new(&prefs.git_command_path, path)
            .await
            .context("GitRepoReader::new")?,
    );
    let acc = GitimportAccumulator::from_roots(target.get_roots().clone());
    let all_commits = target
        .list_commits(&prefs.git_command_path, path)
        .await
        .context("target.list_commits")?
        .into_iter()
        .collect::<Result<Vec<_>>>()
        .context(
            "Failure in converting Result<Vec<commits>> to Vec<commits> in target.list_commits",
        )?;

    if all_commits.is_empty() {
        info!(ctx.logger(), "Nothing to import for repo {}.", repo_name);
        return Ok(acc.into_inner());
    }

    import_commit_contents(ctx, repo_name, all_commits, uploader, reader, prefs, acc).await
}

pub async fn import_commit_contents<Uploader: GitUploader, Reader: GitReader>(
    ctx: &CoreContext,
    repo_name: String,
    all_commits: Vec<ObjectId>,
    uploader: Arc<Uploader>,
    reader: Arc<Reader>,
    prefs: &GitimportPreferences,
    acc: GitimportAccumulator,
) -> Result<LinkedHashMap<ObjectId, ChangesetId>> {
    let nb_commits_to_import = all_commits.len();
    let dry_run = prefs.dry_run;
    let backfill_derivation = prefs.backfill_derivation.clone();
    let acc = Arc::new(acc);
    let ctx = &ctx.clone_with_repo_name(&repo_name);
    let scuba = ctx.scuba().clone();
    // How many commits to query from bonsai git mapping per SQL query.
    const SQL_CONCURRENCY: usize = 10_000;
    let mappings: Vec<(ObjectId, ChangesetId)> = stream::iter(all_commits.clone())
        // Ignore any error. This is an optional optimization
        .chunks(SQL_CONCURRENCY)
        .map(|oids| {
            cloned!(uploader, ctx);
            async move {
                uploader
                    .preload_uploaded_commits(&ctx, oids.as_slice())
                    .await
                    .context("preload_uploaded_commits")
            }
        })
        .buffered(prefs.concurrency)
        .try_collect::<Vec<_>>()
        .try_timed()
        .await?
        .log_future_stats(
            scuba.clone(),
            "Prefetched existing BonsaiGit Mappings",
            "Import".to_string(),
        )
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    acc.extend(mappings);
    let n_existing_commits = acc.len();
    if n_existing_commits > 0 {
        info!(
            ctx.logger(),
            "GitRepo:{} {} of {} commit(s) already exist",
            repo_name,
            n_existing_commits,
            nb_commits_to_import,
        );
    }
    let count = Arc::new(AtomicUsize::new(n_existing_commits));
    // Filter out the commits that we have already synced in the past
    let relevant_commits = all_commits
        .into_iter()
        .filter(|oid| acc.get(oid).is_none())
        .collect::<Vec<_>>();
    // Create a channel to send the commits that have been converted into bonsais. Asynchronously, we will receive the commits
    // and call finalize batch on them (for deriving data) without blocking the import of commits
    let (finalize_sender, mut finalize_receiver) = mpsc::channel(prefs.concurrency);
    // Spawn off an async consumer that would finalize batches of commits which have been imported into Mononoke
    let batch_finalizer = mononoke::spawn_task({
        cloned!(
            backfill_derivation,
            ctx,
            acc,
            uploader,
            repo_name,
            count,
            scuba
        );
        async move {
            while let Some(incoming) = finalize_receiver.recv().await {
                cloned!(
                    backfill_derivation,
                    ctx,
                    acc,
                    uploader,
                    repo_name,
                    count,
                    mut scuba
                );
                async move {
                    let finalized_chunk_res = uploader
                        .finalize_batch(&ctx, dry_run, backfill_derivation, incoming, &acc)
                        .try_timed()
                        .await
                        .context("finalize_batch");
                    let finalized_chunk = match finalized_chunk_res {
                        Err(e) => {
                            // Log the error if any
                            info!(ctx.logger(), "{:?}", e);
                            anyhow::bail!(e);
                        }
                        Ok((stats, chunk)) => {
                            scuba.add_future_stats(&stats);
                            scuba.log_with_msg("Completed Finalize Batch", "Import".to_string());
                            chunk
                        }
                    };
                    let processed_count = finalized_chunk.len();
                    // Only log progress after every batch to avoid log-spew and wasted time
                    if let Some((last_git_sha1, last_bcs_id)) = finalized_chunk.last() {
                        count.fetch_add(processed_count, Ordering::Relaxed);
                        info!(
                            ctx.logger(),
                            "GitRepo:{} commit {} of {} - Oid:{} => Bid:{}",
                            &repo_name,
                            count.load(Ordering::Relaxed),
                            nb_commits_to_import,
                            last_git_sha1.to_brief(),
                            last_bcs_id.to_brief()
                        );
                    }
                    anyhow::Ok(())
                }
                .await?;
            }
            anyhow::Ok(())
        }
    });
    // Create a channel to send the commits that have had their Git data uploaded and file changes identified. Asynchronously, we will
    // receive these commits and derive bonsai for them without blocking the import pipeline
    let (bonsai_sender, mut bonsai_receiver) = mpsc::channel(prefs.concurrency);
    // Spawn off an async consumer that would generate bonsai commits for Git commits that have had their Git data and file changes uploaded
    // to Mononoke
    let bonsai_creator = mononoke::spawn_task({
        cloned!(ctx, uploader, acc, mut scuba);
        let concurrency = prefs.concurrency;
        async move {
            let mut batch_buffer = Vec::with_capacity(concurrency);
            while let Some((extracted_commit, file_changes)) = bonsai_receiver.recv().await {
                let extracted_commit: ExtractedCommit = extracted_commit;
                let oid = extracted_commit.metadata.oid;
                let int_cs_result = uploader
                    .generate_intermediate_changeset_for_commit(
                        &ctx,
                        extracted_commit.metadata,
                        file_changes,
                        &acc,
                        dry_run,
                    )
                    .try_timed()
                    .await
                    .context("generate_changeset_for_commit");
                let int_cs = match int_cs_result {
                    Err(e) => {
                        // Log the error if any
                        info!(ctx.logger(), "{:?}", e);
                        anyhow::bail!(e);
                    }
                    Ok((stats, int_cs)) => {
                        scuba.add_future_stats(&stats);
                        scuba.log_with_msg(
                            "Created Bonsai Changeset for Git Commit",
                            "Import".to_string(),
                        );
                        int_cs
                    }
                };
                let git_sha1 = oid_to_sha1(&oid)?;
                batch_buffer.push((git_sha1, int_cs));
                if batch_buffer.len() == concurrency {
                    // We have the required batch size of commits, send it for finalization
                    finalize_sender
                        .send(batch_buffer)
                        .await
                        .context("Receiver dropped while sending Vec<(bonsai_id, git_sha1)>")?;
                    batch_buffer = Vec::with_capacity(concurrency);
                }
            }
            // If there are still changesets pending to be processed, send them through
            if !batch_buffer.is_empty() {
                finalize_sender
                    .send(batch_buffer)
                    .await
                    .context("Receiver dropped while sending Vec<(bonsai_id, git_sha1)>")?;
            }
            // Drop the sender since we finished sending all the changesets to the finalizer
            drop(finalize_sender);
            anyhow::Ok(())
        }
    });
    // Kick off a stream that consumes the walk and prepared commits. Then, produce the Bonsais.
    let mut commits_with_file_changes = stream::iter(relevant_commits)
        .map(Ok)
        .map_ok(|oid| {
            cloned!(
                ctx,
                reader,
                uploader,
                prefs.lfs,
                prefs.submodules,
                prefs.stream_for_changed_trees
            );
            async move {
                mononoke::spawn_task({
                    async move {
                        let extracted_commit = ExtractedCommit::new(&ctx, oid, &reader)
                            .await
                            .with_context(|| format!("While extracting {}", oid))?;

                        let diff = extracted_commit.diff(&ctx, &reader, submodules);
                        let file_changes_uploader = uploader.clone();
                        let file_changes_future = async {
                            find_file_changes(
                                &ctx,
                                &lfs,
                                reader.clone(),
                                file_changes_uploader,
                                diff,
                            )
                            .await
                            .context("find_file_changes")
                        };
                        let oid = extracted_commit.metadata.oid;
                        // Before generating the corresponding changeset at Mononoke end, upload the raw git commit
                        // and the git tree pointed to by the git commit.
                        let entries_stream = if !stream_for_changed_trees {
                            stream::iter(
                                extracted_commit
                                    .changed_trees(&ctx, &reader)
                                    .try_collect::<Vec<_>>()
                                    .await?,
                            )
                            .map(Ok)
                            .boxed()
                        } else {
                            extracted_commit.changed_trees(&ctx, &reader).boxed()
                        };
                        let entries_uploader = uploader.clone();
                        let entries_future = async {
                            entries_stream
                                .map_ok(|entry| {
                                    cloned!(entries_uploader, reader, ctx);
                                    async move {
                                        mononoke::spawn_task(async move {
                                            upload_git_object(
                                                &ctx,
                                                entries_uploader,
                                                reader,
                                                &entry.0,
                                            )
                                            .await?;
                                            anyhow::Ok(())
                                        })
                                        .await?
                                    }
                                })
                                .try_buffer_unordered(100)
                                .try_collect::<()>()
                                .await
                        };
                        // Upload packfile base item for Git commit and the raw Git commit
                        let git_commit_future = async {
                            upload_git_object_by_content(
                                &ctx,
                                uploader,
                                &oid,
                                extracted_commit.original_commit.clone(),
                            )
                            .await
                        };
                        let (file_changes, _, _) =
                            try_join3(file_changes_future, entries_future, git_commit_future)
                                .await?;

                        Result::<_, Error>::Ok((extracted_commit, file_changes))
                    }
                })
                .await?
            }
        })
        .try_buffered(prefs.concurrency);
    async {
        while let Some((extracted_commit, file_changes)) = commits_with_file_changes
            .try_next()
            .try_timed()
            .await?
            .log_future_stats(
                scuba.clone(),
                "Uploaded Content Blob, Git Blob, Commits and Trees",
                "Import".to_string(),
            )
        {
            bonsai_sender
                .send((extracted_commit, file_changes))
                .await
                .context("Receiver dropped while sending Vec<(ExtractedCommit, FileChanges)>")?;
        }
        anyhow::Ok(())
    }
    .try_timed()
    .await?
    .log_future_stats(
        scuba.clone(),
        "Uploaded Content Blob, Git Blob, Commits and Trees for all commits",
        "Import".to_string(),
    );
    // Drop the sender since we finished sending all the commits to the bonsai creator
    drop(bonsai_sender);
    // Ensure that the bonsai creator has completed before we exit
    bonsai_creator
        .try_timed()
        .await
        .context("Error while running bonsai_creator for commits")?
        .log_future_stats(
            scuba.clone(),
            "Completed Bonsai Changeset creation for all commits",
            "Import".to_string(),
        )
        .context("Panic while running bonsai_creator for commits")?;
    // Ensure that the batch finalization has completed before we exit
    batch_finalizer
        .try_timed()
        .await
        .context("Error while running finalize_batch for commits")?
        .log_future_stats(
            scuba.clone(),
            "Completed Finalize Batch for all commits",
            "Import".to_string(),
        )
        .context("Panic while running finalize_batch for commits")?;

    debug!(ctx.logger(), "Completed git import for repo {}.", repo_name);
    let acc = Arc::try_unwrap(acc).map_err(|_| {
        anyhow::anyhow!("Expected only one strong reference to GitimportAccumulator at this point")
    })?;
    Ok(acc.into_inner())
}

/// Enum for the different types of targets for a Git ref
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub enum RefTarget {
    #[default]
    Commit,
    Blob(ObjectId),
    Tree(ObjectId),
}

impl RefTarget {
    pub fn is_content(&self) -> bool {
        match self {
            RefTarget::Blob(_) | RefTarget::Tree(_) => true,
            _ => false,
        }
    }

    pub fn is_tree(&self) -> bool {
        match self {
            RefTarget::Tree(_) => true,
            _ => false,
        }
    }

    pub fn into_inner_hash(self) -> Result<ObjectId> {
        match self {
            RefTarget::Blob(oid) | RefTarget::Tree(oid) => Ok(oid),
            _ => anyhow::bail!("RefTarget is not a content type and hence has no hash"),
        }
    }
}

/// Object representing the metadata associated with Git refs
#[derive(Clone, Copy, Debug, Default, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct GitRefMetadata {
    /// Object id of the tag if the ref is a tag
    pub maybe_tag_id: Option<ObjectId>,
    /// Flag indicating if the ref is a nested ref. Applies only for tags
    pub nested_tag: bool,
    /// The type of object that the ref points to
    pub target: RefTarget,
}

/// Object representing Git refs
#[derive(Debug, Hash, PartialEq, Eq, PartialOrd, Ord, Clone)]
pub struct GitRef {
    /// Name of the ref
    pub name: Vec<u8>,
    /// Metadata associated with the ref
    pub metadata: GitRefMetadata,
}

impl GitRef {
    fn new(name: Vec<u8>) -> Self {
        Self {
            name,
            metadata: GitRefMetadata::default(),
        }
    }
}

/// Read symbolic references from git
pub async fn read_symref(
    symref_name: &str,
    path: &Path,
    prefs: &GitimportPreferences,
) -> Result<GitSymbolicRefsEntry> {
    let mut command = Command::new(&prefs.git_command_path)
        .current_dir(path)
        .env_clear()
        .kill_on_drop(false)
        .stdout(Stdio::piped())
        .arg("symbolic-ref")
        .arg(symref_name)
        .spawn()
        .with_context(|| format!("failed to run git with {:?}", prefs.git_command_path))?;
    let mut stdout = BufReader::new(command.stdout.take().context("stdout not set up")?);
    let mut ref_mapping = String::new();
    stdout.read_line(&mut ref_mapping).await.with_context(|| {
        format!(
            "failed to get output of git symbolic-ref for ref {} at path {}",
            symref_name,
            path.display()
        )
    })?;
    let ref_mapping = ref_mapping.trim();
    let symref_entry = match ref_mapping.strip_prefix(BRANCH_REF_PREFIX) {
        Some(branch_name) => GitSymbolicRefsEntry::new(
            symref_name.to_string(),
            branch_name.to_string(),
            BRANCH_REF.to_string(),
        )?,
        None => match ref_mapping.strip_prefix(TAG_REF_PREFIX) {
            Some(tag_name) => GitSymbolicRefsEntry::new(
                symref_name.to_string(),
                tag_name.to_string(),
                TAG_REF.to_string(),
            )?,
            None => anyhow::bail!(
                "Unexpected ref format {} for symref {}",
                ref_mapping,
                symref_name
            ),
        },
    };
    Ok(symref_entry)
}

/// Resolve git rev using `git rev-parse --verify`
pub async fn resolve_rev(
    rev: &str,
    path: &Path,
    prefs: &GitimportPreferences,
) -> Result<Option<ObjectId>> {
    let output = Command::new(&prefs.git_command_path)
        .current_dir(path)
        .env_clear()
        .kill_on_drop(false)
        .stdout(Stdio::piped())
        .arg("rev-parse")
        .arg("--verify")
        .arg("--end-of-options")
        .arg(rev)
        .output()
        .await
        .with_context(|| format!("failed to run git with {:?}", prefs.git_command_path))?;
    if !output.status.success() {
        return Ok(None);
    }
    let oid_str = str::from_utf8(&output.stdout)?;
    let oid_str = oid_str.trim();
    let oid: ObjectId = oid_str.parse().context("reading refs")?;
    Ok(Some(oid))
}

pub async fn read_git_refs(
    path: &Path,
    prefs: &GitimportPreferences,
) -> Result<BTreeMap<GitRef, ObjectId>> {
    let reader = GitRepoReader::new(&prefs.git_command_path, path).await?;

    let mut command = Command::new(&prefs.git_command_path)
        .current_dir(path)
        .env_clear()
        .kill_on_drop(false)
        .stdout(Stdio::piped())
        .arg("for-each-ref")
        .arg("--format=%(objectname) %(refname)")
        .spawn()
        .with_context(|| format!("failed to run git with {:?}", prefs.git_command_path))?;
    let stdout = BufReader::new(command.stdout.take().context("stdout not set up")?);
    let mut lines = stdout.lines();

    let mut refs = BTreeMap::new();

    while let Some(line) = lines
        .next_line()
        .await
        .context("git command didn't output anything")?
    {
        if let Some((oid_str, ref_name)) = line.split_once(' ') {
            let mut oid: ObjectId = oid_str.parse().context("reading refs")?;
            let mut git_ref = GitRef::new(ref_name.into());
            loop {
                let object = reader.get_object(&oid).await.with_context(|| {
                    format!("unable to read git object: {oid} for ref: {ref_name}")
                })?;
                let should_break = object.with_parsed(|parsed| {
                    let mut should_break = false;
                    match parsed {
                        ObjectRef::Commit(_) => {
                            git_ref.metadata.target = RefTarget::Commit;
                            refs.insert(git_ref.clone(), oid);
                            should_break = true;
                        }
                        ObjectRef::Blob(_) => {
                            if !prefs.allow_content_refs {
                                anyhow::bail!("Ref: {ref_name} points to a blob");
                            }
                            git_ref.metadata.target = RefTarget::Blob(oid);
                            refs.insert(git_ref.clone(), oid);
                            should_break = true;
                        }
                        ObjectRef::Tree(_) => {
                            if !prefs.allow_content_refs {
                                anyhow::bail!("Ref: {ref_name} points to a tree");
                            }
                            git_ref.metadata.target = RefTarget::Tree(oid);
                            refs.insert(git_ref.clone(), oid);
                            should_break = true;
                        }
                        // If the ref is a tag, then we capture the object id of the tag.
                        // The loop is designed to peel the tag but we want the outermost
                        // tag object so only get the ID if we haven't already done it before.
                        ObjectRef::Tag(tag) => {
                            if git_ref.metadata.maybe_tag_id.is_none() {
                                git_ref.metadata.maybe_tag_id = Some(oid);
                            } else {
                                // We have already captured the tag id, which means this is at least the
                                // second tag in the chain. We want to mark this as a nested tag.
                                git_ref.metadata.nested_tag = true;
                            }
                            oid = tag.target();
                        }
                    }
                    Ok(should_break)
                })?;
                if should_break {
                    break;
                }
            }
        }
    }
    Ok(refs)
}

pub async fn import_tree_as_single_bonsai_changeset<Uploader: GitUploader>(
    ctx: &CoreContext,
    path: &Path,
    uploader: Arc<Uploader>,
    git_cs_id: ObjectId,
    prefs: &GitimportPreferences,
) -> Result<ChangesetId> {
    let acc = GitimportAccumulator::from_roots(HashMap::new());
    let reader = Arc::new(GitRepoReader::new(&prefs.git_command_path, path).await?);

    let sha1 = oid_to_sha1(&git_cs_id)?;

    let mut extracted_commit = ExtractedCommit::new(ctx, git_cs_id, &reader)
        .await
        .with_context(|| format!("While extracting {}", git_cs_id))?;
    // Discard the parents: the commit we want to create has no parents
    extracted_commit.metadata.parents = Vec::new();

    let diff = extracted_commit.diff_root(ctx, &reader, prefs.submodules);
    let file_changes =
        find_file_changes(ctx, &prefs.lfs, reader.clone(), uploader.clone(), diff).await?;

    // Before generating the corresponding changeset at Mononoke end, upload the raw git commit and raw git tree.
    upload_git_object_by_content(
        ctx,
        uploader.clone(),
        &git_cs_id,
        extracted_commit.original_commit.clone(),
    )
    .await
    .with_context(|| format_err!("Failed to upload raw git commit {}", git_cs_id))?;

    upload_git_tree_recursively(
        ctx,
        uploader.clone(),
        reader.clone(),
        &extracted_commit.tree_oid,
    )
    .await
    .with_context(|| {
        format_err!(
            "Failed to upload git trees corresponding to commit {}",
            git_cs_id
        )
    })?;

    uploader
        .generate_intermediate_changeset_for_commit(
            ctx,
            extracted_commit.metadata,
            file_changes,
            &acc,
            prefs.dry_run,
        )
        .and_then(|int_cs| {
            uploader
                .finalize_batch(
                    ctx,
                    prefs.dry_run,
                    prefs.backfill_derivation.clone(),
                    vec![(sha1, int_cs)],
                    &acc,
                )
                .map_ok(|batch| {
                    batch
                        .into_iter()
                        .next_back()
                        .expect("Finalize batch should produce a changeset for each sha1")
                        .1
                })
        })
        .await
}
