/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use anyhow::anyhow;
use anyhow::Error;
use anyhow::Result;
use bonsai_hg_mapping::BonsaiHgMappingRef;
use bookmarks::BookmarksRef;
use commit_graph::CommitGraphRef;
use commit_graph::CommitGraphWriterRef;
use derivative::Derivative;
use megarepo_configs::SourceMappingRules;
use mononoke_types::path::MPath;
use mononoke_types::ChangesetId;
use mononoke_types::FileChange;
use mononoke_types::NonRootMPath;
use repo_blobstore::RepoBlobstoreArc;
use repo_blobstore::RepoBlobstoreRef;
use repo_derived_data::RepoDerivedDataRef;
use repo_identity::RepoIdentityRef;
use slog::error;
use sorted_vector_map::SortedVectorMap;
use thiserror::Error;

pub trait MultiMover: Send + Sync {
    /// Move a path, to potentially multiple locations.
    fn multi_move_path(&self, path: &NonRootMPath) -> Result<Vec<NonRootMPath>>;

    /// Returns true if the path conflicts with any of the paths
    /// the mover will move.  Paths conflict if either one of them
    /// is a path prefix of the other.
    fn conflicts_with(&self, path: &NonRootMPath) -> Result<bool>;
}

pub type DirectoryMultiMover =
    Arc<dyn Fn(&MPath) -> Result<Vec<MPath>, Error> + Send + Sync + 'static>;

/// Determines when a file change filter should be applied.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum FileChangeFilterApplication {
    /// Filter only before getting the implicit deletes from the bonsai
    ImplicitDeletes,
    /// Filter only before calling the multi mover
    MultiMover,
    /// Filter both before getting the implicit deletes from the bonsai and
    /// before calling the multi mover
    Both,
}

// Function that can be used to filter out irrelevant file changes from the bonsai
// before getting its implicit deletes and/or calling the multi mover.
// Getting implicit deletes requires doing manifest lookups that are O(file changes),
// so removing unnecessary changes before can significantly speed up rewrites.
// This can also be used to filter out specific kinds of file changes, e.g.
// git submodules or untracked changes.
pub type FileChangeFilterFunc<'a> =
    Arc<dyn Fn((&NonRootMPath, &FileChange)) -> bool + Send + Sync + 'a>;

/// Specifies a filter to be applied to file changes from a bonsai to remove
/// unwanted changes before certain stages of the rewrite process, e.g. before
/// getting the implicit deletes from the bonsai or before calling the multi
/// mover.
#[derive(Derivative, Clone)]
#[derivative(Debug)]
pub struct FileChangeFilter<'a> {
    /// Function containing the filter logic
    #[derivative(Debug = "ignore")]
    pub func: FileChangeFilterFunc<'a>,
    /// When to apply the filter
    pub application: FileChangeFilterApplication,
}

pub trait Repo = RepoIdentityRef
    + RepoBlobstoreArc
    + BookmarksRef
    + BonsaiHgMappingRef
    + RepoDerivedDataRef
    + RepoBlobstoreRef
    + CommitGraphRef
    + CommitGraphWriterRef
    + Send
    + Sync;

#[derive(Debug, Error)]
pub enum ErrorKind {
    #[error("Remapped commit {0} expected in target repo, but not present")]
    MissingRemappedCommit(ChangesetId),
    #[error(
        "Can't reorder changesets parents to put {0} first because it's not a changeset's parent."
    )]
    MissingForcedParent(ChangesetId),
}

pub struct MegarepoMultiMover {
    overrides: Vec<(String, Vec<String>)>,
    prefix: Option<NonRootMPath>,
}

impl MegarepoMultiMover {
    pub fn new(mapping_rules: SourceMappingRules) -> Result<Self> {
        // We apply the longest prefix first
        let mut overrides = mapping_rules.overrides.into_iter().collect::<Vec<_>>();
        overrides.sort_unstable_by_key(|(ref prefix, _)| prefix.len());
        overrides.reverse();
        let prefix = NonRootMPath::new_opt(mapping_rules.default_prefix)?;
        Ok(Self { overrides, prefix })
    }
}

