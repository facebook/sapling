/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Worktree registry: data model and persistence for worktree groups.
//!
//! This crate manages the JSON registry that tracks which EdenFS-backed
//! working copies belong together in a worktree group. It is intentionally
//! kept separate from the `cmdworktree` command implementation so that
//! other crates (e.g., `clone`, Python bindings for smartlog) can access
//! worktree group information without pulling in command-layer dependencies.

use std::collections::BTreeMap;
use std::path::Path;
use std::path::PathBuf;

use anyhow::Context;
use anyhow::Result;
use fs_err as fs;
use serde::Deserialize;
use serde::Serialize;
use util::lock::PathLock;

// --- Data Model ---

#[derive(Serialize, Deserialize)]
pub struct Registry {
    pub version: u32,
    pub groups: BTreeMap<String, Group>,
}

#[derive(Serialize, Deserialize)]
pub struct Group {
    pub main: PathBuf,
    pub worktrees: BTreeMap<PathBuf, WorktreeEntry>,
}

#[derive(Serialize, Deserialize)]
pub struct WorktreeEntry {
    pub added: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

impl Registry {
    pub fn new() -> Self {
        Self {
            version: 1,
            groups: BTreeMap::new(),
        }
    }

    pub fn find_group_for_path(&self, path: &Path) -> Option<String> {
        self.groups
            .iter()
            .find(|(_, group)| group.worktrees.contains_key(path))
            .map(|(id, _)| id.clone())
    }
}

impl Group {
    pub fn new(main_path: PathBuf) -> Self {
        let mut worktrees = BTreeMap::new();
        worktrees.insert(
            main_path.clone(),
            WorktreeEntry {
                added: chrono::Utc::now().to_rfc3339(),
                label: None,
            },
        );
        Self {
            main: main_path,
            worktrees,
        }
    }
}

// --- Validation ---

/// Verify that `dest` is not inside an existing source control checkout.
///
/// Walks from `dest` up to the filesystem root, checking each ancestor for
/// SCM marker directories (`.hg`, `.sl`, `.git`, `.svn`). Non-existent
/// intermediates are skipped — only the marker check matters, since
/// `Path::join().exists()` already returns false when the parent doesn't exist.
pub fn check_dest_not_in_repo(dest: &Path) -> Result<()> {
    const SCM_MARKERS: &[&str] = &[".hg", ".sl", ".git", ".svn"];
    for parent in dest.ancestors().skip(1) {
        for marker in SCM_MARKERS {
            if parent.join(marker).exists() {
                anyhow::bail!(
                    "destination '{}' is inside an existing checkout at {}",
                    dest.display(),
                    parent.display()
                );
            }
        }
    }
    Ok(())
}

// --- Registry Persistence ---

pub fn load_registry(shared_store_path: &Path) -> Result<Registry> {
    let path = shared_store_path.join("worktrees.json");
    match fs::read_to_string(&path) {
        Ok(content) => {
            let registry: Registry = serde_json::from_str(&content)
                .with_context(|| format!("failed to parse registry at {}", path.display()))?;
            Ok(registry)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Registry::new()),
        Err(e) => Err(e.into()),
    }
}

pub fn save_registry(shared_store_path: &Path, registry: &Registry) -> Result<()> {
    let path = shared_store_path.join("worktrees.json");
    let content = serde_json::to_string_pretty(registry).context("failed to serialize registry")?;
    util::file::atomic_write(&path, |f| {
        use std::io::Write;
        f.write_all(content.as_bytes())
    })?;
    Ok(())
}

pub fn with_registry_lock<T>(
    shared_store_path: &Path,
    f: impl FnOnce(&mut Registry) -> Result<T>,
) -> Result<T> {
    let lock_path = shared_store_path.join("worktrees.lock");
    let _lock = PathLock::exclusive(&lock_path)?;
    let mut registry = load_registry(shared_store_path)?;
    let result = f(&mut registry)?;
    save_registry(shared_store_path, &registry)?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_new() {
        let reg = Registry::new();
        assert_eq!(reg.version, 1);
        assert!(reg.groups.is_empty());
    }

    #[test]
    fn test_group_new() {
        let main_path = PathBuf::from("/tmp/test_repo");
        let group = Group::new(main_path.clone());
        assert_eq!(group.main, main_path);
        assert_eq!(group.worktrees.len(), 1);
        let entry = group.worktrees.get(&main_path).unwrap();
        assert!(entry.label.is_none());
        assert!(!entry.added.is_empty());
    }

    // --- check_dest_not_in_repo tests ---

    #[test]
    fn test_check_dest_not_in_repo_clean() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("my_worktree");
        assert!(check_dest_not_in_repo(&dest).is_ok());
    }

