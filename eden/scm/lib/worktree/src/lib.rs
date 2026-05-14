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
use std::fmt::Write as _;
#[cfg(unix)]
use std::os::unix::ffi::OsStrExt as _;
#[cfg(target_os = "macos")]
use std::os::unix::fs::MetadataExt as _;
#[cfg(windows)]
use std::os::windows::ffi::OsStrExt as _;
use std::path::Path;
use std::path::PathBuf;

use anyhow::Context;
use anyhow::Result;
use blake2::Blake2s256;
use blake2::Digest;
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

const GROUP_ID_NAMESPACE: &[u8] = b"group-id";
const WORKTREE_OP_LOCK_NAMESPACE: &[u8] = b"worktree-op-lock";

// These derived names must be stable outside a single process: group IDs are
// written to the registry, and lock names are used for on-disk coordination.
// Callers are expected to pass the same canonical path spelling they use when
// reading or writing the registry; this helper only does lexical cleanup
// (`.`, `..`, duplicate separators, `\\?\` stripping), not symlink resolution.
// The ids intentionally model destination-path identity before the checkout
// exists, so they follow host path semantics rather than the checkout's future
// case-sensitive/case-insensitive mount setting.
//
// `OsStr::as_encoded_bytes()` is only documented for round-tripping within the
// same Rust version and target platform:
// https://doc.rust-lang.org/std/ffi/struct.OsStr.html#method.as_encoded_bytes
// Convert to a stable platform representation before hashing.
#[cfg(all(unix, not(target_os = "macos")))]
fn update_stable_path_bytes(hasher: &mut Blake2s256, path: &Path) {
    hasher.update(path.as_os_str().as_bytes());
}

#[cfg(windows)]
fn update_stable_path_bytes(hasher: &mut Blake2s256, path: &Path) {
    // EdenFS clone/config path handling on Windows follows the platform's
    // usual case-insensitive path identity, so case-only spelling differences
    // should coordinate on the same derived id.
    let normalized = path.to_string_lossy().to_lowercase();
    for unit in normalized.encode_utf16() {
        hasher.update(unit.to_le_bytes());
    }
}

#[cfg(target_os = "macos")]
fn update_case_folded_unix_path_bytes(hasher: &mut Blake2s256, path: &Path) {
    for chunk in path.as_os_str().as_bytes().utf8_chunks() {
        if !chunk.valid().is_empty() {
            let normalized = chunk.valid().to_lowercase();
            hasher.update(normalized.as_bytes());
        }
        if !chunk.invalid().is_empty() {
            hasher.update(chunk.invalid());
        }
    }
}

#[cfg(target_os = "macos")]
fn detect_case_sensitive_existing_ancestor(path: &Path) -> Result<bool> {
    let mut probe = util::path::absolute(path)?;
    while !probe.exists() {
        if !probe.pop() {
            return Ok(true);
        }
    }
    detect_case_sensitive(&probe)
}

#[cfg(target_os = "macos")]
fn detect_case_sensitive(path: &Path) -> Result<bool> {
    let original = path.symlink_metadata()?;
    let Some(path_str) = path.to_str() else {
        return Ok(true);
    };
    let lowercase = path_str.to_lowercase();
    let case_variant = if lowercase != path_str {
        lowercase
    } else {
        let uppercase = path_str.to_uppercase();
        if uppercase == path_str {
            return Ok(true);
        }
        uppercase
    };
    let variant = match Path::new(&case_variant).symlink_metadata() {
        Ok(metadata) => metadata,
        Err(_) => return Ok(true),
    };
    Ok(original.dev() != variant.dev() || original.ino() != variant.ino())
}

#[cfg(target_os = "macos")]
fn update_stable_path_bytes(hasher: &mut Blake2s256, path: &Path) {
    if detect_case_sensitive_existing_ancestor(path).unwrap_or(true) {
        hasher.update(path.as_os_str().as_bytes());
    } else {
        update_case_folded_unix_path_bytes(hasher, path);
    }
}

