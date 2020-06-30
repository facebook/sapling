/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::convert::TryInto;
use std::fmt;
use std::future::Future;
use std::pin::Pin;

use anyhow::anyhow;
use blobrepo_hg::BlobRepoHg;
use blobstore::Loadable;
use changeset_info::ChangesetInfo;
use chrono::{DateTime, FixedOffset};
use cloned::cloned;
use context::CoreContext;
use derived_data::BonsaiDerived;
use fsnodes::RootFsnodeId;
use futures::compat::{Future01CompatExt, Stream01CompatExt};
use futures::future::{self, try_join, FutureExt, Shared};
use futures::stream::{self, Stream, StreamExt, TryStreamExt};
use manifest::{Diff as ManifestDiff, Entry as ManifestEntry, ManifestOps, PathOrPrefix};
use maplit::hashset;
use mercurial_types::Globalrev;
pub use mononoke_types::Generation;
use mononoke_types::{BonsaiChangeset, FileChange, MPath, MPathElement};
use reachabilityindex::ReachabilityIndex;
use unodes::RootUnodeManifestId;

use crate::changeset_path::ChangesetPathContext;
use crate::changeset_path_diff::ChangesetPathDiffContext;
use crate::errors::MononokeError;
use crate::path::MononokePath;
use crate::repo::RepoContext;
use crate::specifiers::{ChangesetId, GitSha1, HgChangesetId};