    #[test]
    fn test_check_dest_not_in_repo_hg() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join(".hg")).unwrap();
        let dest = dir.path().join("sub").join("worktree");
        let err = check_dest_not_in_repo(&dest).unwrap_err();
        assert!(format!("{}", err).contains("inside an existing checkout"));
    }

    #[test]
    fn test_check_dest_not_in_repo_sl() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join(".sl")).unwrap();
        let dest = dir.path().join("worktree");
        let err = check_dest_not_in_repo(&dest).unwrap_err();
        assert!(format!("{}", err).contains("inside an existing checkout"));
    }

    #[test]
    fn test_check_dest_not_in_repo_git() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join(".git")).unwrap();
        let dest = dir.path().join("worktree");
        let err = check_dest_not_in_repo(&dest).unwrap_err();
        assert!(format!("{}", err).contains("inside an existing checkout"));
    }

    // --- Registry tests ---

    #[test]
    fn test_load_registry_missing() {
        let dir = tempfile::tempdir().unwrap();
        let reg = load_registry(dir.path()).unwrap();
        assert_eq!(reg.version, 1);
        assert!(reg.groups.is_empty());
    }

    #[test]
    fn test_save_load_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let mut reg = Registry::new();
        let main_path = PathBuf::from("/tmp/main_repo");
        let mut group = Group::new(main_path.clone());
        let linked_path = PathBuf::from("/tmp/linked_wt");
        group.worktrees.insert(
            linked_path.clone(),
            WorktreeEntry {
                added: "2025-01-01T00:00:00Z".to_string(),
                label: Some("feature-x".to_string()),
            },
        );
        reg.groups.insert("test-group-id".to_string(), group);

        save_registry(dir.path(), &reg).unwrap();
        let loaded = load_registry(dir.path()).unwrap();

        assert_eq!(loaded.version, 1);
        assert_eq!(loaded.groups.len(), 1);
        let grp = loaded.groups.get("test-group-id").unwrap();
        assert_eq!(grp.main, main_path);
        assert_eq!(grp.worktrees.len(), 2);
        let linked_entry = grp.worktrees.get(&linked_path).unwrap();
        assert_eq!(linked_entry.label.as_deref(), Some("feature-x"));
        assert_eq!(linked_entry.added, "2025-01-01T00:00:00Z");
    }

    #[test]
    fn test_check_dest_not_in_repo_svn() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join(".svn")).unwrap();
        let dest = dir.path().join("worktree");
        let err = check_dest_not_in_repo(&dest).unwrap_err();
        assert!(format!("{}", err).contains("inside an existing checkout"));
    }

    #[test]
    fn test_check_dest_not_in_repo_deeply_nested() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join(".git")).unwrap();
        let dest = dir.path().join("a").join("b").join("c").join("d");
        let err = check_dest_not_in_repo(&dest).unwrap_err();
        assert!(format!("{}", err).contains("inside an existing checkout"));
        assert!(format!("{}", err).contains(&dir.path().display().to_string()));
    }

    #[test]
    fn test_check_dest_not_in_repo_root_level() {
        // Destination at filesystem root should succeed (no SCM markers above)
        let dest = PathBuf::from("/tmp/some_unique_worktree_test_path");
        assert!(check_dest_not_in_repo(&dest).is_ok());
    }

    // --- find_group_for_path tests ---

    #[test]
    fn test_find_group_for_path_found() {
        let mut reg = Registry::new();
        let main_path = PathBuf::from("/tmp/main");
        let linked_path = PathBuf::from("/tmp/linked");
        let mut group = Group::new(main_path.clone());
        group.worktrees.insert(
            linked_path.clone(),
            WorktreeEntry {
                added: "2025-01-01T00:00:00Z".to_string(),
                label: None,
            },
        );
        reg.groups.insert("group-1".to_string(), group);

        assert_eq!(
            reg.find_group_for_path(&main_path),
            Some("group-1".to_string())
        );
        assert_eq!(
            reg.find_group_for_path(&linked_path),
            Some("group-1".to_string())
        );
    }

    #[test]
    fn test_find_group_for_path_not_found() {
        let mut reg = Registry::new();
        let main_path = PathBuf::from("/tmp/main");
        reg.groups
            .insert("group-1".to_string(), Group::new(main_path));

        let unknown = PathBuf::from("/tmp/unknown");
        assert!(reg.find_group_for_path(&unknown).is_none());
    }

    #[test]
    fn test_find_group_for_path_multiple_groups() {
        let mut reg = Registry::new();
        let main_a = PathBuf::from("/tmp/repo_a");
        let main_b = PathBuf::from("/tmp/repo_b");
        reg.groups
            .insert("group-a".to_string(), Group::new(main_a.clone()));
        reg.groups
            .insert("group-b".to_string(), Group::new(main_b.clone()));

        assert_eq!(
            reg.find_group_for_path(&main_a),
            Some("group-a".to_string())
        );
        assert_eq!(
            reg.find_group_for_path(&main_b),
            Some("group-b".to_string())
        );
    }

    // --- with_registry_lock error propagation ---

    #[test]
    fn test_with_registry_lock_error_does_not_persist() {
        let dir = tempfile::tempdir().unwrap();
        let result: anyhow::Result<()> = with_registry_lock(dir.path(), |registry| {
            registry.groups.insert(
                "should-not-persist".to_string(),
                Group::new(PathBuf::from("/tmp/x")),
            );
            anyhow::bail!("simulated failure");
        });
        assert!(result.is_err());

        // Registry should still be empty since the closure failed.
        let loaded = load_registry(dir.path()).unwrap();
        assert!(loaded.groups.is_empty());
    }

    // --- Other tests ---

    #[test]
    fn test_load_registry_malformed() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("worktrees.json"), "not valid json!!!").unwrap();
        let result = load_registry(dir.path());
        assert!(result.is_err());
    }

    #[test]
    fn test_with_registry_lock() {
        let dir = tempfile::tempdir().unwrap();
        with_registry_lock(dir.path(), |registry| {
            let main_path = PathBuf::from("/tmp/lock_test");
            registry
                .groups
                .insert("lock-group".to_string(), Group::new(main_path));
            Ok(())
        })
        .unwrap();

        // Verify changes were persisted.
        let loaded = load_registry(dir.path()).unwrap();
        assert_eq!(loaded.groups.len(), 1);
        assert!(loaded.groups.contains_key("lock-group"));
    }
}