impl MultiMover for MegarepoMultiMover {
    fn multi_move_path(&self, path: &NonRootMPath) -> Result<Vec<NonRootMPath>> {
        for (override_prefix_src, dsts) in &self.overrides {
            let override_prefix_src = NonRootMPath::new(override_prefix_src.clone())?;
            if override_prefix_src.is_prefix_of(path) {
                let suffix: Vec<_> = path
                    .into_iter()
                    .skip(override_prefix_src.num_components())
                    .collect();

                return dsts
                    .iter()
                    .map(|dst| {
                        let override_prefix = NonRootMPath::new_opt(dst)?;
                        NonRootMPath::join_opt(override_prefix.as_ref(), suffix.clone())
                            .ok_or_else(|| anyhow!("unexpected empty path"))
                    })
                    .collect::<Result<_, _>>();
            }
        }

        Ok(vec![
            NonRootMPath::join_opt(self.prefix.as_ref(), path)
                .ok_or_else(|| anyhow!("unexpected empty path"))?,
        ])
    }

    fn conflicts_with(&self, path: &NonRootMPath) -> Result<bool> {
        match &self.prefix {
            Some(prefix) => {
                if prefix.is_related_to(path) {
                    return Ok(true);
                }
            }
            None => return Ok(true),
        }

        for (override_prefix_src, _) in &self.overrides {
            let override_prefix_src = NonRootMPath::new(override_prefix_src)?;
            if override_prefix_src.is_related_to(path) {
                return Ok(true);
            }
        }

        Ok(false)
    }
}

/// Determines what to do in commits rewriting to empty commit in small repo.
///
/// NOTE: The empty commits from large repo are kept regardless of this flag.
#[derive(PartialEq, Debug, Copy, Clone, Default)]
pub enum CommitRewrittenToEmpty {
    Keep,
    #[default]
    Discard,
}

/// Determines what to do with commits that are empty in large repo.  They may
/// be useful to keep them in small repo if they have some special meaning.
///
/// NOTE: This flag doesn't affect non-empty commits from large repo rewriting
/// to empty commits in small repo. Use CommitsRewrittenToEmpty to control that.
#[derive(PartialEq, Debug, Copy, Clone, Default)]
pub enum EmptyCommitFromLargeRepo {
    #[default]
    Keep,
    Discard,
}

/// Whether Hg or Git extras should be stripped from the commit when rewriting
/// it, to avoid creating many to one mappings between repos.
/// This will depend on the default commit scheme of the source and target repos.
///
/// For example: if the source repo is Hg and the target repo is Git, two
/// commits that differ only by hg extra would be mapped to the same git commit.
/// In this case, hg extras have to be stripped when syncing from Hg to Git.
#[derive(PartialEq, Debug, Copy, Clone, Default)]
pub enum StripCommitExtras {
    #[default]
    None,
    Hg,
    Git,
}

#[derive(PartialEq, Debug, Clone, Default)]
pub struct RewriteOpts {
    pub commit_rewritten_to_empty: CommitRewrittenToEmpty,
    pub empty_commit_from_large_repo: EmptyCommitFromLargeRepo,
    pub strip_commit_extras: StripCommitExtras,
    /// Hg doesn't have a concept of committer and committer date, so commits
    /// that are originally created in Hg have these fields empty when synced
    /// to a git repo.
    ///
    /// This setting determines if, in Hg->Git sync, the committer and committer
    /// date fields should be set to the author and date fields if empty.
    pub should_set_committer_info_to_author_info_if_empty: bool,

    /// Any extra data that should be added to hg_extra during rewrite.
    pub add_hg_extras: SortedVectorMap<String, Vec<u8>>,
}

pub(crate) enum LossyConversionReason {
    FileChanges,
    ImplicitFileChanges,
    SubtreeChanges,
}
