/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::HashSet;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use configmodel::Config;
use context::CoreContext;
use gitcompat::GitCmd;
use gitcompat::RepoGit;
use manifest_tree::TreeManifest;
use parking_lot::Mutex;
use pathmatcher::DynMatcher;
use storemodel::FileStore;
use treestate::treestate::TreeState;
use types::HgId;
use types::RepoPathBuf;
use vfs::VFS;

use crate::client::WorkingCopyClient;
use crate::filesystem::FileSystem;
use crate::filesystem::PendingChange;

/// The `DotGitFileSystem` is similar to EdenFileSystem: The actual "tree state" is
/// tracked elsewhere. The "treestate" only tracks non-clean files (`git status`).
/// Instead of talking to EdenFS via Thrift, talk to `git` via CLI.
pub struct DotGitFileSystem {
    #[allow(unused)]
    treestate: Arc<Mutex<TreeState>>,
    #[allow(unused)]
    vfs: VFS,
    #[allow(unused)]
    store: Arc<dyn FileStore>,
    git: Arc<RepoGit>,
    is_automation: bool,
}

impl DotGitFileSystem {
    pub fn new(
        vfs: VFS,
        dot_dir: &Path,
        store: Arc<dyn FileStore>,
        config: &dyn Config,
    ) -> Result<Self> {
        let git = RepoGit::from_root_and_config(vfs.root().to_owned(), config);
        let treestate = create_treestate(&git, dot_dir, vfs.case_sensitive())?;
        let treestate = Arc::new(Mutex::new(treestate));
        let is_automation = hgplain::is_plain(Some("dotgit-no-optional-locks"));
        Ok(DotGitFileSystem {
            treestate,
            vfs,
            store,
            git: Arc::new(git),
            is_automation,
        })
    }

    fn prepare_git_args<'a>(&self, args: &[&'a str]) -> Vec<&'a str> {
        let mut result = Vec::with_capacity(args.len());
        if self.is_automation {
            // If "git status" is run by automation (ex. ISL), likely in background, do not use
            // "index.lock". Otherwise, other git commands run by the user (ex. "git add") could
            // fail with "fatal: Unable to create '.../.git/index.lock': File exists." if the
            // status command is running and holding the lock at the same time.
            result.push("--no-optional-locks");
        }
        result.extend_from_slice(args);
        result
    }
}

fn create_treestate(
    git: &RepoGit,
    dot_dir: &std::path::Path,
    case_sensitive: bool,
) -> Result<TreeState> {
    let dirstate_path = dot_dir.join("dirstate");
    tracing::trace!("loading dotgit dirstate");
    TreeState::from_overlay_dirstate_with_locked_edit(
        &dirstate_path,
        case_sensitive,
        &|treestate| {
            let p1 = git.resolve_head()?;
            let mut parents = treestate.parents().collect::<Result<Vec<HgId>>>()?;
            // Update the overlay dirstate p1 to match Git HEAD (source of truth).
            if !parents.is_empty() && parents[0] != p1 {
                tracing::info!("updating treestate p1 to match git HEAD");
                parents[0] = p1;
                treestate.set_parents(&mut parents.iter())?;
                treestate.flush()?;
            }
            Ok(())
        },
    )
}

impl FileSystem for DotGitFileSystem {
    fn pending_changes(
        &self,
        _ctx: &CoreContext,
        matcher: DynMatcher,
        _ignore_matcher: DynMatcher,
        _ignore_dirs: Vec<PathBuf>,
        include_ignored: bool,
    ) -> Result<Box<dyn Iterator<Item = Result<PendingChange>>>> {
        tracing::debug!(
            include_ignored = include_ignored,
            "pending_changes (DotGitFileSystem)"
        );
        // Run "git status".
        let args = self.prepare_git_args(&[
            "--porcelain=1",
            "--ignore-submodules=dirty",
            "--untracked-files=all",
            "--no-renames",
            "-z",
            if include_ignored {
                "--ignored"
            } else {
                "--ignored=no"
            },
        ]);
        let out = self.git.call("status", &args)?;

        // TODO: What to do with treestate?
        // Submodule status is handled by the callsite (WorkingCopy::status_internal)

        // Example output:
        //
        // M  FILE1
        // MM FILE2
        //  M FILE3
        // A  FILE4
        //  D FILE5
        // ?? FILE6
        // R  FILE7 -> FILE8 (with --renames)
        // D  FILE7          (with --no-renames)
        // A  FiLE8          (with --no-renames)
        // !! FILE9          (with --ignored)
        // AD FILE10         (added to index, deleted on disk)

        // Some files might be "clean" compared to "." but not "index/staging area".
        // sl should report those as "clean", while git might report "MM" (modified in index, and
        // modified in working copy compared with index).
        let mut need_double_check = Vec::<RepoPathBuf>::new();
        let mut changes: Vec<Result<PendingChange>> = out
            .stdout
            .split(|&c| c == 0)
            .filter_map(|line| -> Option<Result<PendingChange>> {
                if line.get(2) != Some(&b' ') {
                    // Unknown format. Ignore.
                    return None;
                }
                let path_bytes = line.get(3..)?;
                let path = RepoPathBuf::from_utf8(path_bytes.to_vec()).ok()?;
                match matcher.matches_file(&path) {
                    Ok(false) => return None,
                    Ok(true) => {}
                    Err(e) => return Some(Err(e)),
                }
                if &line[..2] == b"MM" {
                    // "MM" files might be "clean"
                    need_double_check.push(path);
                    None
                } else {
                    // Prefer "working copy" state. Fallback to index.
                    let sign = if line[1] == b' ' { line[0] } else { line[1] };
                    let change = match sign {
                        b'D' => PendingChange::Deleted(path),
                        b'!' => PendingChange::Ignored(path),
                        _ => PendingChange::Changed(path),
                    };
                    Some(Ok(change))
                }
            })
            .collect();

        if !need_double_check.is_empty() {
            let args = self.prepare_git_args(&["--name-only", "HEAD"]);
            let out = self.git.call("diff", &args)?;
            let changed = out
                .stdout
                .split(|&c| c == b'\n' || c == b'\r')
                .collect::<HashSet<_>>();
            for path in need_double_check {
                if changed.contains(path.as_byte_slice()) {
                    changes.push(Ok(PendingChange::Changed(path)));
                }
            }
        }

        Ok(Box::new(changes.into_iter()))
    }

    fn sparse_matcher(
        &self,
        _manifests: &[Arc<TreeManifest>],
        _dot_dir: &'static str,
    ) -> Result<Option<DynMatcher>> {
        Ok(None)
    }

    fn set_parents(&self, p1: HgId, p2: Option<HgId>, p1_tree: Option<HgId>) -> Result<()> {
        tracing::debug!(p1=?p1, p2=?p2, p1_tree=?p1_tree, "set_parents (DotGitFileSystem)");
        self.git
            .set_parents(p1, p2, p1_tree.unwrap_or(*HgId::wdir_id()))?;
        Ok(())
    }

    fn get_treestate(&self) -> Result<Arc<Mutex<TreeState>>> {
        Ok(self.treestate.clone())
    }

    fn get_client(&self) -> Option<Arc<dyn WorkingCopyClient>> {
        Some(self.git.clone())
    }
}
