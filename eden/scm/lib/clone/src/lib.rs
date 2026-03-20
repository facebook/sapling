/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::HashSet;
use std::env;
use std::ffi::OsStr;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

use anyhow::Context;
use anyhow::Result;
use anyhow::bail;
use async_runtime::block_on;
use async_runtime::stream_to_iter;
use configmodel::Config;
use configmodel::ConfigExt;
use configmodel::Text;
use context::CoreContext;
use edenapi::SaplingRemoteApi;
use edenapi_types::legacy::StreamingChangelogData;
use fs_err as fs;
use progress_model::ProgressBar;
use repo::repo::Repo;
use serde::Deserialize;
use serde::Serialize;
use tracing::instrument;
use types::HgId;
use util::errors::IOContext;
use util::file::atomic_write;
use util::path::absolute;
use util::path::expand_path;

pub fn get_default_destination_directory(config: &dyn Config) -> Result<PathBuf> {
    Ok(absolute(
        if let Some(default_dir) = config.get("clone", "default-destination-dir") {
            expand_path(default_dir)
        } else {
            env::current_dir()?
        },
    )?)
}

pub fn get_default_eden_backing_directory(config: &dyn Config) -> Result<Option<PathBuf>> {
    let legacy_dir = config
        .get("edenfs", "legacy-backing-repos-dir")
        .map(expand_path);
    if let Some(legacy_dir) = legacy_dir {
        if legacy_dir.exists() {
            return Ok(Some(legacy_dir));
        }
    }
    Ok(config.get("edenfs", "backing-repos-dir").map(expand_path))
}

#[instrument(skip(ctx), err)]
pub fn init_working_copy(
    ctx: &CoreContext,
    repo: &mut Repo,
    target: Option<HgId>,
    sparse_profiles: Vec<String>,
) -> Result<()> {
    if !sparse_profiles.is_empty() {
        let mut sparse_contents: Vec<u8> = Vec::new();
        for profile in &sparse_profiles {
            write!(&mut sparse_contents, "%include {}\n", profile)
                .io_context("error generating sparse contents")?;
        }
        atomic_write(&repo.dot_hg_path().join("sparse"), |f| {
            f.write_all(&sparse_contents)
        })?;
    }

    let wc = repo.working_copy()?;

    if let Some(target) = target {
        let wc = wc.write();
        let wc = wc.lock()?;

        if let Err(err) = checkout::checkout(
            ctx,
            repo,
            &wc,
            target,
            checkout::BookmarkAction::None,
            checkout::CheckoutMode::AbortIfConflicts,
            checkout::ReportMode::Minimal,
            true,
        ) {
            if ctx.config.get_or_default("checkout", "resumable")? {
                ctx.logger.info(format!(
                    "Checkout failed. Resume with '{} checkout --continue'",
                    ctx.logger.cli_name(),
                ));
            }
            return Err(err);
        }
    }

    Ok(())
}

#[cfg(feature = "eden")]
#[derive(Debug, thiserror::Error)]
pub enum EdenCloneError {
    #[error("Failed cloning eden checkout\n Stdout: '{0}'\n Stderr: '{1}'")]
    ExecutionFailure(String, String),
    #[error("edenfs.command config is not set")]
    MissingCommandConfig(),
}

#[cfg(feature = "eden")]
#[tracing::instrument]
fn run_eden_clone_command(clone_command: &mut Command) -> Result<()> {
    let output = clone_command.output().with_context(|| {
        let binary_path = PathBuf::from(clone_command.get_program());
        // On Windows, users frequently hit clone errors caused by EdenFS not being installed.
        if cfg!(windows) && !binary_path.exists() {
            format!(
                "failed to execute {:?}: edenfs binary not found at {:?}.",
                clone_command, binary_path
            )
        } else {
            format!("failed to execute {:?}", clone_command)
        }
    })?;

    if !output.status.success() {
        return Err(EdenCloneError::ExecutionFailure(
            String::from_utf8_lossy(&output.stdout).to_string(),
            String::from_utf8_lossy(&output.stderr).to_string(),
        )
        .into());
    }

    if String::from_utf8_lossy(&output.stdout)
        .to_string()
        .contains("edenfs daemon is not currently running")
    {
        tracing::debug!(target: "clone_info", edenfs_started_at_clone="true");
    }
    Ok(())
}

