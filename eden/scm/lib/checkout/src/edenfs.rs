/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::fs::read_to_string;
use std::io::Write;
#[cfg(unix)]
use std::os::unix::prelude::MetadataExt;
use std::process::Command;
use std::sync::Arc;

use anyhow::bail;
use anyhow::Context;
use anyhow::Result;
use configmodel::Config;
use configmodel::ConfigExt;
use edenfs_client::CheckoutConflict;
use manifest::Manifest;
use pathmatcher::AlwaysMatcher;
use repo::repo::Repo;
use spawn_ext::CommandExt;
use termlogger::TermLogger;
use treestate::filestate::StateFlags;
use types::hgid::NULL_ID;
use types::HgId;
use types::RepoPath;
use workingcopy::util::walk_treestate;
use workingcopy::workingcopy::LockedWorkingCopy;
use workingcopy::workingcopy::WorkingCopy;

use crate::actions::changed_metadata_to_action;
use crate::actions::Action;
use crate::actions::UpdateAction;
use crate::check_conflicts;
use crate::errors::EdenConflictError;
use crate::ActionMap;
use crate::Checkout;
use crate::CheckoutMode;
use crate::CheckoutPlan;

fn actionmap_from_eden_conflicts(
    config: &dyn Config,
    source_manifest: &impl Manifest,
    target_manifest: &impl Manifest,
    conflicts: Vec<edenfs_client::CheckoutConflict>,
) -> Result<ActionMap> {
    let mut map = HashMap::new();
    for conflict in conflicts {
        let action = match conflict.conflict_type {
            edenfs_client::ConflictType::Error => {
                abort_on_eden_conflict_error(config, vec![conflict.clone()])?;
                None
            }
            edenfs_client::ConflictType::UntrackedAdded
            | edenfs_client::ConflictType::RemovedModified => {
                let conflict_path = conflict.path.as_repo_path();
                let meta = target_manifest.get_file(conflict_path)?.context(format!(
                    "file metadata for {} not found at destination commit",
                    conflict_path
                ))?;
                Some(Action::Update(UpdateAction::new(None, meta)))
            }
            edenfs_client::ConflictType::ModifiedRemoved => Some(Action::Remove),
            edenfs_client::ConflictType::ModifiedModified => {
                let conflict_path = conflict.path.as_repo_path();
                let old_meta = source_manifest.get_file(conflict_path)?.context(format!(
                    "file metadata for {} not found at source commit",
                    conflict_path
                ))?;
                let new_meta = target_manifest.get_file(conflict_path)?.context(format!(
                    "file metadata for {} not found at target commit",
                    conflict_path
                ))?;
                changed_metadata_to_action(old_meta, new_meta)
            }
            edenfs_client::ConflictType::MissingRemoved
            | edenfs_client::ConflictType::DirectoryNotEmpty => None,
        };
        if let Some(action) = action {
            map.insert(conflict.path, action);
        }
    }
    Ok(ActionMap { map })
}

pub fn edenfs_checkout(
    lgr: &TermLogger,
    repo: &mut Repo,
    wc: &LockedWorkingCopy,
    target_commit: HgId,
    checkout_mode: CheckoutMode,
) -> anyhow::Result<()> {
    // TODO (sggutier): try to unify these steps with the non-edenfs version of checkout
    let target_commit_tree_hash = repo.tree_resolver()?.get_root_id(&target_commit)?;

    // Perform the actual checkout depending on the mode
    match checkout_mode {
        CheckoutMode::Force => {
            edenfs_force_checkout(repo, wc, target_commit, target_commit_tree_hash)?
        }
        CheckoutMode::NoConflict => {
            edenfs_noconflict_checkout(lgr, repo, wc, target_commit, target_commit_tree_hash)?
        }
        CheckoutMode::Merge => bail!("native merge checkout not yet supported for EdenFS"),
    };

    // Update the treestate and parents with the new changes
    wc.set_parents(vec![target_commit], Some(target_commit_tree_hash))?;
    wc.treestate().lock().flush()?;
    // Clear the update state
    let updatestate_path = wc.dot_hg_path().join("updatestate");
    util::file::unlink_if_exists(updatestate_path)?;
    // Run EdenFS specific "hooks"
    edenfs_redirect_fixup(lgr, repo.config(), wc)?;
    Ok(())
}

