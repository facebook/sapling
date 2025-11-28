/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Populate WAL Queue for Testing
//!
//! This command simulates blobstore write failures by:
//! 1. Listing blobs from a source blobstore directory
//! 2. Optionally deleting those blobs from a target blobstore directory
//! 3. Inserting WAL entries for the target blobstore
//!
//! This is useful for testing the blobstore healer without using blobimport.

use std::fs;
use std::path::Path;
use std::path::PathBuf;

use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;
use blobstore_sync_queue::BlobstoreWal;
use blobstore_sync_queue::BlobstoreWalEntry;
use blobstore_sync_queue::SqlBlobstoreWalBuilder;
use clap::Parser;
use context::CoreContext;
use mononoke_app::MononokeApp;
use mononoke_app::args::RepoArgs;
use mononoke_types::Timestamp;
use sql_construct::SqlConstruct;

use crate::repo::Repo;

/// Populate the WAL queue with entries for testing healer functionality.
#[derive(Parser)]
pub struct CommandArgs {
    #[clap(flatten)]
    repo_args: RepoArgs,

    /// Path to the blobstore base directory (e.g., $TESTTMP/blobstore)
    #[clap(long)]
    blobstore_path: PathBuf,

    /// Source blobstore ID to copy blob list from
    #[clap(long)]
    source_blobstore_id: u32,

    /// Target blobstore ID that is "missing" the blobs (multiplex_id)
    #[clap(long)]
    target_blobstore_id: i32,

    /// Delete blobs from target blobstore directory
    #[clap(long)]
    delete_target_blobs: bool,

    /// Blobstore ID to delete blobs from (defaults to 0)
    #[clap(long, default_value = "0")]
    delete_from_blobstore_id: u32,

    /// Path to the blobstore sync queue database
    #[clap(long, default_value = "")]
    wal_db_path: String,
}

pub async fn run(app: MononokeApp, args: CommandArgs) -> Result<()> {
    let ctx = app.new_basic_context();

    let _repo: Repo = app
        .open_repo(&args.repo_args)
        .await
        .context("Failed to open repo")?;

    // Construct paths
    let source_blobs_dir = args
        .blobstore_path
        .join(args.source_blobstore_id.to_string())
        .join("blobs");
    let delete_blobs_dir = args
        .blobstore_path
        .join(args.delete_from_blobstore_id.to_string())
        .join("blobs");

    // Get list of blob keys from source blobstore
    let blob_keys = list_blob_keys(&source_blobs_dir)?;

    println!(
        "Found {} blobs in source blobstore {}",
        blob_keys.len(),
        args.source_blobstore_id
    );

    if blob_keys.is_empty() {
        return Err(anyhow!(
            "No blobs found in source blobstore directory: {}",
            source_blobs_dir.display()
        ));
    }

    // Delete blobs from target blobstore if requested
    if args.delete_target_blobs {
        delete_blobs(&delete_blobs_dir, &blob_keys)?;
        eprintln!(
            "Deleted {} blobs from target blobstore {}",
            blob_keys.len(),
            args.delete_from_blobstore_id
        );
    }

    // Insert WAL entries
    let wal_db_path = if args.wal_db_path.is_empty() {
        // Default to $TESTTMP/blobstore_sync_queue/sqlite_dbs (the database file)
        let testtmp = std::env::var("TESTTMP").context("TESTTMP environment variable not set")?;
        PathBuf::from(testtmp)
            .join("blobstore_sync_queue")
            .join("sqlite_dbs")
    } else {
        PathBuf::from(&args.wal_db_path)
    };

    // Create parent directory if it doesn't exist
    if let Some(parent) = wal_db_path.parent() {
        std::fs::create_dir_all(parent).with_context(|| {
            format!(
                "Failed to create WAL database directory: {}",
                parent.display()
            )
        })?;
    }

    insert_wal_entries(&ctx, &wal_db_path, &blob_keys, args.target_blobstore_id).await?;

    eprintln!(
        "Inserted {} WAL entries for target multiplex_id {}",
        blob_keys.len(),
        args.target_blobstore_id
    );

    Ok(())
}

fn list_blob_keys(blobs_dir: &Path) -> Result<Vec<(String, u64)>> {
    if !blobs_dir.exists() {
        return Err(anyhow!(
            "Blobstore directory does not exist: {}",
            blobs_dir.display()
        ));
    }

    let mut blob_keys = Vec::new();

    for entry in fs::read_dir(blobs_dir)
        .with_context(|| format!("Failed to read directory: {}", blobs_dir.display()))?
    {
        let entry = entry?;
        let path = entry.path();

        if path.is_file() {
            let filename = path
                .file_name()
                .and_then(|n| n.to_str())
                .ok_or_else(|| anyhow!("Invalid filename"))?;

            // Remove "blob-" prefix if present
            let key = if let Some(stripped) = filename.strip_prefix("blob-") {
                stripped.to_string()
            } else {
                filename.to_string()
            };

            // Get file size
            let metadata = fs::metadata(&path)?;
            let size = metadata.len();

            blob_keys.push((key, size));
        }
    }

    Ok(blob_keys)
}

fn delete_blobs(target_dir: &Path, blob_keys: &[(String, u64)]) -> Result<()> {
    if !target_dir.exists() {
        eprintln!(
            "Target blobstore directory does not exist: {}, skipping deletion",
            target_dir.display()
        );
        return Ok(());
    }

    for (key, _) in blob_keys {
        // Try both with and without "blob-" prefix
        let paths = [
            target_dir.join(format!("blob-{}", key)),
            target_dir.join(key),
        ];

        for path in &paths {
            if path.exists() {
                fs::remove_file(path)
                    .with_context(|| format!("Failed to delete blob: {}", path.display()))?;
            }
        }
    }

    Ok(())
}

async fn insert_wal_entries(
    ctx: &CoreContext,
    wal_db_path: &Path,
    blob_keys: &[(String, u64)],
    multiplex_id: i32,
) -> Result<()> {
    // Build WAL using SQLite path
    let wal = SqlBlobstoreWalBuilder::with_sqlite_path(wal_db_path, false /* readonly */)?
        .build(ctx.sql_query_telemetry());

    let timestamp = Timestamp::now();

    // Insert entries for each blob
    for (key, size) in blob_keys {
        let entry = BlobstoreWalEntry::new(key.clone(), multiplex_id.into(), timestamp, *size);

        wal.log(ctx, entry)
            .await
            .with_context(|| format!("Failed to insert WAL entry for key: {}", key))?;
    }

    Ok(())
}
