/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![feature(trait_alias)]

mod errors;

use anyhow::Error;
use blobstore::Loadable;
use bonsai_hg_mapping::BonsaiHgMappingRef;
use cas_client::CasClient;
use changesets::ChangesetsRef;
use context::CoreContext;
pub use errors::CasChangesetUploaderErrorKind;
use mercurial_types::HgAugmentedManifestId;
use mercurial_types::HgChangesetId;
use mononoke_types::ChangesetId;
use repo_blobstore::RepoBlobstoreArc;
use scm_client::ScmCasClient;
use slog::debug;
use slog::trace;
use stats::prelude::*;

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
    pub async fn upload_single_changeset<'a>(
        &self,
        ctx: &'a CoreContext,
        repo: &impl Repo,
        changeset_id: &ChangesetId,
    ) -> Result<(), Error> {
        let bonsai_changeset = changeset_id.load(ctx, &repo.repo_blobstore()).await?;
        let files = bonsai_changeset.file_changes();
        // select changed files, excluding deleted files
        let file_ids = files
            .into_iter()
            .map(|(_path, f)| f.simplify().map(|f| f.content_id()))
            .filter_map(std::convert::identity)
            .collect::<Vec<_>>();
        let start_time = std::time::Instant::now();
        // upload files
        self.client
            .ensure_upload_files_by_content_id(ctx, &repo.repo_blobstore(), file_ids, true)
            .await?;
        // upload manifests
        // TODO: upload augmented_manifests delta for this changeset
        trace!(
            ctx.logger(),
            "Upload of (bonsai) changeset {} to CAS took {} seconds",
            changeset_id,
            start_time.elapsed().as_secs_f64(),
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
    ) -> Result<(), CasChangesetUploaderErrorKind> {
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
