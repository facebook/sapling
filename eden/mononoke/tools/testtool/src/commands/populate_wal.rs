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
use metaconfig_types::BlobConfig;
use mononoke_app::MononokeApp;
use mononoke_app::args::RepoArgs;
use mononoke_types::Timestamp;
use repo_identity::RepoIdentityRef;
use sql_construct::SqlConstructFromShardedDatabaseConfig;

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

    /// Delete blobs from target blobstore directory
    #[clap(long)]
    delete_target_blobs: bool,

    /// Blobstore ID to delete blobs from (defaults to 0)
    #[clap(long, default_value = "0")]
    delete_from_blobstore_id: u32,

    /// Storage ID to get the WAL from (recommended, extracts multiplex_id from config)
    #[clap(long)]
    storage_id: Option<String>,

    /// Target blobstore ID to use as multiplex_id (legacy, for backward compatibility)
    #[clap(long)]
    target_blobstore_id: Option<u32>,
}

pub async fn run(app: MononokeApp, args: CommandArgs) -> Result<()> {
    let ctx = app.new_basic_context();

    let repo: Repo = app
        .open_repo(&args.repo_args)
        .await
        .context("Failed to open repo")?;

    // Get repo prefix for blob keys
    let repo_prefix = repo.repo_identity().name();

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
    let blob_keys = list_blob_keys(&source_blobs_dir, repo_prefix)?;

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

    // Determine which approach to use for getting WAL and multiplex_id
    let (wal, multiplex_id) = match (&args.storage_id, args.target_blobstore_id) {
        // Preferred approach: Use storage_id to extract multiplex_id from config
        (Some(storage_id), _) => {
            let storage_configs = app.storage_configs();
            let storage_config = storage_configs
                .storage
                .get(storage_id)
                .ok_or_else(|| anyhow!("Storage config not found for ID: {}", storage_id))?;

            // Extract queue_db and multiplex_id from MultiplexedWal config
            let (queue_db, multiplex_id) = match &storage_config.blobstore {
                BlobConfig::MultiplexedWal {
                    queue_db,
                    multiplex_id,
                    ..
                } => (queue_db, *multiplex_id),
                _ => {
                    return Err(anyhow!(
                        "Storage config for '{}' is not MultiplexedWal",
                        storage_id
                    ));
                }
            };

            // Build the WAL using SqlBlobstoreWalBuilder
            let wal = SqlBlobstoreWalBuilder::with_sharded_database_config(
                ctx.fb,
                queue_db,
                app.mysql_options(),
                false, // readonly_storage
            )
            .context("While opening WAL")?
            .build(ctx.sql_query_telemetry());

            (wal, multiplex_id.into())
        }

        // Legacy approach: Use target_blobstore_id directly as multiplex_id
        (None, Some(target_blobstore_id)) => {
            // Get repo config and extract blobstore config directly
            let repo_configs = app.repo_configs();
            let (_repo_name, repo_config) = repo_configs
                .get_repo_config(repo.repo_identity().id())
                .ok_or_else(|| {
                    anyhow!(
                        "Repo config not found for repo_id: {:?}",
                        repo.repo_identity().id()
                    )
                })?;

            // Extract queue_db from the repo's MultiplexedWal config
            let queue_db = match &repo_config.storage_config.blobstore {
                BlobConfig::MultiplexedWal { queue_db, .. } => queue_db,
                _ => {
                    return Err(anyhow!(
                        "Repo storage config is not MultiplexedWal (required for WAL). Use --storage-id instead."
                    ));
                }
            };

            // Build the WAL using SqlBlobstoreWalBuilder
            let wal = SqlBlobstoreWalBuilder::with_sharded_database_config(
                ctx.fb,
                queue_db,
                app.mysql_options(),
                false, // readonly_storage
            )
            .context("While opening WAL")?
            .build(ctx.sql_query_telemetry());

            // Use target_blobstore_id as multiplex_id (legacy behavior)
            (wal, target_blobstore_id as i32)
        }

        // Error: Neither parameter specified
        (None, None) => {
            return Err(anyhow!(
                "Must specify either --storage-id (recommended) or --target-blobstore-id (legacy)"
            ));
        }
    };

    insert_wal_entries(&ctx, &wal, &blob_keys, multiplex_id).await?;

    eprintln!(
        "Inserted {} WAL entries for target multiplex_id {}",
        blob_keys.len(),
        multiplex_id
    );

    Ok(())
}

