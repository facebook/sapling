/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::BTreeSet;
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

use crate::Group;
use crate::Registry;
use crate::Reservations;
use crate::WORKTREE_NAME_FILE;
use crate::WorktreeEntry;
use crate::dissolve_group;
use crate::dissolve_group_if_empty;
use crate::group_id_for_main_path;
use crate::load_registry;
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
    /// `$GIT_COMMON_DIR/sl/store`, which holds the worktree operation locks.
    shared_store_path: PathBuf,
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

    fn check_add(&self, dest: &Path) -> Result<(PathBuf, usize)> {
        let registry = load_registry(&self.shared_store_path)?;
        check_unregistered(&registry, dest)?;
        let main =
            resolve_group_for_main_path(&registry, &canonicalize_or_original(&self.repo_path));
        let linked = count_linked_worktrees(&registry, &main);
        Ok((main, linked))
    }

    fn register(
        &self,
        main: &Path,
        dest: &Path,
        label: Option<&str>,
    ) -> Result<Option<anyhow::Error>> {
        with_registry_lock(&self.shared_store_path, |registry| {
            check_unregistered(registry, dest)?;
            let group_id = resolve_group_id_for_main_path(registry, main);
            // The limit was enforced at reservation time; deliberately no re-check
            // here, since re-checking after the clone is exactly what could strand
            // this checkout on disk unregistered.
            registry
                .groups
                .entry(group_id)
                .or_insert_with(|| Group::new(main.to_owned()))
                .worktrees
                .insert(
                    dest.to_owned(),
                    WorktreeEntry {
                        added: chrono::Utc::now().to_rfc3339(),
                        label: label.map(str::to_owned),
                    },
                );
            // Written under the registry lock, as in `set_label`.
            Ok(
                write_worktree_name_marker(dest, &self.ident.resolve_full_dot_dir(dest), label)
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

    fn check_add(&self, _dest: &Path) -> Result<(PathBuf, usize)> {
        let worktrees = self.git.list_worktrees()?;
        let main = worktrees
            .iter()
            .find(|worktree| worktree.is_main)
            .context("Git did not report a main worktree")?;
        let main = canonicalize_or_original(&main.path);
        let linked = worktrees
            .iter()
            .filter(|worktree| !worktree.is_main)
            .count();
        Ok((main, linked))
    }

    fn occupied_slots(&self, reservations: &Reservations, main: &Path) -> Result<usize> {
        // Git lists a new checkout before its reservation is released; count it once.
        let linked = self
            .git
            .list_worktrees()?
            .into_iter()
            .filter(|worktree| !worktree.is_main)
            .map(|worktree| canonicalize_or_original(&worktree.path));
        let reserved = reservations
            .reservations
            .values()
            .filter(|reservation| reservation.group_main == main)
            .map(|reservation| reservation.dest.clone());
        Ok(linked.chain(reserved).collect::<BTreeSet<_>>().len())
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
                shared_store_path: repo.store_path.clone(),
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

    /// Directory holding the registry, reservations, and operation locks.
    pub fn shared_store_path(&self) -> PathBuf {
        match &self.backend {
            WorktreeBackend::Eden(backend) => backend.shared_store_path.clone(),
            WorktreeBackend::Git(backend) => backend.shared_store_path.clone(),
        }
    }

    /// Reject registered destinations and return the group's main checkout and
    /// linked-worktree count from one snapshot. The count excludes reservations;
    /// enforce limits with `occupied_slots` under the reservation lock.
    pub fn check_add(&self, dest: &Path) -> Result<(PathBuf, usize)> {
        match &self.backend {
            WorktreeBackend::Eden(backend) => backend.check_add(dest),
            WorktreeBackend::Git(backend) => backend.check_add(dest),
        }
    }

    /// Count linked checkouts of `main` plus `reservations`. To enforce a limit,
    /// call this inside [`crate::with_reservations`] after pruning expired
    /// reservations.
    pub fn occupied_slots(
        &self,
        registry: &Registry,
        reservations: &Reservations,
        main: &Path,
    ) -> Result<usize> {
        match &self.backend {
            WorktreeBackend::Eden(_) => {
                Ok(count_linked_worktrees(registry, main) + reservations.count_for_group(main))
            }
            WorktreeBackend::Git(backend) => backend.occupied_slots(reservations, main),
        }
    }

    /// Record a newly created checkout. `Ok(Some(error))` means the checkout is
    /// usable, but its `worktreename` file could not be written.
    pub fn register(
        &self,
        main: &Path,
        dest: &Path,
        label: Option<&str>,
    ) -> Result<Option<anyhow::Error>> {
        match &self.backend {
            WorktreeBackend::Eden(backend) => backend.register(main, dest, label),
            WorktreeBackend::Git(backend) => {
                Ok(write_git_label(&backend.ident.resolve_full_dot_dir(dest), label).err())
            }
        }
    }

    /// The Git repository, if this is a native Git checkout.
    pub fn git(&self) -> Option<&RepoGit> {
        match &self.backend {
            WorktreeBackend::Eden(_) => None,
            WorktreeBackend::Git(backend) => Some(&backend.git),
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

fn check_unregistered(registry: &Registry, dest: &Path) -> Result<()> {
    if registry.find_group_for_path(dest).is_some() {
        anyhow::bail!(
            "destination path '{}' is already registered as a worktree",
            dest.display()
        );
    }
    Ok(())
}

fn resolve_group_for_main_path(registry: &Registry, canonical_repo_path: &Path) -> PathBuf {
    registry
        .find_group_for_path(canonical_repo_path)
        .and_then(|group_id| {
            registry
                .groups
                .get(&group_id)
                .map(|group| group.main.clone())
        })
        .unwrap_or_else(|| canonical_repo_path.to_path_buf())
}

fn resolve_group_id_for_main_path(registry: &Registry, group_main_path: &Path) -> String {
    registry
        .find_group_for_path(group_main_path)
        .unwrap_or_else(|| group_id_for_main_path(group_main_path))
}

fn count_linked_worktrees(registry: &Registry, group_main_path: &Path) -> usize {
    registry
        .find_group_for_path(group_main_path)
        .and_then(|group_id| registry.groups.get(&group_id))
        .map(Group::linked_worktree_count)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_group_id_for_main_path_reuses_legacy_group_id() {
        let mut registry = Registry::new();
        let main = PathBuf::from("/tmp/main");
        let linked = PathBuf::from("/tmp/linked");
        let legacy_group_id = "legacy-random-group-id".to_string();

        let mut group = Group::new(main.clone());
        group.worktrees.insert(
            linked.clone(),
            WorktreeEntry {
                added: "2025-01-01T00:00:00Z".to_string(),
                label: None,
            },
        );
        registry.groups.insert(legacy_group_id.clone(), group);

        let group_main = resolve_group_for_main_path(&registry, &linked);

        assert_eq!(group_main, main);
        assert_eq!(
            resolve_group_id_for_main_path(&registry, &group_main),
            legacy_group_id
        );
    }
}
