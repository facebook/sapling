/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Result;
use cas_client::build_mononoke_cas_client;
use changesets_uploader::CasChangesetsUploader;
use changesets_uploader::PriorLookupPolicy;
use changesets_uploader::UploadPolicy;
use clap::Args;
use context::CoreContext;
use mercurial_types::HgChangesetId;
use mononoke_types::ChangesetId;
use mononoke_types::MPath;
use slog::info;

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
    /// Upload only the specified path (allowed for full uploads only)
    #[clap(long, short, requires = "full")]
    path: Option<String>,
    /// Verbose logging of the upload process (CAS) vs quiet output by default.
    #[clap(long)]
    verbose: bool,
    /// Upload only the blobs of a changeset.
    #[clap(long)]
    blobs_only: bool,
    /// Upload only the trees of a changeset.
    #[clap(long, conflicts_with = "blobs_only")]
    trees_only: bool,
}

pub async fn cas_store_upload(
    ctx: &CoreContext,
    repo: &Repo,
    args: CasStoreUploadArgs,
) -> Result<()> {
    let cas_changesets_uploader = CasChangesetsUploader::new(build_mononoke_cas_client(
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

    let upload_policy = if args.trees_only {
        UploadPolicy::TreesOnly
    } else if args.blobs_only {
        UploadPolicy::BlobsOnly
    } else {
        UploadPolicy::All
    };

    let mut path = None;
    if let Some(ref spath) = args.path {
        path = Some(MPath::new(spath).with_context(|| anyhow!("Invalid path: {}", spath))?);
    }

    let stats = if args.full {
        cas_changesets_uploader
            .upload_single_changeset_recursively(
                ctx,
                repo,
                &changeset_id,
                path,
                upload_policy,
                PriorLookupPolicy::All,
            )
            .await?
    } else {
        cas_changesets_uploader
            .upload_single_changeset(
                ctx,
                repo,
                &changeset_id,
                upload_policy,
                PriorLookupPolicy::All,
            )
            .await?
    };

    info!(ctx.logger(), "Upload completed. Upload stats: {}", stats);

    Ok(())
}
