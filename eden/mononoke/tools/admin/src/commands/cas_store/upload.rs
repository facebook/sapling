/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::str::FromStr;

use anyhow::anyhow;
use anyhow::Result;
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
    /// The ID of any one of the changeset that needs to be uploaded into the cas store.
    #[clap(long, short = 'i')]
    changeset_id: Option<String>,
    /// Upload the entire changeset's working copy data recursively.
    #[clap(long)]
    full: bool,
    /// Verbose logging of the upload process (CAS) vs quiet output by default.
    #[clap(long)]
    verbose: bool,
}

pub async fn cas_store_upload(
    ctx: &CoreContext,
    repo: &Repo,
    args: CasStoreUploadArgs,
) -> Result<()> {
    let cas_changesets_uploader = CasChangesetsUploader::new(RemoteExecutionCasdClient::new(
        ctx.fb,
        ctx,
        repo.repo_identity.name(),
        args.verbose,
    )?);

    match args.changeset_id {
        Some(changeset_id) => {
            let mut cs_id = ChangesetId::from_str(&changeset_id);
            if cs_id.is_err() {
                let hgid = HgChangesetId::from_str(&changeset_id)?;
                cs_id = repo
                    .bonsai_hg_mapping
                    .get_bonsai_from_hg(ctx, hgid)
                    .await
                    .map(|cs| {
                        cs.ok_or(anyhow!(
                            "unknown commit identifier (only hg and bonsai are supported)"
                        ))
                    })?;
            }
            let changeset_id = cs_id?;
            match args.full {
                true => {
                    cas_changesets_uploader
                        .upload_single_changeset_recursively(ctx, repo, &changeset_id)
                        .await?;
                }
                false => {
                    cas_changesets_uploader
                        .upload_single_changeset(ctx, repo, &changeset_id)
                        .await?;
                }
            }
        }
        None => {
            println!("No changeset id provided. Currently this is the only supported option.");
        }
    }
    Ok(())
}
