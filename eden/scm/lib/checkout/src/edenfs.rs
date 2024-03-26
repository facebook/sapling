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
use context::CoreContext;
use manifest::Manifest;
use pathmatcher::AlwaysMatcher;
use repo::repo::Repo;
use spawn_ext::CommandExt;
use status::Status;
use status::StatusBuilder;
use termlogger::TermLogger;
use treestate::filestate::StateFlags;
use types::workingcopy_client::CheckoutConflict;
use types::workingcopy_client::CheckoutMode;
use types::workingcopy_client::ConflictType;
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
use crate::CheckoutPlan;

fn actionmap_from_eden_conflicts(
    config: &dyn Config,
    wc: &WorkingCopy,
    source_manifest: &impl Manifest,
    target_manifest: &impl Manifest,
    conflicts: Vec<CheckoutConflict>,
) -> Result<(ActionMap, Status)> {
    let mut modified = Vec::new();
    let mut removed = Vec::new();
    let mut added = Vec::new();
    let mut unknown = Vec::new();
    let treestate_binding = wc.treestate();
    let mut treestate = treestate_binding.lock();

    let mut map = HashMap::new();
    for conflict in conflicts {
        let action = match conflict.conflict_type {
            ConflictType::Error => {
                abort_on_eden_conflict_error(config, vec![conflict.clone()])?;
                None
            }
            ConflictType::UntrackedAdded | ConflictType::RemovedModified => {
                let conflict_path = conflict.path.as_repo_path();
                if conflict.conflict_type == ConflictType::UntrackedAdded {
                    let file_state = treestate
                        .normalized_get(conflict_path.as_str().as_bytes())?
                        .map_or(StateFlags::empty(), |f| f.state);
                    if file_state.intersects(StateFlags::EXIST_NEXT) {
                        // This means that the file was added, since it's
                        // visible in the treestate but EdenFS sees it as
                        // untracked
                        added.push(conflict_path.to_owned());
                    } else if !wc
                        .ignore_matcher
                        .match_relative(conflict_path.to_path().as_path(), false)
                    {
                        // If the treestate doesn't see the file, it means that
                        // the file is either ignored or untracked. There are
                        // some particular edge cases when we want to treat
                        // unknown files as special during checkout
                        unknown.push(conflict_path.to_owned());
                    }
                } else if let Some(file_state) =
                    treestate.normalized_get(conflict_path.as_str().as_bytes())?
                {
                    if file_state
                        .state
                        .intersects(StateFlags::EXIST_P1 | StateFlags::EXIST_P2)
                    {
                        // Seems like this code path never gets hit, but let's handle it anyways
                        removed.push(conflict_path.to_owned());
                    }
                }
                let meta = target_manifest.get_file(conflict_path)?.context(format!(
                    "file metadata for {} not found at destination commit",
                    conflict_path
                ))?;
                Some(Action::Update(UpdateAction::new(None, meta)))
            }
            ConflictType::ModifiedRemoved => {
                let conflict_path = conflict.path.as_repo_path();
                modified.push(conflict_path.to_owned());
                Some(Action::Remove)
            }
            ConflictType::ModifiedModified => {
                let conflict_path = conflict.path.as_repo_path();
                modified.push(conflict_path.to_owned());
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
            ConflictType::MissingRemoved | ConflictType::DirectoryNotEmpty => None,
        };
        if let Some(action) = action {
            map.insert(conflict.path, action);
        }
    }

    // This will generate something mostly equivalent to what one gets
    // with the status command
    let mut status_builder = StatusBuilder::new();
    status_builder = status_builder.modified(modified);
    status_builder = status_builder.removed(removed);
    status_builder = status_builder.added(added);
    status_builder = status_builder.unknown(unknown);

    Ok((ActionMap { map }, status_builder.build()))
}

pub fn edenfs_checkout(
    ctx: &CoreContext,
    repo: &Repo,
    wc: &LockedWorkingCopy,
    target_commit: HgId,
    revert_conflicts: bool,
) -> anyhow::Result<()> {
    // TODO (sggutier): try to unify these steps with the non-edenfs version of checkout
    let target_commit_tree_hash = repo.tree_resolver()?.get_root_id(&target_commit)?;

    // Perform the actual checkout depending on the mode
    if revert_conflicts {
        edenfs_force_checkout(repo, wc, target_commit, target_commit_tree_hash)?
    } else {
        edenfs_noconflict_checkout(ctx, repo, wc, target_commit, target_commit_tree_hash)?
    }

    // Update the treestate and parents with the new changes
    if fail::eval("checkout-pre-set-parents", |_| ()).is_some() {
        bail!("Error set by checkout-pre-set-parents FAILPOINTS");
    }

    wc.set_parents(vec![target_commit], Some(target_commit_tree_hash))?;
    wc.treestate().lock().flush()?;
    // Clear the update state
    let updatestate_path = wc.dot_hg_path().join("updatestate");
    util::file::unlink_if_exists(updatestate_path)?;
    // Run EdenFS specific "hooks"
    edenfs_redirect_fixup(&ctx.logger, repo.config(), wc)?;
    Ok(())
}

fn create_edenfs_plan(
    wc: &WorkingCopy,
    config: &dyn Config,
    source_manifest: &impl Manifest,
    target_manifest: &impl Manifest,
    conflicts: Vec<CheckoutConflict>,
) -> Result<(CheckoutPlan, Status)> {
    let vfs = wc.vfs();
    let (actionmap, status) =
        actionmap_from_eden_conflicts(config, wc, source_manifest, target_manifest, conflicts)?;
    let checkout = Checkout::from_config(vfs.clone(), &config)?;
    Ok((checkout.plan_action_map(actionmap), status))
}

fn edenfs_noconflict_checkout(
    ctx: &CoreContext,
    repo: &Repo,
    wc: &LockedWorkingCopy,
    target_commit: HgId,
    target_commit_tree_hash: HgId,
) -> anyhow::Result<()> {
    let current_commit = wc.first_parent()?;
    let tree_resolver = repo.tree_resolver()?;
    let source_mf = tree_resolver.get(&current_commit)?;
    let target_mf = tree_resolver.get(&target_commit)?;

    // Do a dry run to check if there will be any conflicts before modifying any actual files
    let conflicts = wc.working_copy_client()?.checkout(
        target_commit,
        target_commit_tree_hash,
        CheckoutMode::DryRun,
    )?;
    let (plan, status) = create_edenfs_plan(wc, repo.config(), &source_mf, &target_mf, conflicts)?;

    check_conflicts(repo, wc, &plan, &target_mf, &status)?;

    // Signal that an update is being performed
    let updatestate_path = wc.dot_hg_path().join("updatestate");
    util::file::atomic_write(&updatestate_path, |f| {
        write!(f, "{}", target_commit.to_hex())
    })?;

    // Do the actual checkout
    let actual_conflicts = wc.working_copy_client()?.checkout(
        target_commit,
        target_commit_tree_hash,
        CheckoutMode::Normal,
    )?;
    abort_on_eden_conflict_error(repo.config(), actual_conflicts)?;

    // Execute the plan, applying changes to conflicting-ish files
    let apply_result = plan.apply_store(repo.file_store()?.as_ref())?;
    for (path, err) in apply_result.remove_failed {
        ctx.logger
            .warn(format!("update failed to remove {}: {:#}!\n", path, err));
    }

    Ok(())
}

fn edenfs_force_checkout(
    repo: &Repo,
    wc: &LockedWorkingCopy,
    target_commit: HgId,
    target_commit_tree_hash: HgId,
) -> anyhow::Result<()> {
    // Try to run checkout on EdenFS on force mode, then check for network errors
    let conflicts = wc.working_copy_client()?.checkout(
        target_commit,
        target_commit_tree_hash,
        CheckoutMode::Force,
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

    let client = wc.working_copy_client()?;
    let client = match client
        .as_any()
        .downcast_ref::<edenfs_client::EdenFsClient>()
    {
        Some(v) => v,
        None => anyhow::bail!("bug: edenfs_redirect called on non-eden working copy"),
    };

    // Check edenfs-client/src/redirect.rs for the config paths and file format.
    let client_paths = vec![
        wc.vfs().root().join(".eden-redirections"),
        client.client_path().join("config.toml"),
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
        if ConflictType::Error == conflict.conflict_type {
            hg_metrics::increment_counter("abort_on_eden_conflict_error", 1);
            return Err(EdenConflictError {
                path: conflict.path.into_string(),
                message: conflict.message,
            });
        }
    }
    Ok(())
}
