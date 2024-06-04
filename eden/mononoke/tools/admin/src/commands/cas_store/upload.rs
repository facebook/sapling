/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::str::FromStr;

use anyhow::anyhow;
use anyhow::Result;
use blobstore::Loadable;
use cas_client::RemoteExecutionCasdClient;
use clap::Args;
use context::CoreContext;
use mercurial_derivation::DeriveHgChangeset;
use mercurial_types::HgAugmentedManifestId;
use mercurial_types::HgChangesetId;
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
    /// Verbose logging of the upload process (CAS) vs quiet output by default.
    #[clap(long)]
    verbose: bool,
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
                    // For now, assuming the parent commit has been uploaded!
                    // So, the logic is only for the current commit!
                    // For now the logic is only to upload the files, not the (augmented) trees.
                    let bonsai_changeset = changeset_id.load(ctx, &repo.repo_blobstore).await?;
                    let files = bonsai_changeset.file_changes();
                    let file_ids = files
                        .into_iter()
                        .map(|(_path, f)| f.simplify().map(|f| f.content_id()))
                        .filter_map(std::convert::identity)
                        .collect::<Vec<_>>();
                    println!("Uploading {} files", file_ids.len());
                    mononoke_cas_client
                        .ensure_upload_files_by_content_id(
                            ctx,
                            &repo.repo_blobstore,
                            file_ids,
                            true,
                        )
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
