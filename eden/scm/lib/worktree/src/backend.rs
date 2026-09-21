/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::BTreeMap;
use std::path::Path;
use std::path::PathBuf;

use anyhow::Context;
use anyhow::Result;
use configmodel::Config;
use configmodel::ConfigExt;
use gitcompat::RepoGit;
use repo_minimal_info::RepoMinimalInfo;

use crate::dissolve_group;
use crate::dissolve_group_if_empty;
use crate::load_registry;
use crate::with_registry_lock;

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

trait WorktreeBackend {
    fn list(&self) -> Result<Vec<ListedWorktree>>;
}

struct SaplingBackend {
    repo_path: PathBuf,
    shared_store_path: PathBuf,
}

struct GitBackend {
    repo_path: PathBuf,
    git: RepoGit,
}

impl WorktreeBackend for SaplingBackend {
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
        .context("failed to list Sapling worktrees")
    }
}

impl WorktreeBackend for GitBackend {
    fn list(&self) -> Result<Vec<ListedWorktree>> {
        let shared_store_path = self.git.common_dir().join("sl").join("store");
        let labels = load_labels(&shared_store_path)?;
        let current = canonicalize_or_original(&self.repo_path);

        Ok(self
            .git
            .list_worktrees()
            .context("failed to list Git worktrees")?
            .into_iter()
            .map(|worktree| {
                let canonical_path = canonicalize_or_original(&worktree.path);
                ListedWorktree {
                    path: worktree.path,
                    is_main: worktree.is_main,
                    label: labels.get(&canonical_path).cloned(),
                    current: canonical_path == current,
                }
            })
            .collect())
    }
}

/// Access to the worktrees associated with one repository.
pub struct Worktrees {
    backend: Box<dyn WorktreeBackend>,
}

impl Worktrees {
    /// Open the worktree implementation appropriate for `repo`.
    pub fn open(repo: &RepoMinimalInfo, config: &dyn Config) -> Result<Self> {
        let is_git =
            repo.requirements.contains("dotgit") && repo.store_requirements.contains("git-store");
        let backend: Box<dyn WorktreeBackend> = if is_git {
            if !config
                .get_or("worktree", "git-enabled", || true)
                .context("invalid worktree.git-enabled configuration")?
            {
                anyhow::bail!("worktree commands for git are disabled by worktree.git-enabled");
            }
            Box::new(GitBackend {
                repo_path: repo.path.clone(),
                git: RepoGit::from_root_and_config(repo.path.clone(), config),
            })
        } else if repo.requirements.contains("eden") {
            Box::new(SaplingBackend {
                repo_path: repo.path.clone(),
                shared_store_path: repo.store_path.clone(),
            })
        } else {
            anyhow::bail!("worktree commands require an EdenFS-backed repository");
        };

        Ok(Self { backend })
    }

    /// List the repository's worktrees using the backend's source of truth.
    pub fn list(&self) -> Result<Vec<ListedWorktree>> {
        self.backend.list()
    }
}

fn load_labels(shared_store_path: &Path) -> Result<BTreeMap<PathBuf, String>> {
    Ok(load_registry(shared_store_path)
        .with_context(|| {
            format!(
                "failed to load worktree labels from {}",
                shared_store_path.display()
            )
        })?
        .groups
        .into_values()
        .flat_map(|group| group.worktrees)
        .filter_map(|(path, entry)| {
            entry
                .label
                .map(|label| (canonicalize_or_original(&path), label))
        })
        .collect())
}

fn canonicalize_or_original(path: &Path) -> PathBuf {
    util::path::canonicalize_best_effort(path)
        .map(util::path::strip_unc_prefix)
        .unwrap_or_else(|_| path.to_owned())
}
