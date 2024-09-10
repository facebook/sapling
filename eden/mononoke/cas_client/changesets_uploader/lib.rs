/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![feature(trait_alias)]

mod errors;

use std::fmt::Display;
use std::sync::Arc;

use atomic_counter::AtomicCounter;
use atomic_counter::RelaxedCounter;
use blobstore::Loadable;
use bonsai_hg_mapping::BonsaiHgMappingRef;
use bytesize::ByteSize;
use cas_client::CasClient;
use cloned::cloned;
use context::CoreContext;
pub use errors::CasChangesetUploaderErrorKind;
use futures::future;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use futures::try_join;
use futures::FutureExt;
use futures_watchdog::WatchdogExt;
use manifest::Diff;
use manifest::Entry;
use manifest::Manifest;
use manifest::ManifestOps;
use mercurial_types::HgAugmentedManifestId;
use mercurial_types::HgChangesetId;
use mercurial_types::HgManifestId;
use mononoke_types::ChangesetId;
use mononoke_types::MPath;
use repo_blobstore::RepoBlobstoreArc;
use scm_client::ScmCasClient;
use scm_client::UploadOutcome;
use slog::debug;
use stats::prelude::*;

const MAX_CONCURRENT_MANIFESTS: usize = 50;
const MAX_CONCURRENT_MANIFESTS_TREES_ONLY: usize = 500;
const MAX_CONCURRENT_FILES_PER_MANIFEST: usize = 20;
const DEBUG_LOG_INTERVAL: usize = 10000;

#[derive(Default, Debug)]
pub struct UploadCounters {
    uploaded: RelaxedCounter,
    uploaded_bytes: RelaxedCounter,
    already_present: RelaxedCounter,
    largest_uploaded_blob_bytes: RelaxedCounter,
}

#[derive(Debug, PartialEq, Clone)]
pub enum UploadPolicy {
    All,
    BlobsOnly,
    TreesOnly,
}

#[derive(Debug, PartialEq, Clone)]
pub enum PriorLookupPolicy {
    All,
    BlobsOnly,
    TreesOnly,
    None,
}

impl PriorLookupPolicy {
    pub fn enabled_trees(&self) -> bool {
        matches!(self, PriorLookupPolicy::TreesOnly) || matches!(self, PriorLookupPolicy::All)
    }
    pub fn enabled_blobs(&self) -> bool {
        matches!(self, PriorLookupPolicy::BlobsOnly) || matches!(self, PriorLookupPolicy::All)
    }
}

impl Display for UploadCounters {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> Result<(), std::fmt::Error> {
        write!(
            f,
            "uploaded digests: {}, already present digests: {}, uploaded bytes: {}, the largest uploaded blob: {}",
            self.uploaded.get(),
            self.already_present.get(),
            ByteSize::b(self.uploaded_bytes.get() as u64).to_string_as(true),
            ByteSize::b(self.largest_uploaded_blob_bytes.get() as u64).to_string_as(true)
        )
    }
}

pub type UploadStats = Arc<UploadCounters>;

impl UploadCounters {
    pub fn sum(&self) -> usize {
        self.uploaded.get() + self.already_present.get()
    }

    pub fn add(&self, other: &UploadCounters) {
        self.uploaded.add(other.uploaded.get());
        self.already_present.add(other.already_present.get());
        self.uploaded_bytes.add(other.uploaded_bytes.get());
        if self.largest_uploaded_blob_bytes.get() < other.largest_uploaded_blob_bytes.get() {
            self.largest_uploaded_blob_bytes.add(
                other.largest_uploaded_blob_bytes.get() - self.largest_uploaded_blob_bytes.get(),
            );
        }
    }

    pub fn tick(&self, ctx: &CoreContext, outcome: UploadOutcome) {
        match outcome {
            UploadOutcome::Uploaded(size) => {
                let size = size as usize;
                self.uploaded.inc();
                self.uploaded_bytes.add(size);
                if self.largest_uploaded_blob_bytes.get() < size {
                    self.largest_uploaded_blob_bytes
                        .add(size - self.largest_uploaded_blob_bytes.get());
                }
            }
            UploadOutcome::AlreadyPresent => {
                self.already_present.inc();
            }
        }
        self.maybe_log(ctx, DEBUG_LOG_INTERVAL);
    }

