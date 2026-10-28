/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::io;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;

use anyhow::Context;
use anyhow::Result;
use configmodel::Config;
use configmodel::ConfigExt;
use fs_err as fs;
use gitcompat::RepoGit;
use identity::Identity;
use repo_minimal_info::RepoMinimalInfo;

use crate::WORKTREE_NAME_FILE;
use crate::dissolve_group;
use crate::dissolve_group_if_empty;
use crate::with_registry_lock;
use crate::write_worktree_name_marker;

#[derive(Debug, Eq, PartialEq)]
/// A worktree presented through the backend-independent listing API.
pub struct ListedWorktree {
    /// Absolute path to the working tree.
    pub path: PathBuf,
    /// Whether this is the repository's main working tree.
    pub is_main: bool,
    /// Optional Sapling label associated with this working tree.
    pub label: Option<String>,
    /// Whether this is the working tree from which the command was run.
    pub current: bool,
}

enum WorktreeBackend {
    Eden(EdenBackend),
    Git(GitBackend),
}

struct EdenBackend {
    repo_path: PathBuf,
    shared_store_path: PathBuf,
    ident: Identity,
}

struct GitBackend {
    repo_path: PathBuf,
    git: RepoGit,
    ident: Identity,
}

impl EdenBackend {
    fn list(&self) -> Result<Vec<ListedWorktree>> {
        let current = util::path::strip_unc_prefix(
            util::path::canonicalize_best_effort(&self.repo_path)
                .with_context(|| format!("failed to canonicalize {}", self.repo_path.display()))?,
        );

        with_registry_lock(&self.shared_store_path, |registry| {
            let Some(group_id) = registry.find_group_for_path(&current) else {
                return Ok(Vec::new());
            };

            let Some(group) = registry.groups.get(&group_id) else {
                anyhow::bail!("worktree group '{group_id}' disappeared from the registry");
            };

            if !group.main.exists() {
                dissolve_group(registry, &group_id);
                return Ok(Vec::new());
            }

            if group.worktrees.keys().any(|path| !path.exists()) {
                let Some(group) = registry.groups.get_mut(&group_id) else {
                    anyhow::bail!("worktree group '{group_id}' disappeared from the registry");
                };
                group.worktrees.retain(|path, _| path.exists());
                dissolve_group_if_empty(registry, &group_id);
            }

            Ok(registry
                .groups
                .get(&group_id)
                .map(|group| {
                    group
                        .worktrees
                        .iter()
                        .map(|(path, entry)| ListedWorktree {
                            path: path.clone(),
                            is_main: *path == group.main,
                            label: entry.label.clone(),
                            current: *path == current,
                        })
                        .collect()
                })
                .unwrap_or_default())
        })
        .context("failed to list EdenFS worktrees")
    }

    fn set_label(&self, target: &Path, label: Option<&str>) -> Result<Option<anyhow::Error>> {
        let current = canonicalize_or_original(&self.repo_path);
        with_registry_lock(&self.shared_store_path, |registry| {
            let Some(group_id) = registry.find_group_for_path(&current) else {
                anyhow::bail!("this worktree is not part of a group");
            };
            let Some(entry) = registry
                .groups
                .get_mut(&group_id)
                .and_then(|group| group.worktrees.get_mut(target))
            else {
                anyhow::bail!("'{}' is not in this worktree group", target.display());
            };
            entry.label = label.map(str::to_owned);

            // The registry is the source of truth; the marker only lets external
            // tools such as `scm-prompt.sh` avoid reading it. Writing it under the
            // registry lock keeps the two in step.
            Ok(
                write_worktree_name_marker(target, &self.ident.resolve_full_dot_dir(target), label)
                    .err(),
            )
        })
    }
}

impl GitBackend {
    fn list(&self) -> Result<Vec<ListedWorktree>> {
        let current = canonicalize_or_original(&self.repo_path);

        self.git
            .list_worktrees()
            .context("failed to list Git worktrees")?
            .into_iter()
            .map(|worktree| {
                let canonical_path = canonicalize_or_original(&worktree.path);
                Ok(ListedWorktree {
                    label: read_git_label(&self.ident.resolve_full_dot_dir(&worktree.path))?,
                    path: worktree.path,
                    is_main: worktree.is_main,
                    current: canonical_path == current,
                })
            })
            .collect::<Result<Vec<_>>>()
    }