fn create_edenfs_plan(
    wc: &WorkingCopy,
    config: &dyn Config,
    source_manifest: &impl Manifest,
    target_manifest: &impl Manifest,
    conflicts: Vec<edenfs_client::CheckoutConflict>,
) -> Result<CheckoutPlan> {
    let vfs = wc.vfs();
    let actionmap =
        actionmap_from_eden_conflicts(config, source_manifest, target_manifest, conflicts)?;
    let checkout = Checkout::from_config(vfs.clone(), &config)?;
    Ok(checkout.plan_action_map(actionmap))
}

fn edenfs_noconflict_checkout(
    lgr: &TermLogger,
    repo: &mut Repo,
    wc: &LockedWorkingCopy,
    target_commit: HgId,
    target_commit_tree_hash: HgId,
) -> anyhow::Result<()> {
    let current_commit = wc.parents()?.into_iter().next().unwrap_or(NULL_ID);
    let tree_resolver = repo.tree_resolver()?;
    let source_mf = tree_resolver.get(&current_commit)?;
    let target_mf = tree_resolver.get(&target_commit)?;

    // Do a dry run to check if there will be any conflicts before modifying any actual files
    let conflicts = wc.eden_client()?.checkout(
        target_commit,
        target_commit_tree_hash,
        edenfs_client::CheckoutMode::DryRun,
    )?;
    let plan = create_edenfs_plan(wc, repo.config(), &source_mf, &target_mf, conflicts)?;

    let status = wc.status(Arc::new(AlwaysMatcher::new()), false, repo.config(), lgr)?;

    check_conflicts(lgr, repo, wc, &plan, &target_mf, &status)?;

    // Signal that an update is being performed
    let updatestate_path = wc.dot_hg_path().join("updatestate");
    util::file::atomic_write(&updatestate_path, |f| {
        write!(f, "{}", target_commit.to_hex())
    })?;

    // Do the actual checkout
    let actual_conflicts = wc.eden_client()?.checkout(
        target_commit,
        target_commit_tree_hash,
        edenfs_client::CheckoutMode::Normal,
    )?;
    abort_on_eden_conflict_error(repo.config(), actual_conflicts)?;

    // Execute the plan, applying changes to conflicting-ish files
    let apply_result = plan.apply_store(repo.file_store()?.as_ref())?;
    for (path, err) in apply_result.remove_failed {
        lgr.warn(format!("update failed to remove {}: {:#}!\n", path, err));
    }

    Ok(())
}

fn edenfs_force_checkout(
    repo: &mut Repo,
    wc: &LockedWorkingCopy,
    target_commit: HgId,
    target_commit_tree_hash: HgId,
) -> anyhow::Result<()> {
    // Try to run checkout on EdenFS on force mode, then check for network errors
    let conflicts = wc.eden_client()?.checkout(
        target_commit,
        target_commit_tree_hash,
        edenfs_client::CheckoutMode::Force,
    )?;
    abort_on_eden_conflict_error(repo.config(), conflicts)?;

    wc.clear_merge_state()?;

    // Tell EdenFS to forget about all changes in the working copy
    clear_edenfs_dirstate(wc)?;

    Ok(())
}

fn clear_edenfs_dirstate(wc: &LockedWorkingCopy) -> anyhow::Result<()> {
    let tbind = wc.treestate();
    let mut treestate = tbind.lock();
    let matcher = Arc::new(AlwaysMatcher::new());
    let mask = StateFlags::EXIST_P1 | StateFlags::EXIST_P2 | StateFlags::EXIST_NEXT;
    let mut tracked = Vec::new();
    walk_treestate(
        &mut treestate,
        matcher,
        StateFlags::empty(),
        mask,
        StateFlags::empty(),
        |path, _state| {
            tracked.push(path);
            Ok(())
        },
    )?;
    for path in tracked {
        treestate.remove(path.as_byte_slice())?;
    }
    Ok(())
}