#[cfg(feature = "eden")]
#[instrument(err)]
pub fn eden_clone(
    repo: &Repo,
    working_copy: &Path,
    target: Option<HgId>,
    filters: Option<HashSet<Text>>,
) -> Result<()> {
    let config = repo.config();

    let mut clone_command = edenfs_client::build_eden_command(config)?;

    clone_command.args([
        OsStr::new("clone"),
        repo.shared_path().as_os_str(),
        working_copy.as_os_str(),
    ]);

    if let Some(rev) = target {
        clone_command.args(["-r", &rev.to_hex()]);
    } else {
        clone_command.arg("--allow-empty-repo");
    }

    if let Some(filters) = filters {
        if filters.iter().any(|f| !f.is_empty()) {
            clone_command.args(["--backing-store", "filteredhg"]);
            // TODO: We should use "--filter-paths" once it's rolled out everywhere
            let mut filter_args = vec!["--filter-path"];
            filter_args.append(&mut filters.iter().map(|f| f.as_ref()).collect());
            clone_command.args(&filter_args);
        } else {
            // The config didn't specify a filter, so we don't need to try to pass one.
            clone_command.args(["--backing-store", "filteredhg"]);
        }
    }

    // Pass additional eden clone args from config if specified
    // Use shlex to properly parse args that may contain spaces or quotes
    if let Ok(Some(eden_clone_args)) = config.get_opt::<String>("edenfs", "eden-clone-args") {
        if let Some(args) = shlex::split(&eden_clone_args) {
            for arg in args {
                clone_command.arg(arg);
            }
        }
    }

    run_eden_clone_command(&mut clone_command).context("error performing eden clone")
}

/// Copy user-specific EdenFS config from a source checkout to a new one.
///
/// Copies the following sections from the source's config.toml into the
/// destination's config.toml:
///   - [redirections]: user-specific redirections.
///     Repo-level redirections from .eden-redirections are already
///     applied by the clone and don't need copying.
///   - [profiles]: active prefetch profiles (e.g. "edenfs").
///   - [predictive-prefetch]: predictive prefetch settings.
///
/// After writing, runs `eden redirect fixup` to apply redirections.
///
/// NOTE: If `eden redirect fixup` proves unreliable, an alternative approach
/// would be to call `eden redirect add` for each redirection individually
/// instead of writing config.toml and running `eden redirect fixup`.
#[cfg(feature = "eden")]
pub fn copy_eden_user_config(
    config: &dyn Config,
    source_client_dir: &Path,
    dest_mount: &Path,
) -> Result<()> {
    let source_config_path = source_client_dir.join("config.toml");
    let source_content = fs::read_to_string(&source_config_path)
        .with_context(|| format!("failed to read {}", source_config_path.display()))?;
    let source_table: toml::Table = source_content
        .parse()
        .with_context(|| format!("failed to parse {}", source_config_path.display()))?;

    let source_redirections = source_table.get("redirections").and_then(|v| v.as_table());
    let source_profiles = source_table.get("profiles").and_then(|v| v.as_table());
    let source_predictive = source_table
        .get("predictive-prefetch")
        .and_then(|v| v.as_table());

    let has_redirections = source_redirections.is_some_and(|t| !t.is_empty());
    let has_profiles = source_profiles.is_some_and(|t| !t.is_empty());
    let has_predictive = source_predictive.is_some_and(|t| !t.is_empty());

    if !has_redirections && !has_profiles && !has_predictive {
        return Ok(());
    }

    // Resolve the new worktree's client directory.
    // Use edenfs_client::get_client_dir which handles platform differences:
    // - Unix: reads .eden/client symlink
    // - Windows: parses .eden/config TOML for Config.client
    let dest_client_dir = edenfs_client::get_client_dir(dest_mount)?;
    let dest_config_path = dest_client_dir.join("config.toml");

    let dest_content = fs::read_to_string(&dest_config_path)
        .with_context(|| format!("failed to read {}", dest_config_path.display()))?;
    let mut dest_table: toml::Table = dest_content
        .parse()
        .with_context(|| format!("failed to parse {}", dest_config_path.display()))?;

    if let Some(redirections) = source_redirections {
        dest_table.insert(
            "redirections".to_string(),
            toml::Value::Table(redirections.clone()),
        );
    }
    if let Some(profiles) = source_profiles {
        dest_table.insert("profiles".to_string(), toml::Value::Table(profiles.clone()));
    }
    if let Some(predictive) = source_predictive {
        dest_table.insert(
            "predictive-prefetch".to_string(),
            toml::Value::Table(predictive.clone()),
        );
    }

    let new_content = toml::to_string(&dest_table).context("failed to serialize config.toml")?;
    atomic_write(&dest_config_path, |f| f.write_all(new_content.as_bytes()))?;

    // Apply redirections by running `eden redirect fixup`
    if has_redirections {
        let mut cmd = edenfs_client::build_eden_command(config)?;
        cmd.args(["redirect", "fixup", "--mount"]);
        cmd.arg(dest_mount);
        let output = cmd.output().with_context(|| {
            format!(
                "failed to run eden redirect fixup for {}",
                dest_mount.display()
            )
        })?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!(
                "eden redirect fixup failed for {}: {}",
                dest_mount.display(),
                stderr.trim()
            );
        }
    }

    Ok(())
}