fn list_blob_keys(blobs_dir: &Path, _repo_prefix: &str) -> Result<Vec<(String, u64)>> {
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

            // Remove "blob-" prefix if present - the rest is the full blobstore key
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
    wal: &dyn BlobstoreWal,
    blob_keys: &[(String, u64)],
    multiplex_id: i32,
) -> Result<()> {
    let timestamp = Timestamp::now();

    // Collect all entries into a vector for batch insertion
    let entries: Vec<BlobstoreWalEntry> = blob_keys
        .iter()
        .map(|(key, size)| {
            BlobstoreWalEntry::new(key.clone(), multiplex_id.into(), timestamp, *size)
        })
        .collect();

    // Insert all entries in one batch
    wal.log_many(ctx, entries)
        .await
        .context("Failed to insert WAL entries")?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs::File;
    use std::io::Write;

    use mononoke_macros::mononoke;

    use super::*;

    #[mononoke::test]
    fn test_list_blob_keys_with_prefix() {
        let temp_dir = tempfile::tempdir().unwrap();
        let blobs_dir = temp_dir.path().join("blobs");
        fs::create_dir(&blobs_dir).unwrap();

        // Create test blobs with "blob-" prefix
        let test_data = vec![
            ("blob-repo0000.content.blake2.abc123", b"content1"),
            ("blob-repo0000.content.blake2.def456", b"content2"),
        ];

        for (name, content) in &test_data {
            let mut file = File::create(blobs_dir.join(name)).unwrap();
            file.write_all(*content).unwrap();
        }

        let result = list_blob_keys(&blobs_dir, "repo0000").unwrap();

        assert_eq!(result.len(), 2);
        // Keys should have "blob-" prefix stripped
        assert!(
            result
                .iter()
                .any(|(key, size)| key == "repo0000.content.blake2.abc123" && *size == 8)
        );
        assert!(
            result
                .iter()
                .any(|(key, size)| key == "repo0000.content.blake2.def456" && *size == 8)
        );
    }

    #[mononoke::test]
    fn test_list_blob_keys_without_prefix() {
        let temp_dir = tempfile::tempdir().unwrap();
        let blobs_dir = temp_dir.path().join("blobs");
        fs::create_dir(&blobs_dir).unwrap();

        // Create test blobs without "blob-" prefix
        let test_data = vec![
            ("repo0000.alias.sha256.xyz789", b"data1"),
            ("repo0000.changeset.blake2.aaa111", b"data2"),
        ];

        for (name, content) in &test_data {
            let mut file = File::create(blobs_dir.join(name)).unwrap();
            file.write_all(*content).unwrap();
        }

        let result = list_blob_keys(&blobs_dir, "repo0000").unwrap();

        assert_eq!(result.len(), 2);
        assert!(
            result
                .iter()
                .any(|(key, size)| key == "repo0000.alias.sha256.xyz789" && *size == 5)
        );
        assert!(
            result
                .iter()
                .any(|(key, size)| key == "repo0000.changeset.blake2.aaa111" && *size == 5)
        );
    }

    #[mononoke::test]
    fn test_list_blob_keys_empty_directory() {
        let temp_dir = tempfile::tempdir().unwrap();
        let blobs_dir = temp_dir.path().join("blobs");
        fs::create_dir(&blobs_dir).unwrap();

        let result = list_blob_keys(&blobs_dir, "repo0000").unwrap();

        assert_eq!(result.len(), 0);
    }

    #[mononoke::test]
    fn test_list_blob_keys_nonexistent_directory() {
        let temp_dir = tempfile::tempdir().unwrap();
        let blobs_dir = temp_dir.path().join("nonexistent");

        let result = list_blob_keys(&blobs_dir, "repo0000");

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Blobstore directory does not exist")
        );
    }

    #[mononoke::test]
    fn test_delete_blobs_with_prefix() {
        let temp_dir = tempfile::tempdir().unwrap();
        let target_dir = temp_dir.path().join("target");
        fs::create_dir(&target_dir).unwrap();

        // Create test blobs with "blob-" prefix
        File::create(target_dir.join("blob-repo0000.content.blake2.abc123")).unwrap();
        File::create(target_dir.join("blob-repo0000.alias.sha256.def456")).unwrap();

        let blob_keys = vec![
            ("repo0000.content.blake2.abc123".to_string(), 100u64),
            ("repo0000.alias.sha256.def456".to_string(), 200u64),
        ];

        delete_blobs(&target_dir, &blob_keys).unwrap();

        // Files should be deleted
        assert!(
            !target_dir
                .join("blob-repo0000.content.blake2.abc123")
                .exists()
        );
        assert!(
            !target_dir
                .join("blob-repo0000.alias.sha256.def456")
                .exists()
        );
    }

    #[mononoke::test]
    fn test_delete_blobs_without_prefix() {
        let temp_dir = tempfile::tempdir().unwrap();
        let target_dir = temp_dir.path().join("target");
        fs::create_dir(&target_dir).unwrap();

        // Create test blobs without "blob-" prefix
        File::create(target_dir.join("repo0000.content.blake2.xyz789")).unwrap();
        File::create(target_dir.join("repo0000.changeset.blake2.aaa111")).unwrap();

        let blob_keys = vec![
            ("repo0000.content.blake2.xyz789".to_string(), 100u64),
            ("repo0000.changeset.blake2.aaa111".to_string(), 200u64),
        ];

        delete_blobs(&target_dir, &blob_keys).unwrap();

        // Files should be deleted
        assert!(!target_dir.join("repo0000.content.blake2.xyz789").exists());
        assert!(!target_dir.join("repo0000.changeset.blake2.aaa111").exists());
    }

    #[mononoke::test]
    fn test_delete_blobs_nonexistent_directory() {
        let temp_dir = tempfile::tempdir().unwrap();
        let target_dir = temp_dir.path().join("nonexistent");

        let blob_keys = vec![("repo0000.content.blake2.abc123".to_string(), 100u64)];

        // Should not error when directory doesn't exist
        let result = delete_blobs(&target_dir, &blob_keys);
        assert!(result.is_ok());
    }

    #[mononoke::test]
    fn test_delete_blobs_missing_files() {
        let temp_dir = tempfile::tempdir().unwrap();
        let target_dir = temp_dir.path().join("target");
        fs::create_dir(&target_dir).unwrap();

        // Create only one of the files
        File::create(target_dir.join("blob-repo0000.content.blake2.abc123")).unwrap();

        let blob_keys = vec![
            ("repo0000.content.blake2.abc123".to_string(), 100u64),
            ("repo0000.nonexistent.blake2.missing".to_string(), 200u64),
        ];

        // Should not error when some files are missing
        let result = delete_blobs(&target_dir, &blob_keys);
        assert!(result.is_ok());

        // Existing file should be deleted
        assert!(
            !target_dir
                .join("blob-repo0000.content.blake2.abc123")
                .exists()
        );
    }

    #[mononoke::test]
    fn test_list_and_delete_integration() {
        let temp_dir = tempfile::tempdir().unwrap();
        let source_dir = temp_dir.path().join("source");
        let target_dir = temp_dir.path().join("target");
        fs::create_dir(&source_dir).unwrap();
        fs::create_dir(&target_dir).unwrap();

        // Create identical blobs in both directories
        let test_files = vec![
            "blob-repo0000.content.blake2.test1",
            "blob-repo0000.content.blake2.test2",
            "blob-repo0000.alias.sha256.test3",
        ];

        for filename in &test_files {
            File::create(source_dir.join(filename)).unwrap();
            File::create(target_dir.join(filename)).unwrap();
        }

        // List blobs from source
        let blob_keys = list_blob_keys(&source_dir, "repo0000").unwrap();
        assert_eq!(blob_keys.len(), 3);

        // Delete from target
        delete_blobs(&target_dir, &blob_keys).unwrap();

        // Target should be empty
        for filename in &test_files {
            assert!(!target_dir.join(filename).exists());
        }

        // Source should still have files
        for filename in &test_files {
            assert!(source_dir.join(filename).exists());
        }
    }
}