    fn set_label(&self, target: &Path, label: Option<&str>) -> Result<()> {
        let is_member = self
            .git
            .list_worktrees()?
            .iter()
            .any(|worktree| canonicalize_or_original(&worktree.path) == target);
        if !is_member {
            anyhow::bail!("'{}' is not in this worktree group", target.display());
        }
        write_git_label(&self.ident.resolve_full_dot_dir(target), label)
    }
}

/// Access to the worktrees associated with one repository.
pub struct Worktrees {
    backend: WorktreeBackend,
}

impl Worktrees {
    /// Open the worktree implementation appropriate for `repo`.
    pub fn open(repo: &RepoMinimalInfo, config: &dyn Config) -> Result<Self> {
        let is_git =
            repo.requirements.contains("dotgit") && repo.store_requirements.contains("git-store");
        let backend = if is_git {
            if !config
                .get_or("worktree", "git-enabled", || true)
                .context("invalid worktree.git-enabled configuration")?
            {
                anyhow::bail!("worktree commands for git are disabled by worktree.git-enabled");
            }
            WorktreeBackend::Git(GitBackend {
                repo_path: repo.path.clone(),
                git: RepoGit::from_root_and_config(repo.path.clone(), config),
                ident: repo.ident,
            })
        } else if repo.requirements.contains("eden") {
            WorktreeBackend::Eden(EdenBackend {
                repo_path: repo.path.clone(),
                shared_store_path: repo.store_path.clone(),
                ident: repo.ident,
            })
        } else {
            anyhow::bail!(
                "worktree commands require an EdenFS-backed repository or a native Git repository"
            );
        };

        Ok(Self { backend })
    }

    /// List the repository's worktrees using the backend's source of truth.
    pub fn list(&self) -> Result<Vec<ListedWorktree>> {
        match &self.backend {
            WorktreeBackend::Eden(backend) => backend.list(),
            WorktreeBackend::Git(backend) => backend.list(),
        }
    }

    /// Set or remove the label of `target`, a canonical member path.
    /// `Ok(Some(error))` means the label was saved but the EdenFS marker file read
    /// by `scm-prompt.sh` could not be written; callers should warn and continue.
    pub fn set_label(&self, target: &Path, label: Option<&str>) -> Result<Option<anyhow::Error>> {
        match &self.backend {
            WorktreeBackend::Eden(backend) => backend.set_label(target, label),
            WorktreeBackend::Git(backend) => backend.set_label(target, label).map(|()| None),
        }
    }
}

/// Git labels live in each checkout's admin directory, which Git keeps attached
/// to the checkout across moves and deletes with it, so a label cannot outlive
/// or detach from its checkout.
fn read_git_label(dot_dir: &Path) -> Result<Option<String>> {
    match fs::read_to_string(dot_dir.join(WORKTREE_NAME_FILE)) {
        Ok(label) => Ok(Some(label).filter(|label| !label.is_empty())),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

/// Unlike the EdenFS marker, a missing file means no label; the prompt then
/// falls back to Git's worktree name.
fn write_git_label(dot_dir: &Path, label: Option<&str>) -> Result<()> {
    let path = dot_dir.join(WORKTREE_NAME_FILE);
    match label {
        Some(label) => {
            // Not `create_dir_all`: a concurrent `git worktree remove` deletes the
            // admin directory, and recreating it would leave junk at the removed path.
            match fs::create_dir(dot_dir) {
                Err(error) if error.kind() != io::ErrorKind::AlreadyExists => {
                    return Err(error.into());
                }
                _ => {}
            }
            // Written atomically so `list` never reads a partial marker.
            util::file::atomic_write(&path, |file| file.write_all(label.as_bytes()))?;
        }
        None => match fs::remove_file(&path) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error.into()),
            _ => {}
        },
    }
    Ok(())
}

fn canonicalize_or_original(path: &Path) -> PathBuf {
    util::path::canonicalize_best_effort(path)
        .map(util::path::strip_unc_prefix)
        .unwrap_or_else(|_| path.to_owned())
}
