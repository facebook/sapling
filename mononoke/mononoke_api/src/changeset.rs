/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use std::collections::{HashMap, HashSet};
use std::convert::TryInto;
use std::fmt;
use std::future::Future;
use std::pin::Pin;

use chrono::{DateTime, FixedOffset};
use cloned::cloned;
use context::CoreContext;
use derived_data::BonsaiDerived;
use fsnodes::RootFsnodeId;
use futures_preview::compat::{Future01CompatExt, Stream01CompatExt};
use futures_preview::stream::Stream;
use futures_util::future::{self, try_join, try_join_all, FutureExt, Shared};
use futures_util::stream::{StreamExt, TryStreamExt};
use manifest::{Diff as ManifestDiff, Entry as ManifestEntry, ManifestOps, PathOrPrefix};
use mercurial_types::Globalrev;
use mononoke_types::{BonsaiChangeset, MPath, MPathElement};
use reachabilityindex::ReachabilityIndex;
use unodes::RootUnodeManifestId;

use crate::changeset_path::ChangesetPathContext;
use crate::changeset_path_diff::ChangesetPathDiffContext;
use crate::errors::MononokeError;
use crate::path::MononokePath;
use crate::repo::RepoContext;
use crate::specifiers::{ChangesetId, HgChangesetId};

#[derive(Clone)]
pub struct ChangesetContext {
    repo: RepoContext,
    id: ChangesetId,
    bonsai_changeset:
        Shared<Pin<Box<dyn Future<Output = Result<BonsaiChangeset, MononokeError>> + Send>>>,
    root_fsnode_id:
        Shared<Pin<Box<dyn Future<Output = Result<RootFsnodeId, MononokeError>> + Send>>>,
    root_unode_manifest_id:
        Shared<Pin<Box<dyn Future<Output = Result<RootUnodeManifestId, MononokeError>> + Send>>>,
}

impl fmt::Debug for ChangesetContext {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "ChangesetContext(repo={:?} id={:?})",
            self.repo().name(),
            self.id()
        )
    }
}

/// A context object representing a query to a particular commit in a repo.
impl ChangesetContext {
    /// Construct a new `MononokeChangeset`.  The changeset must exist
    /// in the repo.
    pub(crate) fn new(repo: RepoContext, id: ChangesetId) -> Self {
        let bonsai_changeset = {
            cloned!(repo);
            async move {
                repo.blob_repo()
                    .get_bonsai_changeset(repo.ctx().clone(), id)
                    .compat()
                    .await
                    .map_err(MononokeError::from)
            }
        };
        let bonsai_changeset = bonsai_changeset.boxed().shared();
        let root_fsnode_id = {
            cloned!(repo);
            async move {
                RootFsnodeId::derive(
                    repo.ctx().clone(),
                    repo.blob_repo().clone(),
                    repo.fsnodes_derived_mapping().clone(),
                    id,
                )
                .compat()
                .await
                .map_err(MononokeError::from)
            }
        };
        let root_fsnode_id = root_fsnode_id.boxed().shared();
        let root_unode_manifest_id = {
            cloned!(repo);
            async move {
                RootUnodeManifestId::derive(
                    repo.ctx().clone(),
                    repo.blob_repo().clone(),
                    repo.unodes_derived_mapping().clone(),
                    id,
                )
                .compat()
                .await
                .map_err(MononokeError::from)
            }
        };
        let root_unode_manifest_id = root_unode_manifest_id.boxed().shared();
        Self {
            repo,
            id,
            bonsai_changeset,
            root_fsnode_id,
            root_unode_manifest_id,
        }
    }

    /// The context for this query.
    pub(crate) fn ctx(&self) -> &CoreContext {
        &self.repo.ctx()
    }

    /// The `RepoContext` for this query.
    pub(crate) fn repo(&self) -> &RepoContext {
        &self.repo
    }

    /// The canonical bonsai changeset ID for the changeset.
    pub fn id(&self) -> ChangesetId {
        self.id
    }

