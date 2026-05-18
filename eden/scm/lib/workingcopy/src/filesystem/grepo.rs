/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use configmodel::Config;
use configmodel::ConfigExt;
use context::CoreContext;
use grepomanifest::parse::parse_manifest;
use grepomanifest::schema::Project;
use manifest::FileType;
use manifest::Manifest;
use manifest_tree::ReadTreeManifest;
use manifest_tree::TreeManifest;
use parking_lot::Mutex;
use pathmatcher::DynMatcher;
use treestate::treestate::TreeState;
use types::HgId;
use types::RepoPath;
use vfs::VFS;

use crate::WorkingCopy;
use crate::client::WorkingCopyClient;
use crate::fast_path_wdir_parents;
use crate::filesystem::DotGitFileSystem;
use crate::filesystem::FileSystem;
use crate::filesystem::PendingChange;

pub struct GrepoFileSystem {
    inner: DotGitFileSystem,
    vfs: VFS,
    tree_resolver: Arc<dyn ReadTreeManifest>,
    manifest_path: PathBuf,
}

impl GrepoFileSystem {
    pub fn new(
        inner: DotGitFileSystem,
        vfs: VFS,
        tree_resolver: Arc<dyn ReadTreeManifest>,
        config: &dyn Config,
    ) -> Result<Self> {
        let manifest_path = grepo_manifest_path(&vfs, config)?;
        Ok(GrepoFileSystem {
            inner,
            vfs,
            tree_resolver,
            manifest_path,
        })
    }

    /// Parse the `.repo/manifests` from the working copy.
    fn parse_grepo_projects(&self) -> Result<BTreeMap<PathBuf, Project>> {
        Ok(parse_grepo_manifest(&self.manifest_path)?.projects)
    }
}

pub(crate) fn grepo_manifest_path(vfs: &VFS, config: &dyn Config) -> Result<PathBuf> {
    let path = config.get_or("grepo", "manifestpath", || {
        ".repo/manifests/default.xml".to_string()
    })?;
    Ok(vfs.join(path.as_str().try_into()?))
}

/// Parse `.repo/manifests` from the working copy.
pub(crate) fn parse_grepo_manifest(
    manifest_path: &PathBuf,
) -> Result<grepomanifest::schema::Manifest> {
    if !manifest_path.exists() {
        tracing::debug!(target: "workingcopy::repo_tool", "manifest file does not exist");
        return Ok(grepomanifest::schema::Manifest::default());
    }
    parse_manifest(&fs_err::read(manifest_path)?)
}

impl FileSystem for GrepoFileSystem {
    fn pending_changes(
        &self,
        _context: &CoreContext,
        matcher: DynMatcher,
        _ignore_matcher: DynMatcher,
        _ignore_dirs: Vec<PathBuf>,
        _include_ignored: bool,
    ) -> Result<Box<dyn Iterator<Item = Result<PendingChange>>>> {
        let mut changes = Vec::new();
        if let Some(parent_tree) =
            WorkingCopy::current_manifests(&self.get_treestate()?.lock(), &self.tree_resolver)?
                .into_iter()
                .next()
        {
            for (path, _proj) in self.parse_grepo_projects()?.iter() {
                let path_str = path.to_str().unwrap();
                let repo_path = RepoPath::from_str(path_str)?;

                if !matcher.matches_file(repo_path)? {
                    continue;
                }

                let parent_node =
                    parent_tree
                        .get_file(repo_path)?
                        .and_then(|f| match f.file_type {
                            FileType::GitSubmodule => Some(f.hgid),
                            // TODO: support linkfile and copyfile
                            _ => {
                                tracing::warn!(
                                    "unexpected file type for .repo identity at {:?}",
                                    &repo_path
                                );
                                None
                            }
                        });

                let abs_path = self.vfs.root().join(path_str);
                let curr_node = match identity::sniff_dir(&abs_path)? {
                    Some(id) => fast_path_wdir_parents(&abs_path, id)?.p1().copied(),
                    None => {
                        tracing::warn!("Project {:?} is not a recognized repo", abs_path);
                        None
                    }
                };

                if parent_node != curr_node {
                    let path = repo_path.to_owned();
                    let change = match (parent_node, curr_node) {
                        (None, Some(_)) => Some(PendingChange::Changed(path)),
                        (Some(_), None) => Some(PendingChange::Deleted(path)),
                        (Some(_), Some(_)) => Some(PendingChange::Changed(path)),
                        (None, None) => None,
                    };
                    if let Some(change) = change {
                        changes.push(Ok(change));
                    }
                }
            }
        }

        Ok(Box::new(changes.into_iter()))
    }

    fn wait_for_potential_change(&self, config: &dyn Config) -> Result<()> {
        self.inner.wait_for_potential_change(config)
    }

    fn sparse_matcher(
        &self,
        manifests: &[Arc<TreeManifest>],
        dot_dir: &'static str,
    ) -> Result<Option<DynMatcher>> {
        self.inner.sparse_matcher(manifests, dot_dir)
    }

    fn set_parents(
        &self,
        p1: HgId,
        p2: Option<HgId>,
        parent_tree_hash: Option<HgId>,
    ) -> Result<()> {
        self.inner.set_parents(p1, p2, parent_tree_hash)
    }

    fn get_treestate(&self) -> Result<Arc<Mutex<TreeState>>> {
        self.inner.get_treestate()
    }

    fn get_client(&self) -> Option<Arc<dyn WorkingCopyClient>> {
        self.inner.get_client()
    }
}
