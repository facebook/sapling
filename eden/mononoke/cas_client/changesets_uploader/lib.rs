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
use cas_client::CasClient;
use changesets::ChangesetsRef;
use cloned::cloned;
use context::CoreContext;
pub use errors::CasChangesetUploaderErrorKind;
use futures::future;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use futures::try_join;
use futures::FutureExt;
use manifest::find_intersection_of_diffs;
use manifest::AsyncManifest;
use manifest::Diff;
use manifest::Entry;
use manifest::ManifestOps;
use mercurial_types::HgAugmentedManifestId;
use mercurial_types::HgChangesetId;
use mercurial_types::HgManifestId;
use mononoke_types::ChangesetId;
use repo_blobstore::RepoBlobstoreArc;
use scm_client::ScmCasClient;
use scm_client::UploadOutcome;
use slog::debug;
use stats::prelude::*;

const MAX_CONCURRENT_MANIFESTS: usize = 100;
const MAX_CONCURRENT_MANIFESTS_TREES_ONLY: usize = 500;
const MAX_CONCURRENT_FILES_PER_MANIFEST: usize = 20;
const DEBUG_LOG_INTERVAL: usize = 10000;

#[derive(Default, Debug)]
struct DebugUploadCounters {
    uploaded: RelaxedCounter,
    already_present: RelaxedCounter,
}

#[derive(Debug, PartialEq, Clone)]
pub enum UploadPolicy {
    All,
    BlobsOnly,
    TreesOnly,
}

impl Display for DebugUploadCounters {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> Result<(), std::fmt::Error> {
        write!(
            f,
            "Uploaded: {}, Already present: {}",
            self.uploaded.get(),
            self.already_present.get()
        )
    }
}

impl DebugUploadCounters {
    pub fn sum(&self) -> usize {
        self.uploaded.get() + self.already_present.get()
    }

    pub fn tick(&self, ctx: &CoreContext, outcome: UploadOutcome) {
        match outcome {
            UploadOutcome::Uploaded => {
                self.uploaded.inc();
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

pub trait Repo = BonsaiHgMappingRef + ChangesetsRef + RepoBlobstoreArc + Send + Sync;

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
    ) -> Result<(), CasChangesetUploaderErrorKind> {
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

        // Diff hg manifest with parents
        let diff_stream = match (
            hg_cs.p1().map(HgChangesetId::new),
            hg_cs.p2().map(HgChangesetId::new),
        ) {
            (Some(p1), Some(p2)) => {
                let p1_hg_cs = p1.load(ctx, &blobstore);
                let p2_hg_cs = p2.load(ctx, &blobstore);
                let hg_cs = hg_cs_id.load(ctx, &blobstore);
                let (p1_hg_cs, p2_hg_cs, hg_cs) = future::try_join3(p1_hg_cs, p2_hg_cs, hg_cs)
                    .await
                    .map_err(|e| CasChangesetUploaderErrorKind::Error(e.into()))?;

                find_intersection_of_diffs(
                    ctx.clone(),
                    blobstore,
                    hg_cs.manifestid(),
                    vec![p1_hg_cs.manifestid(), p2_hg_cs.manifestid()],
                )
                .map_ok(move |(_, entry)| entry)
                .map_err(CasChangesetUploaderErrorKind::DiffChangesetFailed)
                .boxed()
            }

            (Some(p), None) | (None, Some(p)) => {
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

        match upload_policy {
            UploadPolicy::BlobsOnly => {
                self.client
                    .ensure_upload_file_contents(ctx, &blobstore, files_list, true)
                    .await?;
            }
            UploadPolicy::TreesOnly => {
                self.client
                    .ensure_upload_augmented_trees(ctx, &blobstore, manifests_list, true)
                    .await?;
            }
            UploadPolicy::All => {
                try_join!(
                    self.client.ensure_upload_augmented_trees(
                        ctx,
                        &blobstore,
                        manifests_list,
                        true
                    ),
                    self.client
                        .ensure_upload_file_contents(ctx, &blobstore, files_list, true)
                )?;
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
        Ok(())
    }

    /// Upload a given Changeset to a CAS backend recursively.
    /// The implementation assumes that if hg manifest is derived, then augmented manifest is also derived.
    pub async fn upload_single_changeset_recursively<'a>(
        &self,
        ctx: &'a CoreContext,
        repo: &impl Repo,
        changeset_id: &ChangesetId,
        upload_policy: UploadPolicy,
    ) -> Result<(), CasChangesetUploaderErrorKind> {
        let start_time = std::time::Instant::now();
        let progress_counter: Arc<DebugUploadCounters> = Arc::new(Default::default());
        let final_progress_counter = progress_counter.clone();
        let (hg_cs_id, hg_manifest_id) = self
            .get_manifest_id_from_changeset(ctx, repo, changeset_id)
            .await?;

        let hg_augmented_manifest_id: HgAugmentedManifestId =
            HgAugmentedManifestId::new(hg_manifest_id.into_nodehash());
        debug!(
            ctx.logger(),
            "Uploading data recursively for [root augmented manifest: {}, changeset id: {}, hg changeset id: {}]",
            hg_augmented_manifest_id,
            changeset_id,
            hg_cs_id
        );

        // We will traverse over Mercurial manifests for now, as augmented manifests haven't been
        // derived yet and don't yet implement AsyncManifest.  This will be trivial to swap in later.
        let max_concurrent_manifests = if matches!(upload_policy, UploadPolicy::TreesOnly) {
            MAX_CONCURRENT_MANIFESTS_TREES_ONLY
        } else {
            MAX_CONCURRENT_MANIFESTS
        };
        bounded_traversal::bounded_traversal_stream(
            max_concurrent_manifests,
            Some(hg_manifest_id),
            |hg_manifest_id| {
                cloned!(progress_counter);
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
                                true,
                            )
                            .await
                            .map_err(|error| {
                                CasChangesetUploaderErrorKind::TreeUploadFailed(
                                    hg_augmented_manifest_id,
                                    error,
                                )
                            })?;
                        progress_counter.tick(ctx, outcome);
                    }
                    //}
                    let hg_manifest = hg_manifest_id.load(ctx, repo.repo_blobstore()).await?;
                    let mut children = Vec::new();
                    hg_manifest
                        .list(ctx, repo.repo_blobstore())
                        .await?
                        .try_for_each_concurrent(
                            MAX_CONCURRENT_FILES_PER_MANIFEST,
                            |(_elem, entry)| match entry {
                                Entry::Tree(tree) => {
                                    children.push(tree);
                                    future::ok(()).left_future()
                                }
                                Entry::Leaf(leaf) => {
                                    cloned!(progress_counter);
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
                                                    true,
                                                )
                                                .await
                                                .map_err(|error| {
                                                    CasChangesetUploaderErrorKind::FileUploadFailed(
                                                        leaf.1, error,
                                                    )
                                                })?;
                                            progress_counter.tick(ctx, outcome);
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

        final_progress_counter.log(ctx);
        debug!(
            ctx.logger(),
            "Upload of (bonsai) changeset {} to CAS (recursively) took {:.2} seconds, corresponding hg changeset is {}. Upload included {}.",
            changeset_id,
            start_time.elapsed().as_secs_f64(),
            hg_cs_id,
            match upload_policy {
                UploadPolicy::BlobsOnly => "blobs only",
                UploadPolicy::TreesOnly => "augmented trees only",
                UploadPolicy::All => "augmented trees and blobs",
            }
        );
        STATS::uploaded_changesets_recursive.add_value(1);
        Ok(())
    }
}
