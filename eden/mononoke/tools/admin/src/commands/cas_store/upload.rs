/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::str::FromStr;

use anyhow::Result;
use blobstore::Loadable;
use cas_client::RemoteExecutionCasdClient;
use clap::Args;
use context::CoreContext;
use mercurial_derivation::DeriveHgChangeset;
use mercurial_types::HgAugmentedManifestId;
use mononoke_types::ChangesetId;
use scm_client::MononokeCasClient;

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
}

pub async fn cas_store_upload(
    ctx: &CoreContext,
    repo: &Repo,
    args: CasStoreUploadArgs,
) -> Result<()> {
    let mononoke_cas_client = MononokeCasClient::new(RemoteExecutionCasdClient::new(
        ctx.fb,
        ctx,
        repo.repo_identity.name(),
    )?);

    match args.changeset_id {
        Some(changeset_id) => {
            let changeset_id = ChangesetId::from_str(&changeset_id)?;
            let hg_cs_id = repo.derive_hg_changeset(ctx, changeset_id).await?;
            let hg_cs = hg_cs_id.load(ctx, &repo.repo_blobstore).await?;
            let hg_manifest = hg_cs.manifestid();
            let hg_augmented_manifest = HgAugmentedManifestId::new(hg_manifest.into_nodehash());
            match args.full {
                true => {
                    mononoke_cas_client
                        .upload_root_augmented_tree_recursive(
                            ctx,
                            &repo.repo_blobstore,
                            &hg_augmented_manifest,
                        )
                        .await?;
                    println!(
                        "Uploaded data recursively assuming it is derived, augmented manifest: {}, changeset id: {}, hg changeset id: {}",
                        hg_augmented_manifest, changeset_id, hg_cs_id
                    );
                }
                false => {
                    // TODO: find first non-uploaded predecessor commit, diff the trees and upload the diff.
                    println!(
                        "No full option provided. Currently this is the only supported option."
                    );
                }
            }
        }
        None => {
            println!("No changeset id provided. Currently this is the only supported option.");
        }
    }
    Ok(())
}
