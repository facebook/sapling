/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;

use anyhow::anyhow;
use anyhow::Result;
use blobstore::Blobstore;
use clap::Args;
use commit_graph_types::edges::ChangesetEdges;
use context::CoreContext;
use fbthrift::compact_protocol;
use metaconfig_types::RepoConfigRef;
use mononoke_app::MononokeApp;
use mononoke_types::BlobstoreBytes;
use mononoke_types::ChangesetId;
use preloaded_commit_graph_storage::ExtendablePreloadedEdges;
use repo_blobstore::RepoBlobstoreRef;
use repo_identity::RepoIdentityRef;
use sql_commit_graph_storage::SqlCommitGraphStorage;
use sql_commit_graph_storage::SqlCommitGraphStorageBuilder;
use tokio::time::sleep;
use tokio::time::Duration;

use super::Repo;

#[derive(Args)]
pub struct UpdatePreloadedArgs {
    /// Blobstore key for the preloaded commit graph.
    #[clap(long)]
    blobstore_key: String,

    /// Whether to rebuild the preloaded commit graph or start
    /// from the previous blob.
    #[clap(long)]
    rebuild: bool,

    /// Number of times to retry fetching changeset edges
    /// from the database.
    #[clap(long, default_value_t = 0)]
    sql_retries: u64,

    /// Maximum number of changeset edges in a chunk
    /// fetched from the database.
    #[clap(long)]
    chunk_size: u64,

    /// Sleep time between fetching changeset edges in milliseconds.
    #[clap(long)]
    sleep_ms: u64,

    /// Sleep time before exiting the program in seconds.
    #[clap(long, default_value_t = 60)]
    sleep_before_exit_secs: u64,
}

async fn try_fetch_chunk(
    ctx: &CoreContext,
    sql_storage: &SqlCommitGraphStorage,
    start_id: u64,
    end_id: u64,
    chunk_size: u64,
    mut sql_retries: u64,
    sleep_ms: u64,
) -> Result<HashMap<ChangesetId, ChangesetEdges>> {
    loop {
        match sql_storage
            .fetch_many_edges_in_id_range(ctx, start_id, end_id, chunk_size, false)
            .await
        {
            Ok(edges) => return Ok(edges),
            Err(err) => match sql_retries {
                0 => return Err(err),
                _ => {
                    println!("{:?}", err);
                    println!("Retrying fetching changeset edges");

                    sql_retries -= 1;
                    sleep(Duration::from_millis(sleep_ms)).await;
                }
            },
        }
    }
}

pub(super) async fn update_preloaded(
    ctx: &CoreContext,
    app: &MononokeApp,
    repo: &Repo,
    args: UpdatePreloadedArgs,
) -> Result<()> {
    let sql_storage = app
        .repo_factory()
        .sql_factory(&repo.repo_config().storage_config.metadata)
        .await?
        .open::<SqlCommitGraphStorageBuilder>()
        .await?
        .build(
            app.environment().rendezvous_options,
            repo.repo_identity().id(),
        );

    let preloaded_edges = match args.rebuild {
        false => match repo.repo_blobstore().get(ctx, &args.blobstore_key).await? {
            Some(bytes) => {
                preloaded_commit_graph_storage::deserialize_preloaded_edges(bytes.into_raw_bytes())?
            }
            None => Default::default(),
        },
        true => Default::default(),
    };

    // The newly added changesets all have higher sql ids than the maximum
    // id from the previously preloaded changesets.
    let mut start_id = preloaded_edges
        .max_sql_id
        .map_or(1, |id| id.saturating_add(1));
    // Query the maximum sql id for this repo only once to avoid tailing
    // new changesets.
    let end_id = sql_storage.max_id(ctx, false).await?.unwrap_or(0);

    println!(
        "Updating with changesets having sql ids between {} and {} inclusive",
        start_id, end_id
    );

    let mut extendable_preloaded_edges =
        ExtendablePreloadedEdges::from_preloaded_edges(preloaded_edges);

    while start_id <= end_id {
        // Tries to fetch the first chunk_size changeset edges between
        // start_id and end_id.
        let edges_chunk = try_fetch_chunk(
            ctx,
            &sql_storage,
            start_id,
            end_id,
            args.chunk_size,
            args.sql_retries,
            args.sleep_ms,
        )
        .await?;

        if edges_chunk.is_empty() {
            break;
        }

        // Query the maximum sql id from the fetched chunk to fetch the next
        // chunks from after it.
        let max_id_in_chunk = sql_storage
            .max_id_in_range(ctx, start_id, end_id, edges_chunk.len() as u64, false)
            .await?
            .ok_or_else(|| anyhow!("Chunk is not empty but couldn't find max id"))?;

        println!(
            "Fetched chunk containing {} edges. Maximum sql id in chunk is {}",
            edges_chunk.len(),
            max_id_in_chunk
        );

        for (_cs_id, edges) in edges_chunk {
            extendable_preloaded_edges.add(edges)?;
        }
        extendable_preloaded_edges.update_max_sql_id(max_id_in_chunk);
        start_id = max_id_in_chunk + 1;

        println!("Extended preloaded edges with chunk");

        sleep(Duration::from_millis(args.sleep_ms)).await;
    }
    println!("Deserializing preloaded edges");

    let bytes = compact_protocol::serialize(
        &extendable_preloaded_edges
            .into_preloaded_edges()
            .to_thrift()?,
    );

    println!("Deserialized preloaded edges into {} bytes", bytes.len());

    repo.repo_blobstore()
        .put(ctx, args.blobstore_key, BlobstoreBytes::from_bytes(bytes))
        .await?;

    // In the case of a multiplexed blobstore, the put operation can exit after it succeeds
    // in one inner blobstore before finishing in all, and leave the rest running in the
    // background. This sleep tries to prevent exiting early before they all finish.
    tokio::time::sleep(Duration::from_secs(args.sleep_before_exit_secs)).await;

    println!("Uploaded updated preloaded edges to blobstore");

    Ok(())
}