/// run `edenfsctl redirect fixup`, potentially in background.
///
/// If the `.eden-redirections` file does not exist in the working copy,
/// or is empty, run nothing.
///
/// Otherwise, parse the fixup directories, if they exist and look okay,
/// run `edenfsctl redirect fixup` in background. This reduces overhead
/// especially on Windows.
///
/// Otherwise, run in foreground. This is needed for automation that relies
/// on `checkout HASH` to setup critical repo redirections.
pub fn edenfs_redirect_fixup(
    lgr: &TermLogger,
    config: &dyn Config,
    wc: &WorkingCopy,
) -> anyhow::Result<()> {
    let is_okay = match is_edenfs_redirect_okay(wc).unwrap_or(Some(false)) {
        Some(r) => r,
        None => return Ok(()),
    };
    let arg0 = config.get_or("edenfs", "command", || "edenfsctl".to_owned())?;
    let args_raw = config.get_or("edenfs", "redirect-fixup", || "redirect fixup".to_owned())?;
    let args = args_raw.split_whitespace().collect::<Vec<_>>();
    let mut cmd0 = Command::new(arg0);
    let cmd = cmd0.args(args);
    if is_okay {
        cmd.spawn_detached()?;
    } else {
        lgr.io().disable_progress(true)?;
        let status = cmd.status();
        lgr.io().disable_progress(false)?;
        status?;
    }
    Ok(())
}

/// Whether the edenfs redirect directories look okay, or None if redirect is unnecessary.
fn is_edenfs_redirect_okay(wc: &WorkingCopy) -> anyhow::Result<Option<bool>> {
    let vfs = wc.vfs();
    let mut redirections = HashMap::new();

    // Check edenfs-client/src/redirect.rs for the config paths and file format.
    let client_paths = vec![
        wc.vfs().root().join(".eden-redirections"),
        wc.eden_client()?.client_path().join("config.toml"),
    ];

    for path in client_paths {
        // Cannot use vfs::read as config.toml is outside of the working copy
        let text = match read_to_string(path) {
            Ok(s) => s,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => {
                tracing::debug!("is_edenfs_redirect_okay failed to check: {}", e);
                return Ok(Some(false));
            }
        };
        if let Ok(s) = toml::from_str::<toml::Table>(text.as_str()) {
            if let Some(r) = s.get("redirections").and_then(|v| v.as_table()) {
                for (k, v) in r.iter() {
                    redirections.insert(k.to_owned(), v.to_string());
                }
            }
        }
    }

    if redirections.is_empty() {
        return Ok(None);
    }

    #[cfg(unix)]
    let root_device_inode = vfs.metadata(RepoPath::empty())?.dev();
    for (path, kind) in redirections.into_iter() {
        let path_metadata = match vfs.metadata(RepoPath::from_str(path.as_str())?) {
            Ok(m) => m,
            Err(_) => continue,
        };
        if cfg!(windows) || kind == "symlink" {
            // kind is "bind" or "symlink". On Windows, "bind" is not supported
            if !path_metadata.is_symlink() {
                return Ok(Some(false));
            }
        } else {
            #[cfg(unix)]
            // Bind mount should have a different device inode
            if path_metadata.dev() == root_device_inode {
                return Ok(Some(false));
            }
        }
    }

    Ok(Some(true))
}

/// abort if there is a ConflictType.ERROR type of conflicts
pub fn abort_on_eden_conflict_error(
    config: &dyn Config,
    conflicts: Vec<CheckoutConflict>,
) -> Result<(), EdenConflictError> {
    if !config
        .get_or_default::<bool>("experimental", "abort-on-eden-conflict-error")
        .unwrap_or_default()
    {
        return Ok(());
    }
    for conflict in conflicts {
        if edenfs_client::ConflictType::Error == conflict.conflict_type {
            hg_metrics::increment_counter("abort_on_eden_conflict_error", 1);
            return Err(EdenConflictError {
                path: conflict.path.into_string(),
                message: conflict.message,
            });
        }
    }
    Ok(())
}
