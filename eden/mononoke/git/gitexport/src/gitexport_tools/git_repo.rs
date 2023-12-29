/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::PathBuf;

use anyhow::Context;
use anyhow::Result;
use git_symbolic_refs::GitSymbolicRefsEntry;
use gix_hash::ObjectId;
use mononoke_api::CoreContext;
use packfile::bundle::BundleWriter;
use packfile::pack::DeltaForm;
use protocol::generator::generate_pack_item_stream;
use protocol::generator::Repo;
use protocol::types::DeltaInclusion;
use protocol::types::PackItemStreamRequest;
use protocol::types::PackfileItemInclusion;
use protocol::types::TagInclusion;
use slog::info;

pub async fn create_git_repo_on_disk(
    ctx: &CoreContext,
    repo: &impl Repo,
    git_repo_path: PathBuf,
) -> Result<()> {
    let logger = ctx.logger();
    info!(
        logger,
        "Exporting temporary repo to git repo on path: {0:#?}", &git_repo_path
    );
    let logger = ctx.logger();

    let symref_name = "HEAD";
    let ref_name = "master";
    let ref_type = "branch";

    let symref_entry = GitSymbolicRefsEntry::new(
        symref_name.to_string(),
        ref_name.to_string(),
        ref_type.to_string(),
    )?;
    repo.git_symbolic_refs()
        .add_or_update_entries(vec![symref_entry])
        .await
        .context("failed to add symbolic ref entries")?;

    // Open the output file for writing
    let output_file = tokio::fs::File::create(git_repo_path.clone())
        .await
        .with_context(|| format!("Error in opening/creating output file {0:?}", git_repo_path))?;

    let delta_inclusion = DeltaInclusion::Include {
        form: DeltaForm::RefAndOffset,
        inclusion_threshold: 0.6,
    };
    let request = PackItemStreamRequest::full_repo(
        delta_inclusion,
        TagInclusion::AsIs,
        PackfileItemInclusion::Generate,
    );
    let response = generate_pack_item_stream(ctx, repo, request)
        .await
        .context("Error in generating pack item stream")?;

    // Since this is a full clone
    let prereqs: Option<Vec<ObjectId>> = None;

    // Create the bundle writer with the header pre-written
    let concurrency = 1000;
    let mut writer = BundleWriter::new_with_header(
        output_file,
        response.included_refs.into_iter().collect(),
        prereqs,
        response.num_items as u32,
        concurrency,
        DeltaForm::RefAndOffset, // Ref deltas are supported by Git when cloning from a bundle
    )
    .await?;

    writer
        .write(response.items)
        .await
        .context("Error in writing packfile items to bundle")?;

    // Finish writing the bundle
    writer
        .finish()
        .await
        .context("Error in finishing write to bundle")?;

    info!(logger, "Finished creating git repo!");

    Ok(())
}