#[cfg(not(any(unix, windows)))]
fn update_stable_path_bytes(hasher: &mut Blake2s256, path: &Path) {
    let normalized = path.to_string_lossy();
    hasher.update(normalized.as_bytes());
}

// Build an opaque deterministic identifier from a path. The identifier is
// process-independent so multiple racing commands can derive the same group id
// or per-path lock file name before touching the registry.
fn stable_path_id(domain: &[u8], path: &Path) -> String {
    let normalized_path = util::path::strip_unc_prefix(util::path::normalize(path));
    let mut hasher = Blake2s256::new();
    hasher.update(domain);
    hasher.update([0]);
    // Hash a path representation we control rather than Rust's opaque
    // OsStr encoding, since these ids are persisted and used cross-process.
    update_stable_path_bytes(&mut hasher, &normalized_path);

    // Truncate to 128 bits to keep ids compact while preserving opaque, deterministic names.
    let digest = hasher.finalize();
    let mut id = String::with_capacity(32);
    for byte in &digest[..16] {
        write!(&mut id, "{byte:02x}").expect("writing to String cannot fail");
    }
    id
}

/// Derive the registry group id from the canonical main worktree path.
///
/// The first `worktree add` that creates a group and any concurrent racers must
/// independently pick the same id without first consulting shared state.
pub fn group_id_for_main_path(main_path: &Path) -> String {
    stable_path_id(GROUP_ID_NAMESPACE, main_path)
}

fn worktree_path_lockfile_name(worktree_path: &Path) -> String {
    // Keep the lock name under the shared store so callers can coordinate on a
    // target path before the worktree itself exists on disk.
    format!(
        "worktree-op-{}.lock",
        stable_path_id(WORKTREE_OP_LOCK_NAMESPACE, worktree_path)
    )
}

pub fn lock_worktree_path_op(shared_store_path: &Path, worktree_path: &Path) -> Result<PathLock> {
    let lock_path = shared_store_path.join(worktree_path_lockfile_name(worktree_path));
    Ok(PathLock::exclusive(lock_path)?)
}

/// Hold the per-worktree operation lock for `worktree_path` while running `f`.
///
/// Intended composition:
/// 1. Take this lock around the long-running filesystem / EdenFS operation for
///    a specific worktree path.
/// 2. Enter `with_registry_lock()` only for the short read-modify-write of
///    `worktrees.json`.
pub fn with_worktree_path_op_lock<T>(
    shared_store_path: &Path,
    worktree_path: &Path,
    f: impl FnOnce() -> Result<T>,
) -> Result<T> {
    let _lock = lock_worktree_path_op(shared_store_path, worktree_path)?;
    f()
}

// --- Worktree-name marker ---

/// Filename of the per-worktree name marker, written into the worktree's dot
/// directory (e.g., `.sl/worktreename`).
///
/// Read by external tools (notably `eden/scm/contrib/scm-prompt.sh`) to display
/// the worktree's name in the shell prompt without consulting the registry.
const WORKTREE_NAME_FILE: &str = "worktreename";

/// Compute what the worktree name marker should contain: the label if non-empty,
/// otherwise the basename of the worktree path.
fn worktree_name_marker_content(worktree_path: &Path, label: Option<&str>) -> String {
    label
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .unwrap_or_else(|| {
            worktree_path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default()
        })
}

