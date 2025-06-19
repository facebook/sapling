/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use cmdlib_cross_repo::create_single_direction_commit_syncer;
use commit_id::parse_commit_id;
use context::CoreContext;
use futures::StreamExt;
use futures::TryStreamExt;
use futures::future::try_join;
use metaconfig_types::CommitSyncConfigVersion;
use mononoke_api::Repo;
use mononoke_app::MononokeApp;
use mononoke_app::args::SourceAndTargetRepoArgs;
use repo_identity::RepoIdentityRef;
use slog::info;
use synced_commit_mapping::SyncedCommitMappingEntry;
use tokio::fs::File;
use tokio::io::AsyncBufReadExt;
use tokio::io::BufReader;

use super::common::process_stream_and_wait_for_replication;

/// Given the list of commit identifiers resolve them to bonsai hashes in source
/// and target repo and insert a sync commit mapping with specified version name.
/// This is useful for initial backfill to mark commits that are identical between
/// repositories.
/// Input file can contain any commit identifier (e.g. bookmark name)
/// but the safest approach is to use commit hashes (bonsai or hg)
/// 'source-repo' argument represents the small repo while 'target-repo' is the large repo.
#[derive(Debug, clap::Args)]
pub struct BackfillNoopMappingArgs {
    #[clap(flatten)]
    repo_args: SourceAndTargetRepoArgs,

    #[clap(long, help = "Name of the noop mapping that will be inserted")]
    mapping_version_name: String,

    #[clap(
        long,
        help = "List of commit hashes which are remapped with noop mapping"
    )]
    input_file: String,
}

pub async fn run(ctx: &CoreContext, app: MononokeApp, args: BackfillNoopMappingArgs) -> Result<()> {
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
    let input_file = File::open(&args.input_file)
        .await
        .with_context(|| format!("Failed to open {}", args.input_file))?;
    let reader = BufReader::new(input_file);

    let s = tokio_stream::wrappers::LinesStream::new(reader.lines())
        .map_err(Error::from)
        .map_ok(async |cs_id| {
            let small_cs_id = parse_commit_id(ctx, &small_repo, &cs_id);

            let large_cs_id = parse_commit_id(ctx, &large_repo, &cs_id);

            let (small_cs_id, large_cs_id) = try_join(small_cs_id, large_cs_id).await?;

            let entry = SyncedCommitMappingEntry {
                large_repo_id: large_repo.repo_identity().id(),
                large_bcs_id: large_cs_id,
                small_repo_id: small_repo.repo_identity().id(),
                small_bcs_id: small_cs_id,
                version_name: Some(mapping_version_name.clone()),
                source_repo: Some(commit_sync_data.get_source_repo_type()),
            };
            Ok(entry)
        })
        .try_buffer_unordered(100)
        .chunks(100)
        .then(async |chunk| {
            let mapping = commit_sync_data.get_mapping();
            let chunk: Result<Vec<_>> = chunk.into_iter().collect();
            let chunk = chunk?;
            let len = chunk.len();
            mapping.add_bulk(ctx, chunk).await?;
            Result::<_>::Ok(len as u64)
        })
        .boxed();

    process_stream_and_wait_for_replication(ctx, &commit_sync_data, s).await?;

    Ok(())
}