#[derive(Clone)]
pub struct ChangesetContext {
    repo: RepoContext,
    id: ChangesetId,
    bonsai_changeset:
        Shared<Pin<Box<dyn Future<Output = Result<BonsaiChangeset, MononokeError>> + Send>>>,
    changeset_info:
        Shared<Pin<Box<dyn Future<Output = Result<ChangesetInfo, MononokeError>> + Send>>>,
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
                id.load(repo.ctx().clone(), repo.blob_repo().blobstore())
                    .await
                    .map_err(MononokeError::from)
            }
        };
        let bonsai_changeset = bonsai_changeset.boxed().shared();
        let changeset_info = {
            cloned!(repo);
            async move {
                ChangesetInfo::derive(repo.ctx().clone(), repo.blob_repo().clone(), id)
                    .compat()
                    .await
                    .map_err(MononokeError::from)
            }
        };
        let changeset_info = changeset_info.boxed().shared();
        let root_fsnode_id = {
            cloned!(repo);
            async move {
                RootFsnodeId::derive(repo.ctx().clone(), repo.blob_repo().clone(), id)
                    .compat()
                    .await
                    .map_err(MononokeError::from)
            }
        };
        let root_fsnode_id = root_fsnode_id.boxed().shared();
        let root_unode_manifest_id = {
            cloned!(repo);
            async move {
                RootUnodeManifestId::derive(repo.ctx().clone(), repo.blob_repo().clone(), id)
                    .compat()
                    .await
                    .map_err(MononokeError::from)
            }
        };
        let root_unode_manifest_id = root_unode_manifest_id.boxed().shared();
        Self {
            repo,
            id,
            changeset_info,
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
    pub fn repo(&self) -> &RepoContext {
        &self.repo
    }

    /// The canonical bonsai changeset ID for the changeset.
    pub fn id(&self) -> ChangesetId {
        self.id
    }

    pub fn into_repo_and_id(self) -> (RepoContext, ChangesetId) {
        let Self { repo, id, .. } = self;
        (repo, id)
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

    /// The git Sha1 for the changeset (if available).
    pub async fn git_sha1(&self) -> Result<Option<GitSha1>, MononokeError> {
        Ok(self
            .repo()
            .blob_repo()
            .bonsai_git_mapping()
            .get_git_sha1_from_bonsai(self.ctx(), self.id)
            .await?)
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

    /// Get the `ChangesetInfo` for this changeset.
    async fn changeset_info(&self) -> Result<ChangesetInfo, MononokeError> {
        if self.repo.derive_changeset_info_enabled() {
            self.changeset_info.clone().await
        } else {
            let bonsai = self.bonsai_changeset().await?;
            Ok(ChangesetInfo::new(self.id(), bonsai))
        }
    }

    /// The IDs of the parents of the changeset.
    pub async fn parents(&self) -> Result<Vec<ChangesetId>, MononokeError> {
        Ok(self.changeset_info().await?.parents().collect())
    }

    /// The author of the changeset.
    pub async fn author(&self) -> Result<String, MononokeError> {
        Ok(self.changeset_info().await?.author().to_string())
    }

    /// The date the changeset was authored.
    pub async fn author_date(&self) -> Result<DateTime<FixedOffset>, MononokeError> {
        Ok(self
            .changeset_info()
            .await?
            .author_date()
            .as_chrono()
            .clone())
    }

    /// The committer of the changeset.  May be `None` if the committer
    /// is not tracked.
    pub async fn committer(&self) -> Result<Option<String>, MononokeError> {
        Ok(self
            .changeset_info()
            .await?
            .committer()
            .map(|s| s.to_string()))
    }

    /// The date the changeset was committed.  May be `None` if the
    /// committer is not tracked.
    pub async fn committer_date(&self) -> Result<Option<DateTime<FixedOffset>>, MononokeError> {
        Ok(self
            .changeset_info()
            .await?
            .committer_date()
            .map(|d| d.as_chrono().clone()))
    }

    /// The commit message.
    pub async fn message(&self) -> Result<String, MononokeError> {
        Ok(self.changeset_info().await?.message().to_string())
    }

    /// The generation number of the given changeset
    pub async fn generation(&self) -> Result<Generation, MononokeError> {
        self.repo()
            .blob_repo()
            .get_generation_number(self.ctx().clone(), self.id)
            .compat()
            .await?
            .ok_or_else(|| {
                MononokeError::NotAvailable(format!("Generation number missing for {:?}", &self.id))
            })
    }

    /// All commit extras as (name, value) pairs.
    pub async fn extras(&self) -> Result<Vec<(String, Vec<u8>)>, MononokeError> {
        Ok(self
            .changeset_info()
            .await?
            .extra()
            .map(|(name, value)| (name.to_string(), Vec::from(value)))
            .collect())
    }

    /// File changes associated with the commit.
    pub async fn file_changes(&self) -> Result<BTreeMap<MPath, Option<FileChange>>, MononokeError> {
        let bonsai = self.bonsai_changeset().await?;
        let bonsai = bonsai.into_mut();
        Ok(bonsai.file_changes)
    }

    /// Returns `true` if this commit is an ancestor of `other_commit`.
    pub async fn is_ancestor_of(&self, other_commit: ChangesetId) -> Result<bool, MononokeError> {
        let is_ancestor_of = self
            .repo()
            .skiplist_index()
            .query_reachability(
                &self.ctx(),
                &self.repo().blob_repo().get_changeset_fetcher(),
                other_commit,
                self.id,
            )
            .await?;
        Ok(is_ancestor_of)
    }

    /// Returns the lowest common ancestor of two commits.
    ///
    /// In case of ambiguity (can happen with multiple merges of the same branches) returns the
    /// common ancestor with lowest id out of those with highest generation number.
    pub async fn common_base_with(
        &self,
        other_commit: ChangesetId,
    ) -> Result<Option<ChangesetContext>, MononokeError> {
        let lca = self
            .repo()
            .skiplist_index()
            .lca(
                self.ctx().clone(),
                self.repo().blob_repo().get_changeset_fetcher(),
                self.id,
                other_commit,
            )
            .await?;
        Ok(lca
            .iter()
            .next()
            .map(|id| Self::new(self.repo.clone(), *id)))
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

        // map from from_path to to_path
        let mut copy_path_map = HashMap::new();
        // map from to_path to from_path
        let mut inv_copy_path_map = HashMap::new();
        let file_changes = self.file_changes().await?;
        // For now we only consider copies when comparing with parent.
        if include_copies_renames && self.parents().await?.contains(&other.id) {
            for (to_path, file_change) in file_changes.iter() {
                if let Some((from_path, csid)) = file_change.as_ref().and_then(|fc| fc.copy_from())
                {
                    if *csid == other.id {
                        copy_path_map.insert(from_path, to_path);
                    }
                }
            }

            // Prefetch fsnode entries for all "from paths" so that we don't need
            // to refetch them later
            let from_path_to_mf_entry = other
                .root_fsnode_id()
                .await?
                .fsnode_id()
                .find_entries(
                    self.ctx().clone(),
                    self.repo().blob_repo().get_blobstore(),
                    copy_path_map.keys().cloned().cloned(),
                )
                .compat()
                .try_filter_map(|(maybe_from_path, entry)| async move {
                    Ok(maybe_from_path.map(|from_path| (from_path, entry)))
                })
                .try_collect::<HashMap<_, _>>()
                .await?;
            inv_copy_path_map = copy_path_map
                .iter()
                .map(move |(from_path, to_path)| {
                    let mf_entry = from_path_to_mf_entry.get(from_path).cloned().ok_or(
                        MononokeError::from(anyhow!(
                            "internal error cannot find {:?} in parent commit",
                            from_path
                        )),
                    )?;
                    let res: Result<_, MononokeError> = Ok((to_path, (from_path, mf_entry)));
                    res
                })
                .collect::<Result<HashMap<_, _>, _>>()?;
        }

        // set of paths from other that were copied in (not moved)
        // We check if `self` contains paths that were source for copy or move in `other`
        // If self does contain a path, then we consider it to be a copy, otherwise
        // it's a move.
        let copied_paths = self
            .root_fsnode_id()
            .await?
            .fsnode_id()
            .find_entries(
                self.ctx().clone(),
                self.repo().blob_repo().get_blobstore(),
                copy_path_map.keys().cloned().cloned(),
            )
            .compat()
            .try_filter_map(|(maybe_from_path, _)| async move { Ok(maybe_from_path) })
            .try_collect::<HashSet<_>>()
            .await?;

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
                    ManifestDiff::Added(Some(path), entry @ ManifestEntry::Leaf(_)) => {
                        if !within_restrictions(Some(path.clone()), &path_restrictions) {
                            None
                        } else if let Some((from_path, from_entry)) = inv_copy_path_map.get(&&path)
                        {
                            // There's copy information that we can use.
                            if copied_paths.contains(from_path) {
                                // If the source still exists in the current commit it was a copy.
                                Some(ChangesetPathDiffContext::Copied(
                                    ChangesetPathContext::new_with_fsnode_entry(
                                        self.clone(),
                                        path.clone(),
                                        entry,
                                    ),
                                    ChangesetPathContext::new_with_fsnode_entry(
                                        other.clone(),
                                        (**from_path).clone(),
                                        *from_entry,
                                    ),
                                ))
                            } else {
                                // If it doesn't it was a move
                                Some(ChangesetPathDiffContext::Moved(
                                    ChangesetPathContext::new_with_fsnode_entry(
                                        self.clone(),
                                        path.clone(),
                                        entry,
                                    ),
                                    ChangesetPathContext::new_with_fsnode_entry(
                                        other.clone(),
                                        (**from_path).clone(),
                                        *from_entry,
                                    ),
                                ))
                            }
                        } else {
                            Some(ChangesetPathDiffContext::Added(
                                ChangesetPathContext::new_with_fsnode_entry(
                                    self.clone(),
                                    path,
                                    entry,
                                ),
                            ))
                        }
                    }
                    ManifestDiff::Removed(Some(path), entry @ ManifestEntry::Leaf(_)) => {
                        if let Some(_) = copy_path_map.get(&path) {
                            // The file is was moved (not removed), it will be covered by a "Moved" entry.
                            None
                        } else if !within_restrictions(Some(path.clone()), &path_restrictions) {
                            None
                        } else {
                            Some(ChangesetPathDiffContext::Removed(
                                ChangesetPathContext::new_with_fsnode_entry(
                                    other.clone(),
                                    path,
                                    entry,
                                ),
                            ))
                        }
                    }
                    ManifestDiff::Changed(
                        Some(path),
                        from_entry @ ManifestEntry::Leaf(_),
                        to_entry @ ManifestEntry::Leaf(_),
                    ) => {
                        if !within_restrictions(Some(path.clone()), &path_restrictions) {
                            None
                        } else {
                            Some(ChangesetPathDiffContext::Changed(
                                ChangesetPathContext::new_with_fsnode_entry(
                                    self.clone(),
                                    path.clone(),
                                    to_entry,
                                ),
                                ChangesetPathContext::new_with_fsnode_entry(
                                    other.clone(),
                                    path,
                                    from_entry,
                                ),
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
            .try_filter_map(|(path, entry)| async move {
                match (path, entry) {
                    (Some(mpath), ManifestEntry::Leaf(_)) => Ok(Some(mpath)),
                    _ => Ok(None),
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

    /// Returns a stream of `ChangesetContext` for the history of the repository from this commit.
    pub async fn history(
        &self,
        until_timestamp: Option<i64>,
    ) -> impl Stream<Item = Result<ChangesetContext, MononokeError>> + '_ {
        let cs_info_enabled = self.repo.derive_changeset_info_enabled();

        let terminate = until_timestamp.map(move |until_timestamp| {
            move |changeset_id| async move {
                let info = if cs_info_enabled {
                    ChangesetInfo::derive(
                        self.ctx().clone(),
                        self.repo().blob_repo().clone(),
                        changeset_id,
                    )
                    .compat()
                    .await?
                } else {
                    let bonsai = changeset_id
                        .load(self.ctx().clone(), self.repo().blob_repo().blobstore())
                        .await?;
                    ChangesetInfo::new(changeset_id, bonsai)
                };
                let date = info.author_date().as_chrono().clone();
                Ok::<_, MononokeError>(date.timestamp() < until_timestamp)
            }
        });

        stream::try_unfold(
            // starting state
            (hashset! { self.id() }, VecDeque::from(vec![self.id()])),
            // unfold
            move |(mut visited, mut queue)| async move {
                if let Some(changeset_id) = queue.pop_front() {
                    if let Some(terminate) = terminate {
                        if terminate(changeset_id).await? {
                            return Ok(Some((None, (visited, queue))));
                        }
                    }
                    let parents = self
                        .repo()
                        .blob_repo()
                        .get_changeset_parents_by_bonsai(self.ctx().clone(), changeset_id)
                        .compat()
                        .await?;
                    queue.extend(parents.into_iter().filter(|parent| visited.insert(*parent)));
                    Ok(Some((Some(changeset_id), (visited, queue))))
                } else {
                    Ok::<_, MononokeError>(None)
                }
            },
        )
        .try_filter_map(move |changeset_id| {
            let changeset = changeset_id
                .map(|changeset_id| ChangesetContext::new(self.repo().clone(), changeset_id));
            async move { Ok::<_, MononokeError>(changeset) }
        })
        .boxed()
    }
}
