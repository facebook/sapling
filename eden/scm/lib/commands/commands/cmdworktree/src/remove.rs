/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeSet;
use std::collections::HashMap;
use std::io::BufRead;
use std::path::Path;
use std::path::PathBuf;

use clidispatch::ReqCtx;
use clidispatch::abort;
use cmdutil::Result;
use repo::repo::Repo;
use workingcopy::workingcopy::WorkingCopy;
use worktree::dissolve_group;
use worktree::load_registry;
use worktree::with_registry_lock;
use worktree::with_worktree_path_op_lock;

use crate::CurrentGroup;
use crate::WorktreeOpts;
use crate::require_group;

pub(crate) fn run(ctx: &ReqCtx<WorktreeOpts>, repo: &Repo, _wc: &WorkingCopy) -> Result<u8> {
    let current_group = require_group(repo)?;

    if ctx.opts.all {
        return run_remove_all(ctx, repo, &current_group);
    }

    let path_args = &ctx.opts.args[1..];
    if path_args.is_empty() {
        abort!("usage: sl worktree remove PATH [PATH...]");
    }

    let targets: BTreeSet<PathBuf> = path_args
        .iter()
        .map(|s| -> Result<PathBuf> {
            Ok(util::path::strip_unc_prefix(
                util::path::canonical_path_allow_missing(s)?,
            ))
        })
        .collect::<Result<_>>()?;
    let target_refs: Vec<&Path> = targets.iter().map(|p| p.as_path()).collect();
    validate_targets(&current_group, &target_refs)?;

    remove_and_update_registry(ctx, repo, &current_group, &target_refs)?;

    // NOTE: Add post-worktree-remove hook if the need arises. Note that the
    // hook's cwd (repo.path()) may not exist if the user removed the worktree
    // they were standing in (see D98226466).

    Ok(0)
}

/// Validate that all user-specified paths are removable linked worktrees.
fn validate_targets(current_group: &CurrentGroup, targets: &[&Path]) -> Result<()> {
    let registry = load_registry(&current_group.shared_store_path)?;
    let grp = match registry.groups.get(&current_group.group_id) {
        Some(group) => group,
        None => abort!("group '{}' not found in registry", current_group.group_id),
    };
    for target in targets {
        if !grp.worktrees.contains_key(*target) {
            if let Some(parent_wt) = grp.worktrees.keys().find(|wt| target.starts_with(wt)) {
                abort!(
                    "{} is not the root of checkout {}, not removing",
                    target.display(),
                    parent_wt.display()
                );
            }
            abort!(
                "{} is not in this worktree group, use `eden rm` instead",
                target.display()
            );
        }
        if *target == grp.main {
            abort!("cannot remove a main worktree with linked worktrees");
        }
    }
    Ok(())
}

/// Confirm, run hooks, tear down EdenFS mounts, and update the worktree registry.
fn remove_and_update_registry(
    ctx: &ReqCtx<WorktreeOpts>,
    repo: &Repo,
    current_group: &CurrentGroup,
    targets: &[&Path],
) -> Result<()> {
    if targets.is_empty() {
        return Ok(());
    }

    let logger = ctx.logger();

    confirm_remove(ctx, targets)?;

    let pre_hooks = hook::Hooks::from_config(repo.config(), ctx.io(), "pre-worktree-remove");
    for target in targets {
        with_worktree_path_op_lock(&current_group.shared_store_path, target, || {
            pre_hooks.run_hooks(
                Some(repo),
                true,
                Some(&HashMap::from([(
                    "path".to_string(),
                    target.display().to_string(),
                )])),
            )?;
            run_eden_remove(ctx, repo, target)?;
            Ok(())
        })?;
    }

    with_registry_lock(&current_group.shared_store_path, |registry| {
        let Some(grp) = registry.groups.get_mut(&current_group.group_id) else {
            return Ok(());
        };
        for target in targets {
            grp.worktrees.remove(*target);
        }
        let linked_count = grp.worktrees.keys().filter(|p| **p != grp.main).count();
        if linked_count == 0 {
            dissolve_group(registry, &current_group.group_id);
        }
        Ok(())
    })?;

    for target in targets {
        logger.info(format!("removed {}", target.display()));
    }

    Ok(())
}

fn run_remove_all(
    ctx: &ReqCtx<WorktreeOpts>,
    repo: &Repo,
    current_group: &CurrentGroup,
) -> Result<u8> {
    let logger = ctx.logger();
    let removed_paths = with_registry_lock(&current_group.shared_store_path, |registry| {
        let grp = match registry.groups.get_mut(&current_group.group_id) {
            Some(group) => group,
            None => abort!("group '{}' not found in registry", &current_group.group_id),
        };
        let linked_paths: Vec<PathBuf> = grp
            .worktrees
            .keys()
            .filter(|p| **p != grp.main)
            .cloned()
            .collect();

        if linked_paths.is_empty() {
            return Ok(Vec::new());
        }

        let path_refs: Vec<&Path> = linked_paths.iter().map(|p| p.as_path()).collect();
        confirm_remove(ctx, &path_refs)?;

        let pre_hooks = hook::Hooks::from_config(repo.config(), ctx.io(), "pre-worktree-remove");
        for path in &linked_paths {
            pre_hooks.run_hooks(
                Some(repo),
                true,
                Some(&HashMap::from([(
                    "path".to_string(),
                    path.display().to_string(),
                )])),
            )?;
        }

        for path in &linked_paths {
            run_eden_remove(ctx, repo, path)?;
            grp.worktrees.remove(path);
        }

        dissolve_group(registry, &current_group.group_id);
        Ok(linked_paths)
    })?;
    if removed_paths.is_empty() {
        logger.info("no linked worktrees to remove");
        return Ok(0);
    }
    for path in &removed_paths {
        logger.info(format!("removed {}", path.display()));
    }

    // NOTE: Add post-worktree-remove hook if the need arises. See single-remove
    // path above for cwd caveat.

    Ok(0)
}

/// Run `eden remove` for `path`. If the checkout directory is already gone
/// (e.g., removed externally via `eden rm`), skips the call and returns Ok
/// so the caller can proceed to clean the worktree registry.
fn run_eden_remove(ctx: &ReqCtx<WorktreeOpts>, repo: &Repo, path: &Path) -> Result<()> {
    if !path.exists() {
        ctx.logger().warn(format!(
            "eden checkout {} not found on disk, continuing to remove from registry",
            path.display()
        ));
        return Ok(());
    }
    edenfs_client::run_eden_remove(repo.config().as_ref(), path)
}

fn confirm_remove(ctx: &ReqCtx<WorktreeOpts>, paths: &[&Path]) -> Result<()> {
    if ctx.global_opts().noninteractive {
        return Ok(());
    }

    let is_interactive = ctx.io().with_input(|input| input.is_tty());
    if !is_interactive {
        abort!("running non-interactively, use -y instead");
    }

    if paths.len() == 1 {
        ctx.io()
            .write(format!("will remove {}\n", paths[0].display()))?;
    } else {
        ctx.io()
            .write(format!("will remove {} worktrees:\n", paths.len()))?;
        for p in paths {
            ctx.io().write(format!("  {}\n", p.display()))?;
        }
    }
    ctx.io().write("proceed? [y/N] ")?;
    ctx.io().flush()?;

    let mut input = ctx.io().input();
    let mut line = String::new();
    let mut reader = std::io::BufReader::new(&mut input);
    reader.read_line(&mut line)?;
    let answer = line.trim();
    if !(answer.eq_ignore_ascii_case("y") || answer.eq_ignore_ascii_case("yes")) {
        abort!("aborted by user");
    }
    Ok(())
}
