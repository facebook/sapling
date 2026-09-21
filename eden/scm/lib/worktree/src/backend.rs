/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::path::PathBuf;

use anyhow::Context;
use anyhow::Result;
use repo_minimal_info::RepoMinimalInfo;

use crate::dissolve_group;
use crate::dissolve_group_if_empty;
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

/// Access to the worktrees associated with one repository.
pub struct Worktrees {
    backend: Box<dyn WorktreeBackend>,
}

impl Worktrees {
    /// Open the worktree implementation appropriate for `repo`.
    pub fn open(repo: &RepoMinimalInfo) -> Result<Self> {
        if !repo.requirements.contains("eden") {
            anyhow::bail!("worktree commands require an EdenFS-backed repository");
        }

        Ok(Self {
            backend: Box::new(SaplingBackend {
                repo_path: repo.path.clone(),
                shared_store_path: repo.store_path.clone(),
            }),
        })
    }

    /// List the repository's worktrees using the backend's source of truth.
    pub fn list(&self) -> Result<Vec<ListedWorktree>> {
        self.backend.list()
    }
}
