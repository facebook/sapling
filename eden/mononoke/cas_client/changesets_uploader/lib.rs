/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![feature(trait_alias)]

mod errors;

use std::sync::Arc;

use anyhow::Error;
use atomic_counter::AtomicCounter;
use atomic_counter::RelaxedCounter;
use blobstore::Loadable;
use bonsai_hg_mapping::BonsaiHgMappingRef;
use cas_client::CasClient;
use changesets::ChangesetsRef;
use context::CoreContext;
pub use errors::CasChangesetUploaderErrorKind;
use futures::future;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use futures::try_join;
use manifest::find_intersection_of_diffs;
use manifest::Diff;
use manifest::Entry;
use manifest::ManifestOps;
use mercurial_types::HgAugmentedManifestId;
use mercurial_types::HgChangesetId;
use mononoke_types::ChangesetId;
use repo_blobstore::RepoBlobstoreArc;
use scm_client::ScmCasClient;
use slog::debug;
use stats::prelude::*;

const MAX_CONCURRENT_UPLOADS: usize = 200;

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

    async fn get_augmented_manifest_id_from_changeset<'a>(
        &self,
        ctx: &'a CoreContext,
        repo: &impl Repo,
        changeset_id: &ChangesetId,
    ) -> Result<(HgChangesetId, HgAugmentedManifestId), CasChangesetUploaderErrorKind> {
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
        let hg_augmented_manifest = HgAugmentedManifestId::new(hg_manifest.into_nodehash());
        Ok((hg_cs_id, hg_augmented_manifest))
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
        blobs_only: bool,
    ) -> Result<(), Error> {
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
        let hg_cs = hg_cs_id.load(ctx, &blobstore).await?;

        // Diff hg manifest with parents
        let diff_stream = match (
            hg_cs.p1().map(HgChangesetId::new),
            hg_cs.p2().map(HgChangesetId::new),
        ) {
            (Some(p1), Some(p2)) => {
                let p1_hg_cs = p1.load(ctx, &blobstore);
                let p2_hg_cs = p2.load(ctx, &blobstore);
                let hg_cs = hg_cs_id.load(ctx, &blobstore);
                let (p1_hg_cs, p2_hg_cs, hg_cs) =
                    future::try_join3(p1_hg_cs, p2_hg_cs, hg_cs).await?;

                find_intersection_of_diffs(
                    ctx.clone(),
                    blobstore,
                    hg_cs.manifestid(),
                    vec![p1_hg_cs.manifestid(), p2_hg_cs.manifestid()],
                )
                .map_ok(move |(_, entry)| entry)
                .map_err(|e| CasChangesetUploaderErrorKind::DiffChangesetFailed(e.to_string()))
                .boxed()
            }

            (Some(p), None) | (None, Some(p)) => {
                let blobstore = repo.repo_blobstore();
                let parent_hg_cs = p.load(ctx, &blobstore);
                let hg_cs = hg_cs_id.load(ctx, &blobstore);
                let (parent_hg_cs, hg_cs) = try_join!(parent_hg_cs, hg_cs)?;

                parent_hg_cs
                    .manifestid()
                    .diff(ctx.clone(), repo.repo_blobstore_arc(), hg_cs.manifestid())
                    .map_ok(move |diff| match diff {
                        Diff::Added(_, entry) => Some(entry),
                        Diff::Removed(..) => None,
                        Diff::Changed(_, _, entry) => Some(entry),
                    })
                    .try_filter_map(future::ok)
                    .map_err(|e| CasChangesetUploaderErrorKind::DiffChangesetFailed(e.to_string()))
                    .boxed()
            }

            (None, None) => {
                let hg_cs = hg_cs_id.load(ctx, &repo.repo_blobstore()).await?;
                hg_cs
                    .manifestid()
                    .list_all_entries(ctx.clone(), repo.repo_blobstore_arc())
                    .map_ok(move |(_, entry)| entry)
                    .map_err(|e| CasChangesetUploaderErrorKind::DiffChangesetFailed(e.to_string()))
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

        if blobs_only {
            self.client
                .ensure_upload_file_contents(ctx, &blobstore, files_list, true)
                .await?;
        } else {
            try_join!(
                self.client
                    .ensure_upload_augmented_trees(ctx, &blobstore, manifests_list, true),
                self.client
                    .ensure_upload_file_contents(ctx, &blobstore, files_list, true)
            )?;
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
        blobs_only: bool,
    ) -> Result<(), CasChangesetUploaderErrorKind> {
        if blobs_only {
            let hg_cs_id = repo
                .bonsai_hg_mapping()
                .get_hg_from_bonsai(ctx, *changeset_id)
                .await
                .map(|cs| {
                    cs.ok_or(CasChangesetUploaderErrorKind::InvalidChangeset(
                        changeset_id.clone(),
                    ))
                })??;
            let hg_cs = hg_cs_id
                .load(ctx, &repo.repo_blobstore())
                .await
                .map_err(|e| CasChangesetUploaderErrorKind::Error(e.into()))?;
            debug!(
                ctx.logger(),
                "Uploading all blobs recursively for [changeset id: {}, hg changeset id: {}]",
                changeset_id,
                hg_cs_id
            );
            let start_time = std::time::Instant::now();
            let progress_counter: Arc<RelaxedCounter> = Arc::new(Default::default());
            hg_cs
                .manifestid()
                .list_leaf_entries(ctx.clone(), repo.repo_blobstore_arc())
                .map_ok(move |(_, entry)| {
                    let progress_counter = progress_counter.clone();
                    async move {
                        let blobstore = repo.repo_blobstore();
                        self.client
                            .upload_file_content(ctx, &blobstore, &entry.1, None, true)
                            .await?;
                        progress_counter.inc();
                        if progress_counter.get() % 200 == 0 {
                            debug!(ctx.logger(), "Uploaded {} blobs", progress_counter.get());
                        }
                        Ok::<(), Error>(())
                    }
                })
                .try_buffered(MAX_CONCURRENT_UPLOADS)
                .try_collect()
                .await?;
            debug!(
                ctx.logger(),
                "Upload of blobs from changeset {} to CAS (recursively) took {} seconds, corresponding hg changeset is {}",
                changeset_id,
                start_time.elapsed().as_secs_f64(),
                hg_cs_id,
            );
            STATS::uploaded_changesets_recursive.add_value(1);
            return Ok(());
        }

        let (hg_cs_id, hg_augmented_manifest) = self
            .get_augmented_manifest_id_from_changeset(ctx, repo, changeset_id)
            .await?;
        debug!(
            ctx.logger(),
            "Uploading data recursively for root augmented manifest: {}, changeset id: {}, hg changeset id: {}",
            hg_augmented_manifest,
            changeset_id,
            hg_cs_id
        );
        let start_time = std::time::Instant::now();
        self.client
            .upload_root_augmented_tree_recursive(
                ctx,
                &repo.repo_blobstore(),
                &hg_augmented_manifest,
            )
            .await?;
        debug!(
            ctx.logger(),
            "Upload of (bonsai) changeset {} to CAS (recursively) took {} seconds, corresponding hg changeset is {}",
            changeset_id,
            start_time.elapsed().as_secs_f64(),
            hg_cs_id,
        );
        STATS::uploaded_changesets_recursive.add_value(1);
        Ok(())
    }
}
