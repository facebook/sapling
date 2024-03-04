/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::env;
use std::ffi::OsStr;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

use anyhow::Context;
use anyhow::Result;
use configmodel::Config;
use configmodel::ConfigExt;
use context::CoreContext;
use repo::repo::Repo;
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
        let wc = wc.lock()?;

        if let Err(err) = checkout::checkout(
            ctx,
            repo,
            &wc,
            target,
            checkout::BookmarkAction::None,
            checkout::CheckoutMode::AbortIfConflicts,
            checkout::ReportMode::Minimal,
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

#[derive(Debug, thiserror::Error)]
pub enum EdenCloneError {
    #[error("Failed cloning eden checkout\n Stdout: '{0}'\n Stderr: '{1}'")]
    ExeuctionFailure(String, String),
    #[error("edenfs.command config is not set")]
    MissingCommandConfig(),
}

fn get_eden_clone_command(config: &dyn Config) -> Result<Command> {
    let eden_command = config.get_opt::<String>("edenfs", "command")?;
    match eden_command {
        Some(cmd) => Ok(Command::new(cmd)),
        None => Err(EdenCloneError::MissingCommandConfig().into()),
    }
}

#[tracing::instrument]
fn run_eden_clone_command(clone_command: &mut Command) -> Result<()> {
    let output = clone_command
        .output()
        .with_context(|| format!("failed to execute {:?}", clone_command))?;

    if !output.status.success() {
        return Err(EdenCloneError::ExeuctionFailure(
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

#[instrument(err)]
pub fn eden_clone(backing_repo: &Repo, working_copy: &Path, target: Option<HgId>) -> Result<()> {
    let config = backing_repo.config();

    let mut clone_command = get_eden_clone_command(config)?;

    // allow tests to specify different configuration directories from prod defaults
    if let Some(base_dir) = config.get_opt::<PathBuf>("edenfs", "basepath")? {
        clone_command.args([
            "--config-dir".into(),
            base_dir.join("eden"),
            "--etc-eden-dir".into(),
            base_dir.join("etc_eden"),
            "--home-dir".into(),
            base_dir.join("home"),
        ]);
    }

    clone_command.args([
        OsStr::new("clone"),
        backing_repo.path().as_os_str(),
        working_copy.as_os_str(),
    ]);

    let enable_windows_symlinks = if let Ok(enabled_everywhere) =
        config.get_or_default::<bool>("experimental", "windows-symlinks")
    {
        enabled_everywhere
    } else {
        config
            .get_or_default::<Vec<String>>("experimental", "windows-symlinks")?
            .contains(&"edenfs".to_owned())
    };
    if enable_windows_symlinks {
        clone_command.args(["--enable-windows-symlinks".to_string()]);
    }

    if let Some(rev) = target {
        clone_command.args(["-r", &rev.to_hex()]);
    } else {
        clone_command.arg("--allow-empty-repo");
    }

    // The old config value was a bool while the new config value is a String. We need to support
    // both values until we can deprecate the old one.
    let eden_sparse_filter = match config.must_get::<String>("clone", "eden-sparse-filter") {
        // A non-empty string means we should activate this filter after cloning the repo.
        // An empty string means the repo should be cloned without activating a filter.
        Ok(val) => Some(val),
        Err(_) => {
            // If the old config value is true, we should use eden sparse but not activate a filter
            if config.get_or_default::<bool>("clone", "use-eden-sparse")? {
                Some("".to_owned())
            } else {
                // Otherwise we don't want to use eden sparse or activate any filters
                None
            }
        }
    };

    // The current Eden installation may not yet support the --filter-path option. We will back-up
    // the clone arguments and retry without --filter-path if our first clone attempt fails.
    let args_without_filter = match eden_sparse_filter {
        Some(filter) if !filter.is_empty() => {
            clone_command.args(["--backing-store", "filteredhg"]);
            let args_without_filter = clone_command
                .get_args()
                .map(|v| v.to_os_string())
                .collect::<Vec<_>>();
            clone_command.args(["--filter-path", &filter]);
            Some(args_without_filter)
        }
        Some(_) => {
            // The config didn't specify a filter, so we don't need to try to pass one.
            clone_command.args(["--backing-store", "filteredhg"]);
            None
        }
        // We aren't cloning with FilteredFS at all. No need to retry the clone if it fails.
        None => None,
    };

    run_eden_clone_command(&mut clone_command).or_else(|e| {
        // Retry the clone without the --filter-path argument
        if let Some(args_without_filter) = args_without_filter {
            let mut new_command = get_eden_clone_command(config)?;
            new_command.args(args_without_filter);
            tracing::debug!(target: "clone_info", empty_eden_filter=true);
            run_eden_clone_command(&mut new_command)?;
            Ok(())
        } else {
            Err(e)
        }
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
