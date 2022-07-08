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

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Result;
use async_runtime::block_unless_interrupted as block_on;
use configmodel::config::Config;
use manifest_tree::Manifest;
use manifest_tree::TreeManifest;
use repo::repo::Repo;
use termlogger::TermLogger;
use tracing::instrument;
use treestate::treestate::TreeState;
use types::HgId;
use types::RepoPath;
use util::errors::IOContext;
use util::errors::IOError;
use util::file::atomic_write;
use util::path::absolute;
use util::path::create_shared_dir;
use util::path::expand_path;
use uuid::Uuid;

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

#[derive(Debug, thiserror::Error)]
pub enum WorkingCopyError {
    #[error("No such checkout target '{0}'")]
    NoSuchTarget(HgId),

    #[error(transparent)]
    Io(#[from] IOError),

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

#[instrument(skip(logger), err)]
pub fn init_working_copy(
    logger: &mut TermLogger,
    repo: &mut Repo,
    target: Option<HgId>,
    sparse_profiles: Vec<String>,
) -> Result<(), WorkingCopyError> {
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

    let target = match target {
        Some(t) => t,
        None => {
            // Nothing to check out - init empty dirstate and bail.
            let mut ts = open_treestate(repo.dot_hg_path())?;
            checkout::clone::flush_dirstate(
                repo.config(),
                &mut ts,
                repo.dot_hg_path(),
                types::hgid::NULL_ID,
            )?;
            return Ok(());
        }
    };

    let roots = repo.dag_commits()?.read().to_dyn_read_root_tree_ids();
    let tree_id = match block_on(roots.read_root_tree_ids(vec![target.clone()]))
        .io_context("error blocking on read_root_tree_ids")??
        .into_iter()
        .next()
    {
        Some((_, tree_id)) => tree_id,
        None => return Err(WorkingCopyError::NoSuchTarget(target)),
    };

    let tree_store = repo.tree_store()?;
    let file_store = repo.file_store()?;

    let source_mf = TreeManifest::ephemeral(tree_store.clone());
    let target_mf = TreeManifest::durable(tree_store.clone(), tree_id.clone());

    for profile in &sparse_profiles {
        let path = RepoPath::from_str(profile).map_err(|e| anyhow!(e))?;
        if matches!(target_mf.get(path)?, None) {
            logger.warn(format!(
                "The profile '{profile}' does not exist. Check out a commit where it exists, or remove it with 'hg sparse disableprofile'."
            ));
        }
    }

    let mut ts = open_treestate(repo.dot_hg_path())?;

    match checkout::clone::checkout(
        repo.config(),
        repo.path(),
        &source_mf,
        &target_mf,
        file_store.clone(),
        &mut ts,
        target,
    ) {
        Ok(stats) => {
            logger.status(format!("{}", stats));

            Ok(())
        }
        Err(err) => {
            if err.resumable {
                logger.status(format!(
                    "Checkout failed. Resume with '{} checkout --continue'",
                    logger.cli_name(),
                ));
            }

            Err(err.source.into())
        }
    }
}

fn open_treestate(dot_hg_path: &Path) -> Result<TreeState> {
    let ts_dir = dot_hg_path.join("treestate");
    create_shared_dir(&ts_dir)?;

    let ts_path = ts_dir.join(format!("{:x}", Uuid::new_v4()));
    TreeState::open(&ts_path, None)
}

#[derive(Debug, thiserror::Error)]
pub enum EdenCloneError {
    #[error("Failed cloning eden checkout\n Stdout: '{0}'\n Stderr: '{1}'")]
    ExeuctionFailure(String, String),
    #[error("edenfs.command config is not set")]
    MissingCommandConfig(),
}

#[instrument(err)]
pub fn eden_clone(backing_repo: &Repo, working_copy: &Path, target: Option<HgId>) -> Result<()> {
    let config = backing_repo.config();
    let eden_command = config.get_opt::<String>("edenfs", "command")?;
    let mut clone_command = match eden_command {
        Some(cmd) => Command::new(cmd),
        None => return Err(EdenCloneError::MissingCommandConfig().into()),
    };

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

    if let Some(rev) = target {
        clone_command.args(["-r", &rev.to_hex()]);
    } else {
        clone_command.arg("--allow-empty-repo");
    }

    tracing::info!(?clone_command, "running edenfsctl clone");

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
    Ok(())
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