/// Write the worktree-name marker file at `<worktree_dot_dir>/worktreename`.
///
/// `worktree_path` is the canonical path of the worktree's working copy root
/// (used for the basename fallback when `label` is `None` or empty).
pub fn write_worktree_name_marker(
    worktree_path: &Path,
    worktree_dot_dir: &Path,
    label: Option<&str>,
) -> Result<()> {
    let content = worktree_name_marker_content(worktree_path, label);
    let path = worktree_dot_dir.join(WORKTREE_NAME_FILE);
    fs::write(&path, &content)
        .with_context(|| format!("failed to write worktree-name marker at {}", path.display()))?;
    Ok(())
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

pub fn dissolve_group(registry: &mut Registry, group_id: &str) {
    registry.groups.remove(group_id);
}

/// Lock the registry file, load it, run `f`, and write back the result.
///
/// This lock is intentionally coarse and should stay scoped to the
/// `worktrees.json` read-modify-write sequence. Callers that need to serialize
/// longer operations for a specific worktree path should do that with
/// `with_worktree_path_op_lock()` and then use this helper only for the final
/// registry update.
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
    #[cfg(unix)]
    use std::ffi::OsStr;

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

    #[cfg(not(windows))]
    #[test]
    fn test_group_id_for_main_path_is_stable() {
        let path = Path::new("/tmp/main");
        let id1 = group_id_for_main_path(path);
        let id2 = group_id_for_main_path(path);
        let other = group_id_for_main_path(Path::new("/tmp/other"));

        assert_eq!(id1, "a1931b83ce3c37d2c69e776d3436433f");
        assert_eq!(id1, id2);
        assert_ne!(id1, other);
    }

    #[test]
    fn test_group_id_for_main_path_normalizes_equivalent_spellings() {
        assert_eq!(
            group_id_for_main_path(Path::new("/tmp/repo/./main")),
            group_id_for_main_path(Path::new("/tmp/repo/main"))
        );
        assert_eq!(
            group_id_for_main_path(Path::new(r"\\?\C:\src\repo\main")),
            group_id_for_main_path(Path::new(r"C:\src\repo\main"))
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_group_id_for_main_path_preserves_non_utf8_bytes() {
        let path1 = Path::new(OsStr::from_bytes(b"/tmp/nonutf8-\x80"));
        let path2 = Path::new(OsStr::from_bytes(b"/tmp/nonutf8-\x81"));

        assert_ne!(group_id_for_main_path(path1), group_id_for_main_path(path2));
    }

    #[cfg(windows)]
    #[test]
    fn test_group_id_for_main_path_windows_drive_path_is_stable() {
        let path = Path::new(r"C:\src\repo\main");
        let id1 = group_id_for_main_path(path);
        let id2 = group_id_for_main_path(path);

        assert_eq!(id1, "0c365461676d023bda366cb242059c49");
        assert_eq!(id1, id2);
    }

    #[cfg(windows)]
    #[test]
    fn test_group_id_for_main_path_windows_paths_are_distinct() {
        let drive_path = Path::new(r"C:\src\repo\main");
        let other_drive_path = Path::new(r"D:\src\repo\main");
        let unc_path = Path::new(r"\\server\share\repo\main");

        assert_eq!(
            group_id_for_main_path(unc_path),
            "d6e031f169842e78da167212078603de"
        );
        assert_ne!(
            group_id_for_main_path(drive_path),
            group_id_for_main_path(other_drive_path)
        );
        assert_ne!(
            group_id_for_main_path(drive_path),
            group_id_for_main_path(unc_path)
        );
    }

    #[cfg(windows)]
    #[test]
    fn test_group_id_for_main_path_windows_case_is_folded() {
        assert_eq!(
            group_id_for_main_path(Path::new(r"C:\Src\Repo\Main")),
            group_id_for_main_path(Path::new(r"c:\src\repo\main"))
        );
    }

    #[test]
    fn test_lock_worktree_path_op_creates_lock_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = PathBuf::from("/tmp/some_worktree");
        let _lock = lock_worktree_path_op(dir.path(), &path).unwrap();

        assert!(dir.path().join(worktree_path_lockfile_name(&path)).exists());
    }

    #[test]
    fn test_lock_worktree_path_op_different_paths_concurrent() {
        let dir = tempfile::tempdir().unwrap();
        let path1 = PathBuf::from("/tmp/worktree1");
        let path2 = PathBuf::from("/tmp/worktree2");

        let _lock1 = lock_worktree_path_op(dir.path(), &path1).unwrap();
        let _lock2 = lock_worktree_path_op(dir.path(), &path2).unwrap();
    }

    #[cfg(not(windows))]
    #[test]
    fn test_worktree_path_lockfile_name_is_stable() {
        let path = Path::new("/tmp/consistent_path");
        let name1 = worktree_path_lockfile_name(path);
        let name2 = worktree_path_lockfile_name(path);
        let other = worktree_path_lockfile_name(Path::new("/tmp/other_path"));

        assert_eq!(name1, "worktree-op-418ede8060a3d4aa0723dc1ea9046340.lock");
        assert_eq!(name1, name2);
        assert_ne!(name1, other);
    }

    #[test]
    fn test_worktree_path_lockfile_name_normalizes_equivalent_spellings() {
        assert_eq!(
            worktree_path_lockfile_name(Path::new("/tmp/repo/linked/../linked")),
            worktree_path_lockfile_name(Path::new("/tmp/repo/linked"))
        );
        assert_eq!(
            worktree_path_lockfile_name(Path::new(r"\\?\C:\src\repo\linked")),
            worktree_path_lockfile_name(Path::new(r"C:\src\repo\linked"))
        );
    }

    #[cfg(windows)]
    #[test]
    fn test_worktree_path_lockfile_name_windows_path_is_stable() {
        let path = Path::new(r"C:\src\repo\linked");
        let name1 = worktree_path_lockfile_name(path);
        let name2 = worktree_path_lockfile_name(path);

        assert_eq!(name1, "worktree-op-7cfc9b010bb34f6dfe81f449cbe366e8.lock");
        assert_eq!(name1, name2);
        assert!(name1.starts_with("worktree-op-"));
        assert!(name1.ends_with(".lock"));
    }

    #[cfg(windows)]
    #[test]
    fn test_worktree_path_lockfile_name_windows_paths_are_distinct() {
        let drive_path = Path::new(r"C:\src\repo\linked");
        let unc_path = Path::new(r"\\server\share\repo\linked");

        assert_eq!(
            worktree_path_lockfile_name(unc_path),
            "worktree-op-e35b48634bbcac02385a23320201f492.lock"
        );
        assert_ne!(
            worktree_path_lockfile_name(drive_path),
            worktree_path_lockfile_name(unc_path)
        );
    }

    #[cfg(windows)]
    #[test]
    fn test_worktree_path_lockfile_name_windows_case_is_folded() {
        assert_eq!(
            worktree_path_lockfile_name(Path::new(r"C:\Src\Repo\Linked")),
            worktree_path_lockfile_name(Path::new(r"c:\src\repo\linked"))
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

    #[test]
    fn test_dissolve_group() {
        let mut registry = Registry::new();
        let main_path = PathBuf::from("/tmp/main_repo");
        let mut group = Group::new(main_path.clone());
        let linked_path = PathBuf::from("/tmp/linked_wt");
        group.worktrees.insert(
            linked_path,
            WorktreeEntry {
                added: "2025-01-01T00:00:00Z".to_string(),
                label: None,
            },
        );
        registry.groups.insert("grp1".to_string(), group);

        dissolve_group(&mut registry, "grp1");

        assert!(!registry.groups.contains_key("grp1"));
    }

    #[test]
    fn test_dissolve_group_nonexistent() {
        // Dissolving a group that doesn't exist should not panic.
        let mut registry = Registry::new();
        let main_path = PathBuf::from("/nonexistent/main");
        let group = Group::new(main_path);
        registry.groups.insert("grp2".to_string(), group);

        dissolve_group(&mut registry, "grp2");
        assert!(!registry.groups.contains_key("grp2"));
    }
}