    /// The Mercurial ID for the changeset.
    pub async fn hg_id(&self) -> Result<Option<HgChangesetId>, MononokeError> {
        let mapping = self
            .repo()
            .blob_repo()
            .get_hg_bonsai_mapping(self.ctx().clone(), self.id)
            .compat()
            .await?;
        Ok(mapping.iter().next().map(|(hg_cs_id, _)| *hg_cs_id))
    }

    /// The Globalrev for the changeset.
    pub async fn globalrev(&self) -> Result<Option<Globalrev>, MononokeError> {
        let mapping = self
            .repo()
            .blob_repo()
            .get_globalrev_from_bonsai(self.id)
            .compat()
            .await?;
        Ok(mapping.into_iter().next())
    }

    pub(crate) async fn root_fsnode_id(&self) -> Result<RootFsnodeId, MononokeError> {
        self.root_fsnode_id.clone().await
    }

    pub(crate) async fn root_unode_manifest_id(
        &self,
    ) -> Result<RootUnodeManifestId, MononokeError> {
        self.root_unode_manifest_id.clone().await
    }

    /// Query the root directory in the repository at this changeset revision.
    pub fn root(&self) -> ChangesetPathContext {
        ChangesetPathContext::new(self.clone(), None)
    }

    /// Query a path within the respository. This could be a file or a
    /// directory.
    pub fn path<P>(&self, path: P) -> Result<ChangesetPathContext, MononokeError>
    where
        P: TryInto<MononokePath>,
        MononokeError: From<P::Error>,
    {
        Ok(ChangesetPathContext::new(self.clone(), path.try_into()?))
    }

    pub async fn paths(
        &self,
        paths: impl Iterator<Item = MononokePath>,
    ) -> Result<impl Stream<Item = Result<ChangesetPathContext, MononokeError>>, MononokeError>
    {
        Ok(self
            .root_fsnode_id()
            .await?
            .fsnode_id()
            .find_entries(
                self.ctx().clone(),
                self.repo().blob_repo().get_blobstore(),
                paths.map(|path| path.into_mpath()),
            )
            .compat()
            .map_ok({
                let changeset = self.clone();
                move |(mpath, entry)| {
                    ChangesetPathContext::new_with_fsnode_entry(
                        changeset.clone(),
                        MononokePath::new(mpath),
                        entry,
                    )
                }
            })
            .map_err(MononokeError::from))
    }

    /// Get the `BonsaiChangeset` information for this changeset.
    async fn bonsai_changeset(&self) -> Result<BonsaiChangeset, MononokeError> {
        self.bonsai_changeset.clone().await
    }

    /// The IDs of the parents of the changeset.
    pub async fn parents(&self) -> Result<Vec<ChangesetId>, MononokeError> {
        Ok(self.bonsai_changeset().await?.parents().collect())
    }

    /// The author of the changeset.
    pub async fn author(&self) -> Result<String, MononokeError> {
        Ok(self.bonsai_changeset().await?.author().to_string())
    }

    /// The date the changeset was authored.
    pub async fn author_date(&self) -> Result<DateTime<FixedOffset>, MononokeError> {
        Ok(self
            .bonsai_changeset()
            .await?
            .author_date()
            .as_chrono()
            .clone())
    }

    /// The committer of the changeset.  May be `None` if the committer
    /// is not tracked.
    pub async fn committer(&self) -> Result<Option<String>, MononokeError> {
        Ok(self
            .bonsai_changeset()
            .await?
            .committer()
            .map(|s| s.to_string()))
    }

    /// The date the changeset was committed.  May be `None` if the
    /// committer is not tracked.
    pub async fn committer_date(&self) -> Result<Option<DateTime<FixedOffset>>, MononokeError> {
        Ok(self
            .bonsai_changeset()
            .await?
            .committer_date()
            .map(|d| d.as_chrono().clone()))
    }

