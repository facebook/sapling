/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use anyhow::format_err;
use cmdlib_cross_repo::create_single_direction_commit_syncer;
use commit_id::parse_commit_id;
use context::CoreContext;
use futures::TryStreamExt;
use metaconfig_types::CommitSyncConfigVersion;
use mononoke_api::Repo;
use mononoke_app::MononokeApp;
use mononoke_app::args::SourceAndTargetRepoArgs;
use repo_identity::RepoIdentityRef;
use slog::info;
use slog::warn;
use synced_commit_mapping::EquivalentWorkingCopyEntry;
use synced_commit_mapping::WorkingCopyEquivalence;
use tokio::fs::File;
use tokio::io::AsyncBufReadExt;
use tokio::io::BufReader;

use super::common::process_stream_and_wait_for_replication;

/// Mark all commits that do not have any mapping as not synced candidate, but leave those that have the mapping alone
#[derive(Debug, clap::Args)]
pub struct MarkNotSyncedArgs {
    #[clap(flatten)]
    repo_args: SourceAndTargetRepoArgs,

    #[clap(long, help = "A version to use")]
    mapping_version_name: String,

    #[clap(
        long,
        help = "List of large repo commit hashes that should be considered to be marked as not sync candidate"
    )]
    input_file: String,

    #[clap(long, help = "Whether to overwrite existing values or not")]
    overwrite: bool,
}

pub async fn run(ctx: &CoreContext, app: MononokeApp, args: MarkNotSyncedArgs) -> Result<()> {
    let small_repo: Repo = app.open_repo(&args.repo_args.source_repo).await?;
    let large_repo: Repo = app.open_repo(&args.repo_args.target_repo).await?;
    let commit_sync_data =
        create_single_direction_commit_syncer(ctx, &app, small_repo.clone(), large_repo.clone())
            .await?;
    info!(
        ctx.logger(),
        "small repo: {}, large repo: {}",
        small_repo.repo_identity().name(),
        large_repo.repo_identity().name(),
    );
    let mapping_version_name = CommitSyncConfigVersion(args.mapping_version_name.to_string());
    let mapping = commit_sync_data.get_mapping();

    if !commit_sync_data
        .version_exists(&mapping_version_name)
        .await?
    {
        return Err(format_err!("{} version is not found", mapping_version_name));
    }

    let input_file = File::open(&args.input_file)
        .await
        .with_context(|| format!("Failed to open {}", args.input_file))?;
    let reader = BufReader::new(input_file);

    let s = tokio_stream::wrappers::LinesStream::new(reader.lines())
        .map_err(Error::from)
        .map_ok(async |line| {
            let cs_id = parse_commit_id(ctx, &large_repo, &line).await?;

            let existing_value = mapping
                .get_equivalent_working_copy(
                    ctx,
                    large_repo.repo_identity().id(),
                    cs_id,
                    small_repo.repo_identity().id(),
                )
                .await?;

            if args.overwrite {
                if let Some(WorkingCopyEquivalence::WorkingCopy(_, _)) = existing_value {
                    return Err(format_err!("unexpected working copy found for {}", cs_id));
                }
            } else if existing_value.is_some() {
                info!(ctx.logger(), "{} already have mapping", cs_id);
                return Ok(1);
            }

            let wc_entry = EquivalentWorkingCopyEntry {
                large_repo_id: large_repo.repo_identity().id(),
                large_bcs_id: cs_id,
                small_repo_id: small_repo.repo_identity().id(),
                small_bcs_id: None,
                version_name: Some(mapping_version_name.clone()),
            };
            let res = if args.overwrite {
                mapping
                    .overwrite_equivalent_working_copy(ctx, wc_entry)
                    .await?
            } else {
                mapping
                    .insert_equivalent_working_copy(ctx, wc_entry)
                    .await?
            };
            if !res {
                warn!(
                    ctx.logger(),
                    "failed to insert NotSyncedMapping entry for {}", cs_id
                );
            }

            // Processed a single entry
            Ok(1)
        })
        .try_buffer_unordered(100);

    process_stream_and_wait_for_replication(ctx, &commit_sync_data, s).await?;
    Ok(())
}