    pub fn maybe_log<'a>(&self, ctx: &'a CoreContext, limit: usize) {
        if self.sum() % limit == 0 {
            self.log(ctx);
        }
    }
    pub fn log<'a>(&self, ctx: &'a CoreContext) {
        debug!(ctx.logger(), "{}", *self);
    }
}

define_stats! {
    prefix = "mononoke.cas_changesets_uploader";
    uploaded_changesets: timeseries(Rate, Sum),
    uploaded_changesets_recursive: timeseries(Rate, Sum),
}

pub trait Repo = BonsaiHgMappingRef + RepoBlobstoreArc + Send + Sync;

pub struct CasChangesetsUploader<Client>
where
    Client: CasClient,
{
    pub client: ScmCasClient<Client>,
}

impl<Client> CasChangesetsUploader<Client>
where
    Client: CasClient,
{
    pub fn new(client: Client) -> Self {
        Self {
            client: ScmCasClient::new(client),
        }
    }

    async fn get_manifest_id_from_changeset<'a>(
        &self,
        ctx: &'a CoreContext,
        repo: &impl Repo,
        changeset_id: &ChangesetId,
    ) -> Result<(HgChangesetId, HgManifestId), CasChangesetUploaderErrorKind> {
        let hg_cs_id = repo
            .bonsai_hg_mapping()
            .get_hg_from_bonsai(ctx, *changeset_id)
            .await
            .map(|cs| {
                cs.ok_or(CasChangesetUploaderErrorKind::InvalidChangeset(
                    changeset_id.clone(),
                ))
            })??;
        let hg_manifest = hg_cs_id
            .load(ctx, &repo.repo_blobstore())
            .await
            .map_err(|e| CasChangesetUploaderErrorKind::Error(e.into()))?
            .manifestid();
        Ok((hg_cs_id, hg_manifest))
    }

    /// Upload a given Changeset to a CAS backend.
    /// The implementation assumes that if hg manifest is derived, then augmented manifest is also derived.
    /// The current implementation is based on the diffing of the hg manifests, rather than hg augmented manifests,
    /// but it is a good starting point, and the result of the diff will be identical.
    /// We will switch over to diff the augmented manifests once we have them, since we can have the digests strait away to pass to the uploads.
    pub async fn upload_single_changeset<'a>(
        &self,
        ctx: &'a CoreContext,
        repo: &impl Repo,
        changeset_id: &ChangesetId,
        upload_policy: UploadPolicy,
        prior_lookup_policy: PriorLookupPolicy,
    ) -> Result<UploadStats, CasChangesetUploaderErrorKind> {
        let hg_cs_id = repo
            .bonsai_hg_mapping()
            .get_hg_from_bonsai(ctx, *changeset_id)
            .await
            .map(|cs| {
                cs.ok_or(CasChangesetUploaderErrorKind::InvalidChangeset(
                    changeset_id.clone(),
                ))
            })??;

        let blobstore = repo.repo_blobstore_arc();
        let hg_cs = hg_cs_id
            .load(ctx, &blobstore)
            .await
            .map_err(|e| CasChangesetUploaderErrorKind::Error(e.into()))?;

        let hg_root_manifest_id = HgAugmentedManifestId::new(hg_cs.manifestid().into_nodehash());

        // Diff hg manifest with parents
        let diff_stream = match (
            hg_cs.p1().map(HgChangesetId::new),
            hg_cs.p2().map(HgChangesetId::new),
        ) {
            // If there is a merge commit, there is no guarantee that the p2 parent is uploaded to CAS
            // (since the sync follows p1 chain)
            // Therefore, we need to diff fully with the p1 parent only.
            (Some(p), None) | (None, Some(p)) | (Some(p), Some(_)) => {
                let blobstore = repo.repo_blobstore();
                let parent_hg_cs = p.load(ctx, &blobstore);
                let hg_cs = hg_cs_id.load(ctx, &blobstore);
                let (parent_hg_cs, hg_cs) = try_join!(parent_hg_cs, hg_cs)
                    .map_err(|e| CasChangesetUploaderErrorKind::Error(e.into()))?;

                parent_hg_cs
                    .manifestid()
                    .diff(ctx.clone(), repo.repo_blobstore_arc(), hg_cs.manifestid())
                    .map_ok(move |diff| match diff {
                        Diff::Added(_, entry) => Some(entry),
                        Diff::Removed(..) => None,
                        Diff::Changed(_, _, entry) => Some(entry),
                    })
                    .try_filter_map(future::ok)
                    .map_err(CasChangesetUploaderErrorKind::DiffChangesetFailed)
                    .boxed()
            }

            (None, None) => {
                let hg_cs = hg_cs_id
                    .load(ctx, &repo.repo_blobstore())
                    .await
                    .map_err(|e| CasChangesetUploaderErrorKind::Error(e.into()))?;
                hg_cs
                    .manifestid()
                    .list_all_entries(ctx.clone(), repo.repo_blobstore_arc())
                    .map_ok(move |(_, entry)| entry)
                    .map_err(CasChangesetUploaderErrorKind::DiffChangesetFailed)
                    .boxed()
            }
        }
        .try_collect::<Vec<_>>()
        .watched(ctx.logger())
        .await?;

        let manifests_list = diff_stream
            .iter()
            .filter_map(|elem| {
                if let Entry::Tree(treeid) = elem {
                    Some(treeid)
                } else {
                    None
                }
            })
            .map(|treeid| (HgAugmentedManifestId::new(treeid.into_nodehash()), None))
            .collect::<Vec<_>>();

        let files_list = diff_stream
            .iter()
            .filter_map(|elem| {
                if let Entry::Leaf(leaf) = elem {
                    Some(leaf.1)
                } else {
                    None
                }
            })
            .map(|fileid| (fileid, None))
            .collect::<Vec<_>>();

        let start_time = std::time::Instant::now();
        let blobstore = repo.repo_blobstore_arc();

        debug!(
            ctx.logger(),
            "Uploading data for changeset id: {}, hg changeset id: {}, number of manifests: {}, number of files: {}",
            changeset_id,
            hg_cs_id,
            manifests_list.len(),
            files_list.len(),
        );

        let upload_counter: Arc<UploadCounters> = Arc::new(Default::default());

        let blobs_lookup = prior_lookup_policy.enabled_blobs();
        let trees_lookup = prior_lookup_policy.enabled_trees();

        match upload_policy {
            UploadPolicy::BlobsOnly => {
                self.client
                    .ensure_upload_file_contents(ctx, &blobstore, files_list, blobs_lookup)
                    .watched(ctx.logger())
                    .await?
                    .into_iter()
                    .for_each(|(_, outcome)| {
                        upload_counter.tick(ctx, outcome);
                    });
            }
            UploadPolicy::TreesOnly => {
                self.client
                    .ensure_upload_augmented_trees(ctx, &blobstore, manifests_list, trees_lookup)
                    .watched(ctx.logger())
                    .await?
                    .into_iter()
                    .for_each(|(_, outcome)| {
                        upload_counter.tick(ctx, outcome);
                    });
            }
            UploadPolicy::All => {
                // Ensure the root manifest is uploaded last, so that we can use it to derive if entire changeset is already present in CAS
                let (outcomes_trees, outcomes_files) = try_join!(
                    self.client
                        .ensure_upload_augmented_trees(
                            ctx,
                            &blobstore,
                            manifests_list
                                .into_iter()
                                .filter(|(treeid, _)| *treeid != hg_root_manifest_id),
                            trees_lookup
                        )
                        .watched(ctx.logger()),
                    self.client
                        .ensure_upload_file_contents(ctx, &blobstore, files_list, blobs_lookup)
                        .watched(ctx.logger())
                )?;

                outcomes_trees.into_iter().for_each(|(_, outcome)| {
                    upload_counter.tick(ctx, outcome);
                });

                outcomes_files.into_iter().for_each(|(_, outcome)| {
                    upload_counter.tick(ctx, outcome);
                });

                // Upload the root manifest last
                let outcome_root = self
                    .client
                    .upload_augmented_tree(
                        ctx,
                        &blobstore,
                        &hg_root_manifest_id,
                        None,
                        trees_lookup,
                    )
                    .watched(ctx.logger())
                    .await?;
                upload_counter.tick(ctx, outcome_root);
            }
        }

        debug!(
            ctx.logger(),
            "Upload of (bonsai) changeset {} to CAS took {} seconds, corresponding hg changeset is {}",
            changeset_id,
            start_time.elapsed().as_secs_f64(),
            hg_cs_id,
        );

        STATS::uploaded_changesets.add_value(1);
        Ok(upload_counter)
    }

    /// Upload a given Changeset to a CAS backend recursively.
    /// Upload can be limited to a specific path if provided.
    /// The implementation assumes that if hg manifest is derived, then augmented manifest is also derived.
    pub async fn upload_single_changeset_recursively<'a>(
        &self,
        ctx: &'a CoreContext,
        repo: &impl Repo,
        changeset_id: &ChangesetId,
        path: Option<MPath>,
        upload_policy: UploadPolicy,
        prior_lookup_policy: PriorLookupPolicy,
    ) -> Result<UploadStats, CasChangesetUploaderErrorKind> {
        let start_time = std::time::Instant::now();
        let upload_counter: Arc<UploadCounters> = Arc::new(Default::default());
        let final_upload_counter = upload_counter.clone();
        let (hg_cs_id, hg_manifest_id) = self
            .get_manifest_id_from_changeset(ctx, repo, changeset_id)
            .await?;

        let hg_augmented_manifest_id: HgAugmentedManifestId =
            HgAugmentedManifestId::new(hg_manifest_id.into_nodehash());

        // We will traverse over Mercurial manifests for now, as augmented manifests haven't been
        // derived yet and don't yet implement Manifest.  This will be trivial to swap in later.
        let max_concurrent_manifests = if matches!(upload_policy, UploadPolicy::TreesOnly) {
            MAX_CONCURRENT_MANIFESTS_TREES_ONLY
        } else {
            MAX_CONCURRENT_MANIFESTS
        };

        let blobs_lookup = prior_lookup_policy.enabled_blobs();
        let trees_lookup = prior_lookup_policy.enabled_trees();

        let mut hg_manifest_id_start = hg_manifest_id;

        if let Some(ref path) = path {
            let path = path.clone();
            let blobstore = repo.repo_blobstore_arc();
            let entry = hg_manifest_id
                .find_entry(ctx.clone(), blobstore, path.clone())
                .await?;
            match entry {
                Some(entry) => {
                    match entry {
                        // adjust the starting point for the traversal below
                        // to start with this entry instead of the root manifest
                        Entry::Tree(treeid) => {
                            hg_manifest_id_start = treeid;
                        }
                        // upload just this single file and exit
                        Entry::Leaf(leaf) => {
                            if !matches!(upload_policy, UploadPolicy::TreesOnly) {
                                debug!(
                                    ctx.logger(),
                                    "Upload single file '{}' for changeset id: {}, hg changeset id: {}",
                                    path.clone(),
                                    changeset_id,
                                    hg_cs_id,
                                );
                                let outcome = self
                                    .client
                                    .upload_file_content(
                                        ctx,
                                        repo.repo_blobstore(),
                                        &leaf.1,
                                        None,
                                        blobs_lookup,
                                    )
                                    .await
                                    .map_err(|error| {
                                        CasChangesetUploaderErrorKind::FileUploadFailedWithFullPath(
                                            leaf.1,
                                            path.clone(),
                                            error,
                                        )
                                    })?;
                                upload_counter.tick(ctx, outcome);
                                debug!(
                                    ctx.logger(),
                                    "Upload completed for '{}' in {:.2} seconds",
                                    path,
                                    start_time.elapsed().as_secs_f64()
                                );
                            }
                            return Ok(upload_counter);
                        }
                    }
                }
                None => {
                    return Err(CasChangesetUploaderErrorKind::PathNotFound(path));
                }
            }
        }

        debug!(
            ctx.logger(),
            "Uploading data recursively for [root augmented manifest: {}, changeset id: {}, hg changeset id: {}, repo path: '{}']",
            hg_augmented_manifest_id,
            changeset_id,
            hg_cs_id,
            path.clone().unwrap_or(MPath::ROOT),
        );

        bounded_traversal::bounded_traversal_stream(
            max_concurrent_manifests,
            Some(hg_manifest_id_start),
            |hg_manifest_id| {
                cloned!(upload_counter);
                let upload_policy = upload_policy.clone();
                async move {
                    if !matches!(upload_policy, UploadPolicy::BlobsOnly) {
                        let hg_augmented_manifest_id: HgAugmentedManifestId =
                            HgAugmentedManifestId::new(hg_manifest_id.into_nodehash());
                        let outcome = self
                            .client
                            .upload_augmented_tree(
                                ctx,
                                repo.repo_blobstore(),
                                &hg_augmented_manifest_id,
                                None,
                                trees_lookup,
                            )
                            .await
                            .map_err(|error| {
                                CasChangesetUploaderErrorKind::TreeUploadFailed(
                                    hg_augmented_manifest_id,
                                    error,
                                )
                            })?;
                        upload_counter.tick(ctx, outcome);
                    }
                    let hg_manifest = hg_manifest_id.load(ctx, repo.repo_blobstore()).await?;
                    let mut children = Vec::new();
                    hg_manifest
                        .list(ctx, repo.repo_blobstore())
                        .await?
                        .try_for_each_concurrent(
                            MAX_CONCURRENT_FILES_PER_MANIFEST,
                            |(elem, entry)| match entry {
                                Entry::Tree(tree) => {
                                    children.push(tree);
                                    future::ok(()).left_future()
                                }
                                Entry::Leaf(leaf) => {
                                    cloned!(upload_counter);
                                    let upload_policy = upload_policy.clone();
                                    async move {
                                        if !matches!(upload_policy, UploadPolicy::TreesOnly) {
                                            let outcome = self
                                                .client
                                                .upload_file_content(
                                                    ctx,
                                                    repo.repo_blobstore(),
                                                    &leaf.1,
                                                    None,
                                                    blobs_lookup,
                                                )
                                                .await
                                                .map_err(|error| {
                                                    CasChangesetUploaderErrorKind::FileUploadFailed(
                                                        leaf.1, elem, error,
                                                    )
                                                })?;
                                            upload_counter.tick(ctx, outcome);
                                        }
                                        Ok(())
                                    }
                                    .right_future()
                                }
                            },
                        )
                        .await?;
                    anyhow::Ok(((), children))
                }
                .boxed()
            },
        )
        .try_collect::<Vec<()>>()
        .await?;

        final_upload_counter.log(ctx);
        debug!(
            ctx.logger(),
            "Upload of (bonsai) changeset {} to CAS (recursively) for path: '{}' took {:.2} seconds, corresponding hg changeset is {}. Upload included {}.",
            changeset_id,
            path.clone().unwrap_or(MPath::ROOT),
            start_time.elapsed().as_secs_f64(),
            hg_cs_id,
            match upload_policy {
                UploadPolicy::BlobsOnly => "blobs only",
                UploadPolicy::TreesOnly => "augmented trees only",
                UploadPolicy::All => "augmented trees and blobs",
            }
        );
        STATS::uploaded_changesets_recursive.add_value(1);
        Ok(final_upload_counter)
    }

    /// Check if a given Changeset is already uploaded to a CAS backend by validating the presence of the hg root augmented manifest.
    /// This is not full scan check but the best approximation we can do.
    /// The lookup shouldn't be relied on if a changeset was uploaded with TreeOnly policy, the mode that isn't used in production.
    pub async fn is_changeset_uploaded<'a>(
        &self,
        ctx: &'a CoreContext,
        repo: &impl Repo,
        changeset_id: &ChangesetId,
    ) -> Result<bool, CasChangesetUploaderErrorKind> {
        let (_, hg_root_manifest_id) = self
            .get_manifest_id_from_changeset(ctx, repo, changeset_id)
            .await?;

        let hg_root_augmented_manifest_id: HgAugmentedManifestId =
            HgAugmentedManifestId::new(hg_root_manifest_id.into_nodehash());

        self.client
            .is_augmented_tree_uploaded(ctx, repo.repo_blobstore(), &hg_root_augmented_manifest_id)
            .await
            .map_err(CasChangesetUploaderErrorKind::Error)
    }
}