/// Copy the sparse/filter config from a source checkout to a new one.
///
/// If the source checkout has a `sparse` file in its dot directory,
/// copies it to the destination so the new checkout has the same
/// active filters.
pub fn copy_sparse_config(source_dot_dir: &Path, dest_dot_dir: &Path) -> Result<()> {
    let sparse_path = source_dot_dir.join("sparse");
    if sparse_path.exists() {
        let dest_sparse = dest_dot_dir.join("sparse");
        fs::copy(&sparse_path, &dest_sparse).with_context(|| {
            format!("failed to copy sparse config to {}", dest_sparse.display())
        })?;
    }
    Ok(())
}

/// Get the tag to use for streaming clone from config.
///
/// This is shared between wireproto and SLAPI streaming clone implementations.
pub fn get_streaming_clone_tag(config: &dyn Config) -> Option<String> {
    if let Some(tag) = config.get("stream_out_shallow", "tag") {
        return Some(tag.to_string());
    }
    if config
        .get_or_default::<bool>("stream_out_shallow", "auto")
        .unwrap_or(false)
    {
        if let Ok(names) =
            config.get_or::<Vec<String>>("remotenames", "selectivepulldefault", Vec::new)
        {
            if let Some(first) = names.first() {
                return Some(first.clone());
            }
        }
    }
    None
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamingCloneResult {
    /// Total bytes written to the index file (00changelog.i).
    pub index_bytes_written: u64,
    /// Total bytes written to the data file (00changelog.d).
    pub data_bytes_written: u64,
}

/// Perform streaming clone, writing changelog files to the given store path.
#[instrument(skip(api), err)]
pub fn streaming_clone_to_files(
    api: &(impl SaplingRemoteApi + ?Sized),
    store_path: &Path,
    tag: Option<String>,
) -> Result<StreamingCloneResult> {
    let response = block_on(api.streaming_clone(tag))?;

    let index_path = store_path.join("00changelog.i");
    let data_path = store_path.join("00changelog.d");

    let result = streaming_clone_inner(&index_path, &data_path, response);
    if result.is_err() {
        // Clean up partial files on error
        let _ = std::fs::remove_file(&index_path);
        let _ = std::fs::remove_file(&data_path);
    }
    result
}

fn streaming_clone_inner(
    index_path: &Path,
    data_path: &Path,
    response: edenapi::Response<edenapi_types::StreamingChangelogResponse>,
) -> Result<StreamingCloneResult> {
    let mut entries = stream_to_iter(response.entries);

    // First entry must be metadata
    let first_entry = entries
        .next()
        .ok_or_else(|| anyhow::anyhow!("Empty streaming clone response"))??;
    let (expected_index_size, expected_data_size) = match first_entry.data {
        Ok(StreamingChangelogData::Metadata(metadata)) => {
            tracing::info!(
                "Streaming clone: expecting {} bytes index, {} bytes data",
                metadata.index_size,
                metadata.data_size
            );
            (metadata.index_size, metadata.data_size)
        }
        Ok(_) => bail!("First streaming clone entry was not metadata"),
        Err(e) => return Err(e).context("Server error in streaming clone metadata"),
    };

    let total = expected_index_size + expected_data_size;
    let progress_bar = ProgressBar::new_adhoc("streaming changelog", total, "bytes");

    let mut index_file = fs::File::create(index_path)?;
    let mut data_file = fs::File::create(data_path)?;

    let mut index_bytes_written: u64 = 0;
    let mut data_bytes_written: u64 = 0;

    for entry in entries {
        let entry = entry?;
        match entry.data {
            Ok(StreamingChangelogData::Metadata(_)) => {
                bail!("Unexpected metadata entry in streaming clone stream");
            }
            Ok(StreamingChangelogData::IndexBlobChunk(blob)) => {
                let bytes = blob.chunk.as_ref();
                index_file
                    .write_all(bytes)
                    .context("Failed to write index chunk")?;
                index_bytes_written += bytes.len() as u64;
                progress_bar.increase_position(bytes.len() as u64);
            }
            Ok(StreamingChangelogData::DataBlobChunk(blob)) => {
                let bytes = blob.chunk.as_ref();
                data_file
                    .write_all(bytes)
                    .context("Failed to write data chunk")?;
                data_bytes_written += bytes.len() as u64;
                progress_bar.increase_position(bytes.len() as u64);
            }
            Err(e) => {
                return Err(e).context("Server error during streaming clone");
            }
        }
    }

    index_file.sync_all().context("Failed to sync index file")?;
    data_file.sync_all().context("Failed to sync data file")?;

    // Validate that the actual bytes written match the expected sizes from metadata
    if index_bytes_written != expected_index_size {
        bail!(
            "Streaming clone index size mismatch: expected {} bytes, but wrote {} bytes",
            expected_index_size,
            index_bytes_written
        );
    }
    if data_bytes_written != expected_data_size {
        bail!(
            "Streaming clone data size mismatch: expected {} bytes, but wrote {} bytes",
            expected_data_size,
            data_bytes_written
        );
    }

    Ok(StreamingCloneResult {
        index_bytes_written,
        data_bytes_written,
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fs;

    use tempfile::TempDir;

    use super::*;

    #[test]
    pub fn test_get_target_dir() -> Result<()> {
        let tmpdir = TempDir::new()?;
        let mut config: BTreeMap<String, String> = BTreeMap::new();

        // Test with non-set default destination directory
        assert_eq!(
            get_default_destination_directory(&config)?,
            env::current_dir()?.as_path()
        );

        // Test setting default destination directory
        let path = tmpdir.path().join("foo").join("bar");
        config.insert(
            "clone.default-destination-dir".to_string(),
            path.to_str().unwrap().to_string(),
        );
        assert_eq!(get_default_destination_directory(&config).unwrap(), path,);

        Ok(())
    }

    #[test]
    pub fn test_get_eden_backing_dir() -> Result<()> {
        let tmpdir = TempDir::new()?;
        let mut config: BTreeMap<String, String> = BTreeMap::new();
        let legacy_dir = tmpdir.path().join("legacy-dir");
        let new_dir = tmpdir.path().join("new-dir");
        config.insert(
            "edenfs.legacy-backing-repos-dir".to_string(),
            legacy_dir.to_string_lossy().to_string(),
        );
        config.insert(
            "edenfs.backing-repos-dir".to_string(),
            new_dir.to_string_lossy().to_string(),
        );
        // if legacy directory does not exist, use new directory
        assert_eq!(get_default_eden_backing_directory(&config)?, Some(new_dir),);
        fs::create_dir(legacy_dir.clone())?;
        // if legacy directory does exist, use legacy directory
        assert_eq!(
            get_default_eden_backing_directory(&config)?,
            Some(legacy_dir),
        );
        Ok(())
    }
}
