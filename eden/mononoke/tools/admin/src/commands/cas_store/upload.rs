/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::anyhow;
use anyhow::Result;
#[cfg(not(fbcode_build))]
use cas_client::DummyCasClient;
#[cfg(fbcode_build)]
use cas_client::RemoteExecutionCasdClient;
use changesets_uploader::CasChangesetsUploader;
use clap::Args;
use context::CoreContext;
use mercurial_types::HgChangesetId;
use mononoke_types::ChangesetId;

use super::Repo;

#[derive(Args)]
/// Subcommand to upload (augmented) tree and blob data into the cas store.
/// This command can also upload the entire changeset.
pub struct CasStoreUploadArgs {
    /// Bonsai changeset id that needs to be uploaded into the cas store.
    #[clap(long, short = 'i')]
    changeset_id: Option<ChangesetId>,
    /// Hg changeset id that needs to be uploaded into the cas store.
    #[clap(long)]
    hg_id: Option<HgChangesetId>,
    /// Upload the entire changeset's working copy data recursively.
    #[clap(long)]
    full: bool,
    /// Verbose logging of the upload process (CAS) vs quiet output by default.
    #[clap(long)]
    verbose: bool,
    /// Upload only the blobs of a changeset.
    #[clap(long)]
    blobs_only: bool,
}

pub async fn cas_store_upload(
    ctx: &CoreContext,
    repo: &Repo,
    args: CasStoreUploadArgs,
) -> Result<()> {
    #[cfg(not(fbcode_build))]
    let cas_changesets_uploader = CasChangesetsUploader::new(DummyCasClient::default());

    #[cfg(fbcode_build)]
    let cas_changesets_uploader = CasChangesetsUploader::new(RemoteExecutionCasdClient::new(
        ctx.fb,
        ctx,
        repo.repo_identity.name(),
        args.verbose,
    )?);
    let changeset_id = match args.changeset_id {
        Some(changeset_id) => Ok(changeset_id),
        None => match args.hg_id {
            Some(hg_id) => repo
                .bonsai_hg_mapping
                .get_bonsai_from_hg(ctx, hg_id)
                .await?
                .ok_or(anyhow!("No bonsai changeset found for hg id {}", hg_id)),
            None => Err(anyhow!(
                "No changeset id provided. Either hg or bonsai changeset id must be provided."
            )),
        },
    }?;
    match args.full {
        true => {
            cas_changesets_uploader
                .upload_single_changeset_recursively(ctx, repo, &changeset_id, args.blobs_only)
                .await?;
        }
        false => {
            cas_changesets_uploader
                .upload_single_changeset(ctx, repo, &changeset_id, args.blobs_only)
                .await?;
        }
    }
    Ok(())
}