    /// The commit message.
    pub async fn message(&self) -> Result<String, MononokeError> {
        Ok(self.bonsai_changeset().await?.message().to_string())
    }

    /// All commit extras as (name, value) pairs.
    pub async fn extras(&self) -> Result<Vec<(String, Vec<u8>)>, MononokeError> {
        Ok(self
            .bonsai_changeset()
            .await?
            .extra()
            .map(|(name, value)| (name.to_string(), Vec::from(value)))
            .collect())
    }

    /// Returns `true` if this commit is an ancestor of `other_commit`.
    pub async fn is_ancestor_of(&self, other_commit: ChangesetId) -> Result<bool, MononokeError> {
        let is_ancestor_of = self
            .repo()
            .skiplist_index()
            .query_reachability(
                self.ctx().clone(),
                self.repo().blob_repo().get_changeset_fetcher(),
                other_commit,
                self.id,
            )
            .compat()
            .await?;
        Ok(is_ancestor_of)
    }

    /// Returns differences between this changeset and some other changeset.
    ///
    /// `self` is considered the "new" changeset (so files missing there are "Removed")
    /// `other` is considered the "old" changeset (so files missing there are "Added")
    /// `include_copies_renames` is only available for diffing commits with its parent
    /// `path_restrictions` if present will narrow down the diff to given paths
    pub async fn diff(
        &self,
        other: ChangesetId,
        include_copies_renames: bool,
        path_restrictions: Option<Vec<MononokePath>>,
    ) -> Result<Vec<ChangesetPathDiffContext>, MononokeError> {
        // Helper to that checks if a path is within the givien path restrictions
        fn within_restrictions(
            path: Option<MPath>,
            path_restrictions: &Option<Vec<MononokePath>>,
        ) -> bool {
            let mononoke_path = MononokePath::new(path);
            path_restrictions.as_ref().map_or(true, |i| {
                i.iter()
                    .any(|path_restriction| mononoke_path.is_related_to(&path_restriction))
            })
        }
        let other = ChangesetContext::new(self.repo.clone(), other);
        let bonsai = self.bonsai_changeset().await?;

        // map from from_path to to_path
        let mut copy_path_map = HashMap::new();
        // map from to_path to from_path
        let mut inv_copy_path_map = HashMap::new();
        let file_changes;
        // For now we only consider copies when comparing with parent.
        if include_copies_renames && self.parents().await?.contains(&other.id) {
            file_changes = bonsai.file_changes().collect::<Vec<_>>();
            for (to_path, file_change) in file_changes.iter() {
                if let Some((from_path, csid)) = file_change.and_then(|fc| fc.copy_from()) {
                    if *csid == other.id {
                        copy_path_map.insert(from_path, to_path);
                        inv_copy_path_map.insert(to_path, from_path);
                    }
                }
            }
        }
        // set of paths from other that were copied in (not moved)
        let copied_paths: HashSet<_> =
            try_join_all(copy_path_map.iter().map(move |(from_path, _)| {
                async move { self.path((*from_path).clone())?.file_type().await }
            }))
            .await?
            .into_iter()
            .zip(copy_path_map.keys())
            .filter_map(|(file_type, path)| file_type.map(|_| path))
            .collect();

        let (self_manifest_root, other_manifest_root) =
            try_join(self.root_fsnode_id(), other.root_fsnode_id()).await?;
        let change_contexts = other_manifest_root // yes, we start from "other" as manfest.diff() is backwards
            .fsnode_id()
            .filtered_diff(
                self.ctx().clone(),
                self.repo().blob_repo().get_blobstore(),
                self_manifest_root.fsnode_id().clone(),
                Some,
                {
                    cloned!(path_restrictions);
                    move |tree_diff| match tree_diff {
                        ManifestDiff::Added(path, ..)
                        | ManifestDiff::Changed(path, ..)
                        | ManifestDiff::Removed(path, ..) => {
                            within_restrictions(path.clone(), &path_restrictions)
                        }
                    }
                },
            )
            .compat()
            .try_filter_map(|diff_entry| {
                future::ok(match diff_entry {
                    ManifestDiff::Added(Some(path), ManifestEntry::Leaf(_)) => {
                        if !within_restrictions(Some(path.clone()), &path_restrictions) {
                            None
                        } else if let Some(from_path) = inv_copy_path_map.get(&&path) {
                            // There's copy information that we can use.
                            if copied_paths.contains(from_path) {
                                // If the source still exists in the current commit it was a copy.
                                Some(ChangesetPathDiffContext::Copied(
                                    ChangesetPathContext::new(self.clone(), path.clone()),
                                    ChangesetPathContext::new(other.clone(), (*from_path).clone()),
                                ))
                            } else {
                                // If it doesn't it was a move
                                Some(ChangesetPathDiffContext::Moved(
                                    ChangesetPathContext::new(self.clone(), path.clone()),
                                    ChangesetPathContext::new(other.clone(), (*from_path).clone()),
                                ))
                            }
                        } else {
                            Some(ChangesetPathDiffContext::Added(ChangesetPathContext::new(
                                self.clone(),
                                path,
                            )))
                        }
                    }
                    ManifestDiff::Removed(Some(path), ManifestEntry::Leaf(_)) => {
                        if let Some(_) = copy_path_map.get(&path) {
                            // The file is was moved (not removed), it will be covered by a "Moved" entry.
                            None
                        } else if !within_restrictions(Some(path.clone()), &path_restrictions) {
                            None
                        } else {
                            Some(ChangesetPathDiffContext::Removed(
                                ChangesetPathContext::new(other.clone(), path),
                            ))
                        }
                    }
                    ManifestDiff::Changed(
                        Some(path),
                        ManifestEntry::Leaf(_a),
                        ManifestEntry::Leaf(_b),
                    ) => {
                        if !within_restrictions(Some(path.clone()), &path_restrictions) {
                            None
                        } else {
                            Some(ChangesetPathDiffContext::Changed(
                                ChangesetPathContext::new(self.clone(), path.clone()),
                                ChangesetPathContext::new(other.clone(), path),
                            ))
                        }
                    }
                    // We don't care about diffs not involving leaves
                    _ => None,
                })
            })
            .try_collect::<Vec<_>>()
            .await?;
        return Ok(change_contexts);
    }

