/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::fmt;
use std::fmt::Debug;
use std::sync::Arc;

use bonsai_git_mapping::BonsaiGitMappingRef;
use bonsai_hg_mapping::BonsaiHgMappingArc;
use bookmarks::BookmarkUpdateLogArc;
use bookmarks::BookmarkUpdateLogRef;
use filestore::FilestoreConfigRef;
use mononoke_types::NonRootMPath;
use repo_cross_repo::RepoCrossRepoRef;
use repo_identity::RepoIdentityRef;
use sorted_vector_map::SortedVectorMap;

// TODO(T182311609): try to use all refs instead of arcs
pub trait Repo = commit_transformation::Repo
    + BonsaiGitMappingRef
    + BonsaiHgMappingArc
    + BookmarkUpdateLogArc
    + BookmarkUpdateLogRef
    + Clone
    + FilestoreConfigRef
    + RepoCrossRepoRef
    + Send
    + Sync
    + 'static;

/// Wrapper to differentiate submodule paths from file changes paths at the
/// type level.
#[derive(Eq, Clone, Debug, PartialEq, Hash, PartialOrd, Ord)]
pub struct SubmodulePath(pub(crate) NonRootMPath);

impl std::fmt::Display for SubmodulePath {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        std::fmt::Display::fmt(&self.0, f)
    }
}

/// Syncing commits from a small Mononoke repo with submodule file changes to a
/// large repo requires the small repo submodule dependencies to be available.
///
/// However, LargeToSmall sync and some SmallToLarge operations don't require
/// loading these repos, in which case this value will be set to `None`.
/// When rewriting commits from small to large (i.e. calling `rewrite_commit`),
/// this map has to be available, or the operation will crash otherwise.
#[derive(Clone)]
pub enum SubmoduleDeps<R> {
    ForSync(HashMap<NonRootMPath, Arc<R>>),
    NotNeeded,
    NotAvailable,
}

impl<R> Default for SubmoduleDeps<R> {
    fn default() -> Self {
        Self::NotNeeded
    }
}

impl<R: RepoIdentityRef> SubmoduleDeps<R> {
    pub fn get_submodule_deps_names(&self) -> Option<SortedVectorMap<&NonRootMPath, &str>> {
        match self {
            Self::ForSync(map) => Some(
                map.iter()
                    .map(|(k, v)| (k, v.repo_identity().name()))
                    .collect(),
            ),
            _ => None,
        }
    }

    pub fn repos(&self) -> Vec<Arc<R>> {
        match self {
            Self::ForSync(map) => map.values().cloned().collect(),
            _ => Vec::new(),
        }
    }

    pub fn dep_map(&self) -> Option<&HashMap<NonRootMPath, Arc<R>>> {
        match self {
            Self::ForSync(map) => Some(map),
            _ => None,
        }
    }
}

impl<R: Repo> Debug for SubmoduleDeps<R> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.get_submodule_deps_names().fmt(f)
    }
}