    pub async fn find_files(
        &self,
        prefixes: Option<Vec<MononokePath>>,
        basenames: Option<Vec<String>>,
    ) -> Result<impl Stream<Item = Result<MononokePath, MononokeError>>, MononokeError> {
        let root = self.root_fsnode_id().await?;
        let prefixes = match prefixes {
            Some(prefixes) => prefixes
                .into_iter()
                .map(|prefix| PathOrPrefix::Prefix(prefix.into()))
                .collect(),
            None => vec![PathOrPrefix::Prefix(None)],
        };
        let mpaths = root
            .fsnode_id()
            .find_entries(
                self.ctx().clone(),
                self.repo().blob_repo().get_blobstore(),
                prefixes,
            )
            .compat()
            .try_filter_map(|(path, entry)| {
                async move {
                    match (path, entry) {
                        (Some(mpath), ManifestEntry::Leaf(_)) => Ok(Some(mpath)),
                        _ => Ok(None),
                    }
                }
            });
        let mpaths = match basenames {
            Some(basenames) => {
                let basename_set = basenames
                    .into_iter()
                    .map(|basename| MPathElement::new(basename.into()))
                    .collect::<Result<HashSet<_>, _>>()
                    .map_err(MononokeError::from)?;
                mpaths
                    .try_filter(move |mpath| future::ready(basename_set.contains(mpath.basename())))
                    .into_stream()
                    .left_stream()
            }
            None => mpaths.into_stream().right_stream(),
        };
        Ok(mpaths
            .map_ok(|mpath| MononokePath::new(Some(mpath)))
            .map_err(MononokeError::from))
    }
}
