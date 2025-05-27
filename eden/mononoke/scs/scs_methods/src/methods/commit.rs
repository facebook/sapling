/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::collections::HashMap;
use std::sync::Arc;

use bytes::Bytes;
use cloned::cloned;
use context::CoreContext;
use futures::pin_mut;
use futures::stream;
use futures::stream::BoxStream;
use futures::stream::FuturesOrdered;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use futures::try_join;
use futures_watchdog::WatchdogExt;
use hooks::HookExecution;
use hooks::HookOutcome;
use itertools::Either;
use itertools::Itertools;
use maplit::btreeset;
use mononoke_api::CandidateSelectionHintArgs;
use mononoke_api::ChangesetContext;
use mononoke_api::ChangesetDiffItem;
use mononoke_api::ChangesetFileOrdering;
use mononoke_api::ChangesetHistoryOptions;
use mononoke_api::ChangesetId;
use mononoke_api::ChangesetLinearHistoryOptions;
use mononoke_api::ChangesetPathContentContext;
use mononoke_api::ChangesetPathDiffContext;
use mononoke_api::ChangesetSpecifier;
use mononoke_api::CopyInfo;
use mononoke_api::MetadataDiff;
use mononoke_api::MononokeError;
use mononoke_api::MononokeRepo;
use mononoke_api::Repo;
use mononoke_api::RepoContext;
use mononoke_api::UnifiedDiff;
use mononoke_api::UnifiedDiffMode;
use mononoke_api::XRepoLookupExactBehaviour;
use mononoke_api::XRepoLookupSyncBehaviour;
use mononoke_macros::mononoke;
use mononoke_types::path::MPath;
use scs_errors::ServiceErrorResultExt;
use source_control as thrift;

use crate::commit_id::map_commit_identities;
use crate::commit_id::map_commit_identity;
use crate::from_request::FromRequest;
use crate::from_request::check_range_and_convert;
use crate::from_request::validate_timestamp;
use crate::history::collect_history;
use crate::into_response::AsyncIntoResponse;
use crate::into_response::AsyncIntoResponseWith;
use crate::into_response::IntoResponse;
use crate::source_control_impl::SourceControlServiceImpl;

// Magic number used when we want to limit concurrency with buffer_unordered.
const CONCURRENCY_LIMIT: usize = 100;

enum CommitComparePath {
    File(thrift::CommitCompareFile),
    Tree(thrift::CommitCompareTree),
}

impl CommitComparePath {
    /// The main path that this comparison applies to.
    fn path(&self) -> Result<&str, scs_errors::ServiceError> {
        // Use the base path where available.  If it is not available, then
        // this is a deletion and the other path should be used.
        match self {
            CommitComparePath::File(file) => file
                .base_file
                .as_ref()
                .or(file.other_file.as_ref())
                .map(|file| file.path.as_str())
                .ok_or_else(|| {
                    scs_errors::internal_error("programming error, file entry has no file").into()
                }),

            CommitComparePath::Tree(tree) => tree
                .base_tree
                .as_ref()
                .or(tree.other_tree.as_ref())
                .map(|tree| tree.path.as_str())
                .ok_or_else(|| {
                    scs_errors::internal_error("programming error, tree entry has no tree").into()
                }),
        }
    }

    async fn from_path_diff(
        path_diff: ChangesetPathDiffContext<Repo>,
    ) -> Result<Self, scs_errors::ServiceError> {
        if path_diff.path().is_file().await? {
            let (base_file, other_file) = try_join!(
                path_diff.base().into_response(),
                path_diff.other().into_response()
            )?;
            let copy_info = path_diff.copy_info().into_response();
            Ok(CommitComparePath::File(thrift::CommitCompareFile {
                base_file,
                other_file,
                copy_info,
                ..Default::default()
            }))
        } else {
            let (base_tree, other_tree) = try_join!(
                path_diff.base().into_response(),
                path_diff.other().into_response()
            )?;
            Ok(CommitComparePath::Tree(thrift::CommitCompareTree {
                base_tree,
                other_tree,
                ..Default::default()
            }))
        }
    }
}

/// Helper for commit_compare to add mutable rename information if appropriate
async fn add_mutable_renames(
    base_changeset: &mut ChangesetContext<Repo>,
    params: &thrift::CommitCompareParams,
) -> Result<(), scs_errors::ServiceError> {
    if params.follow_mutable_file_history.unwrap_or(false) {
        if let Some(paths) = &params.paths {
            let paths: Vec<_> = paths
                .iter()
                .map(MPath::try_from)
                .collect::<Result<Vec<_>, _>>()
                .map_err(|error| MononokeError::InvalidRequest(error.to_string()))?;
            base_changeset
                .add_mutable_renames(paths.into_iter())
                .await?;
        }
    }
    Ok(())
}

struct CommitFileDiffsItem {
    path_diff_context: ChangesetPathDiffContext<Repo>,
    placeholder: bool,
}

impl CommitFileDiffsItem {
    fn to_stopped_at_pair(&self) -> thrift::CommitFileDiffsStoppedAtPair {
        thrift::CommitFileDiffsStoppedAtPair {
            base_path: self.path_diff_context.base().map(|p| p.path().to_string()),
            other_path: self.path_diff_context.other().map(|p| p.path().to_string()),
            ..Default::default()
        }
    }

    async fn total_size(&self) -> Result<u64, scs_errors::ServiceError> {
        if self.placeholder {
            Ok(0)
        } else {
            async fn file_size(
                path: Option<&ChangesetPathContentContext<Repo>>,
            ) -> Result<u64, scs_errors::ServiceError> {
                if let Some(path) = path {
                    if let Some(file) = path.file().await? {
                        return Ok(file.metadata().await?.total_size);
                    }
                }
                Ok(0)
            }
            let (base_size, other_size) = try_join!(
                file_size(self.path_diff_context.base()),
                file_size(self.path_diff_context.other())
            )?;
            Ok(base_size.saturating_add(other_size))
        }
    }

    async fn response_element(
        &self,
        ctx: &CoreContext,
        format: thrift::DiffFormat,
        context_lines: usize,
    ) -> Result<CommitFileDiffsResponseElement, scs_errors::ServiceError> {
        match format {
            thrift::DiffFormat::RAW_DIFF => self.raw_diff(ctx, context_lines).await,
            thrift::DiffFormat::METADATA_DIFF => self.metadata_diff(ctx).await,
            unknown => Err(scs_errors::invalid_request(format!(
                "invalid diff format: {:?}",
                unknown
            ))
            .into()),
        }
    }

    async fn raw_diff(
        &self,
        ctx: &CoreContext,
        context_lines: usize,
    ) -> Result<CommitFileDiffsResponseElement, scs_errors::ServiceError> {
        let mode = if self.placeholder {
            UnifiedDiffMode::OmitContent
        } else {
            UnifiedDiffMode::Inline
        };
        let diff = self
            .path_diff_context
            .unified_diff(ctx, context_lines, mode)
            .await?;
        Ok(CommitFileDiffsResponseElement::RawDiff { diff })
    }

    async fn metadata_diff(
        &self,
        ctx: &CoreContext,
    ) -> Result<CommitFileDiffsResponseElement, scs_errors::ServiceError> {
        let metadata_diff = self.path_diff_context.metadata_diff(ctx).await?;
        Ok(CommitFileDiffsResponseElement::MetadataDiff { metadata_diff })
    }
}

enum CommitFileDiffsResponseElement {
    RawDiff { diff: UnifiedDiff },
    MetadataDiff { metadata_diff: MetadataDiff },
}

impl CommitFileDiffsResponseElement {
    fn size(&self) -> usize {
        match self {
            Self::RawDiff { diff } => diff.raw_diff.len(),
            Self::MetadataDiff { .. } => 1,
        }
    }

    fn into_response_for_item(
        self,
        item: &CommitFileDiffsItem,
    ) -> thrift::CommitFileDiffsResponseElement {
        match self {
            Self::RawDiff { diff } => thrift::CommitFileDiffsResponseElement {
                base_path: item.path_diff_context.base().map(|p| p.path().to_string()),
                other_path: item.path_diff_context.other().map(|p| p.path().to_string()),
                diff: diff.into_response(),
                ..Default::default()
            },
            Self::MetadataDiff { metadata_diff } => thrift::CommitFileDiffsResponseElement {
                base_path: item.path_diff_context.base().map(|p| p.path().to_string()),
                other_path: item.path_diff_context.other().map(|p| p.path().to_string()),
                diff: metadata_diff.into_response(),
                ..Default::default()
            },
        }
    }
}

impl SourceControlServiceImpl {
    /// Returns the lowest common ancestor of two commits.
    ///
    /// In case of ambiguity (can happen with multiple merges of the same branches) returns the
    /// common ancestor with lowest id out of those with highest generation number.
    pub(crate) async fn commit_common_base_with(
        &self,
        ctx: CoreContext,
        commit: thrift::CommitSpecifier,
        params: thrift::CommitCommonBaseWithParams,
    ) -> Result<thrift::CommitLookupResponse, scs_errors::ServiceError> {
        let (_repo, changeset, other_changeset) = self
            .repo_changeset_pair(ctx.clone(), &commit, &params.other_commit_id)
            .watched(ctx.logger())
            .await?;
        let lca = changeset
            .common_base_with(other_changeset.id())
            .watched(ctx.logger())
            .await?;
        Ok(thrift::CommitLookupResponse {
            exists: lca.is_some(),
            ids: if let Some(lca) = lca {
                Some(
                    map_commit_identity(&lca, &params.identity_schemes)
                        .watched(ctx.logger())
                        .await?,
                )
            } else {
                None
            },
            ..Default::default()
        })
    }

    /// Look up commit.
    pub(crate) async fn commit_lookup(
        &self,
        ctx: CoreContext,
        commit: thrift::CommitSpecifier,
        params: thrift::CommitLookupParams,
    ) -> Result<thrift::CommitLookupResponse, scs_errors::ServiceError> {
        let repo = self.repo(ctx, &commit.repo).await?;
        match repo
            .changeset(ChangesetSpecifier::from_request(&commit.id)?)
            .await?
        {
            Some(cs) => {
                let ids = map_commit_identity(&cs, &params.identity_schemes).await?;
                Ok(thrift::CommitLookupResponse {
                    exists: true,
                    ids: Some(ids),
                    ..Default::default()
                })
            }
            None => Ok(thrift::CommitLookupResponse {
                exists: false,
                ids: None,
                ..Default::default()
            }),
        }
    }

    /// Get diff.
    pub(crate) async fn commit_file_diffs(
        &self,
        ctx: CoreContext,
        commit: thrift::CommitSpecifier,
        params: thrift::CommitFileDiffsParams,
    ) -> Result<thrift::CommitFileDiffsResponse, scs_errors::ServiceError> {
        // Check the path count limit
        if params.paths.len() as i64 > thrift::consts::COMMIT_FILE_DIFFS_PATH_COUNT_LIMIT {
            Err(scs_errors::diff_input_too_many_paths(params.paths.len()))?;
        }

        // Resolve the CommitSpecfier into ChangesetContext
        let (base_commit, other_commit) = match params.other_commit_id {
            Some(other_commit_id) => {
                let (_repo, base_commit, other_commit) = self
                    .repo_changeset_pair(ctx.clone(), &commit, &other_commit_id)
                    .await?;
                (base_commit, Some(other_commit))
            }
            None => {
                let (_repo, base_commit) = self.repo_changeset(ctx.clone(), &commit).await?;
                (base_commit, None)
            }
        };

        // Resolve the paths into ChangesetPathContentContext
        // To make it more efficient we do a batch request
        // to resolve all paths into path contexts
        let mut base_commit_paths = vec![];
        let mut other_commit_paths = vec![];
        let paths = params
            .paths
            .into_iter()
            .map(|path_pair| {
                Ok((
                    match path_pair.base_path {
                        Some(path) => {
                            let mpath = MPath::try_from(&path)
                                .map_err(|error| MononokeError::InvalidRequest(error.to_string()))
                                .context("invalid base commit path")?;
                            base_commit_paths.push(mpath.clone());
                            Some(mpath)
                        }
                        None => None,
                    },
                    match &other_commit {
                        Some(_other_commit) => match path_pair.other_path {
                            Some(path) => {
                                let mpath = MPath::try_from(&path)
                                    .map_err(|error| {
                                        MononokeError::InvalidRequest(error.to_string())
                                    })
                                    .context("invalid other commit path")?;
                                other_commit_paths.push(mpath.clone());
                                Some(mpath)
                            }
                            None => None,
                        },
                        None => None,
                    },
                    CopyInfo::from_request(&path_pair.copy_info)?,
                    path_pair.generate_placeholder_diff.unwrap_or(false),
                ))
            })
            .collect::<Result<Vec<_>, scs_errors::ServiceError>>()?;

        let (base_commit_contexts, other_commit_contexts) = try_join!(
            async {
                let base_commit_paths = base_commit
                    .paths_with_content(base_commit_paths.into_iter())
                    .await?;
                let base_commit_contexts = base_commit_paths
                    .map_ok(|path_context| (path_context.path().clone(), path_context))
                    .try_collect::<HashMap<_, _>>()
                    .await?;
                Ok::<_, MononokeError>(base_commit_contexts)
            },
            async {
                match &other_commit {
                    None => Ok(None),
                    Some(other_commit) => {
                        let other_commit_paths = other_commit
                            .paths_with_content(other_commit_paths.into_iter())
                            .await?;
                        let other_commit_contexts = other_commit_paths
                            .map_ok(|path_context| (path_context.path().clone(), path_context))
                            .try_collect::<HashMap<_, _>>()
                            .await?;
                        Ok::<_, MononokeError>(Some(other_commit_contexts))
                    }
                }
            }
        )?;

        let items = paths
            .into_iter()
            .map(|(base_path, other_path, copy_info, placeholder)| {
                let base_path = match base_path {
                    Some(base_path) => {
                        Some(base_commit_contexts.get(&base_path).ok_or_else(|| {
                            scs_errors::invalid_request(format!(
                                "{} not found in {:?}",
                                base_path, commit
                            ))
                        })?)
                    }
                    None => None,
                };

                let other_path = match other_path {
                    Some(other_path) => match &other_commit_contexts {
                        Some(other_commit_contexts) => {
                            Some(other_commit_contexts.get(&other_path).ok_or_else(|| {
                                scs_errors::invalid_request(format!(
                                    "{} not found in {:?}",
                                    other_path, other_commit
                                ))
                            })?)
                        }
                        None => None,
                    },
                    None => None,
                };

                let path_diff_context = ChangesetPathDiffContext::new(
                    base_path.cloned(),
                    other_path.cloned(),
                    copy_info,
                )?;
                Ok(CommitFileDiffsItem {
                    path_diff_context,
                    placeholder,
                })
            })
            .collect::<Result<Vec<_>, scs_errors::ServiceError>>()?;

        // Check the total file size limit
        let total_input_size = stream::iter(items.iter())
            .map(|item| item.total_size())
            .boxed() // Prevents compiler error
            .buffered(100)
            .try_fold(
                0u64,
                |acc, size| async move { Ok(acc.saturating_add(size)) },
            )
            .await?;

        if total_input_size > thrift::consts::COMMIT_FILE_DIFFS_SIZE_LIMIT as u64 {
            Err(scs_errors::diff_input_too_big(total_input_size))?;
        }

        let context = check_range_and_convert("context", params.context, 0..)?;
        let diff_size_limit: Option<usize> = params
            .diff_size_limit
            .map(|limit| check_range_and_convert("diff_size_limit", limit, 0..))
            .transpose()?;
        let mut size_so_far = 0usize;
        let mut stopped_at_pair = None;

        let path_diffs = stream::iter(items)
            .map(|item| {
                cloned!(ctx);
                async move {
                    let element = item.response_element(&ctx, params.format, context).await?;
                    Ok::<_, scs_errors::ServiceError>((item, element))
                }
            })
            .boxed() // Prevents compiler error
            .buffered(20)
            .try_take_while(|(item, element)| {
                let mut limit_reached = false;
                if let Some(diff_size_limit) = diff_size_limit {
                    size_so_far = size_so_far.saturating_add(element.size());
                    if size_so_far > diff_size_limit {
                        limit_reached = true;
                        stopped_at_pair = Some(item.to_stopped_at_pair());
                    }
                }
                async move { Ok(!limit_reached) }
            })
            .map_ok(|(item, element)| element.into_response_for_item(&item))
            .try_collect()
            .await?;

        Ok(thrift::CommitFileDiffsResponse {
            path_diffs,
            stopped_at_pair,
            ..Default::default()
        })
    }

    /// Get commit info.
    pub(crate) async fn commit_info(
        &self,
        ctx: CoreContext,
        commit: thrift::CommitSpecifier,
        params: thrift::CommitInfoParams,
    ) -> Result<thrift::CommitInfo, scs_errors::ServiceError> {
        let (_repo, changeset) = self.repo_changeset(ctx, &commit).await?;
        changeset.into_response_with(&params.identity_schemes).await
    }

    /// Get commit generation.
    pub(crate) async fn commit_generation(
        &self,
        ctx: CoreContext,
        commit: thrift::CommitSpecifier,
        _params: thrift::CommitGenerationParams,
    ) -> Result<i64, scs_errors::ServiceError> {
        let (_repo, changeset) = self.repo_changeset(ctx, &commit).await?;
        Ok(changeset.generation().await?.value() as i64)
    }

    /// Returns `true` if this commit is an ancestor of `other_commit`.
    pub(crate) async fn commit_is_ancestor_of(
        &self,
        ctx: CoreContext,
        commit: thrift::CommitSpecifier,
        params: thrift::CommitIsAncestorOfParams,
    ) -> Result<bool, scs_errors::ServiceError> {
        let (_repo, changeset, other_changeset) = self
            .repo_changeset_pair(ctx, &commit, &params.descendant_commit_id)
            .await?;
        let is_ancestor_of = changeset.is_ancestor_of(other_changeset.id()).await?;
        Ok(is_ancestor_of)
    }

    /// Given a base changeset, find the "other" changeset from parent information
    /// including mutable history if appropriate
    ///
    /// This is entirely a heuristic to guess the "right" thing if the client
    /// doesn't provide an "other" changeset - errors would normally be fed back
    /// to a human and not handled automatically.
    async fn find_commit_compare_parent(
        &self,
        repo: &RepoContext<Repo>,
        base_changeset: &mut ChangesetContext<Repo>,
        params: &thrift::CommitCompareParams,
    ) -> Result<Option<ChangesetContext<Repo>>, scs_errors::ServiceError> {
        let commit_parents = base_changeset.parents().await?;
        let mut other_changeset_id = commit_parents.first().copied();

        if params.follow_mutable_file_history.unwrap_or(false) {
            let mutable_parents = base_changeset.mutable_parents();

            // If there are multiple choices to make, then bail - the user needs to be
            // clear to avoid the ambiguity
            if mutable_parents.len() > 1 {
                return Err(scs_errors::invalid_request(
                    "multiple different mutable parents in supplied paths",
                )
                .into());
            }
            if let Some(Some(parent)) = mutable_parents.into_iter().next() {
                other_changeset_id = Some(parent);
            }
        }

        match other_changeset_id {
            None => Ok(None),
            Some(other_changeset_id) => {
                let other_changeset = repo
                    .changeset(ChangesetSpecifier::Bonsai(other_changeset_id))
                    .await?
                    .ok_or_else(|| scs_errors::internal_error("other changeset is missing"))?;
                Ok(Some(other_changeset))
            }
        }
    }

    /// Diff two commits
    pub(crate) async fn commit_compare(
        &self,
        ctx: CoreContext,
        commit: thrift::CommitSpecifier,
        params: thrift::CommitCompareParams,
    ) -> Result<thrift::CommitCompareResponse, scs_errors::ServiceError> {
        let (base_changeset, other_changeset) = match &params.other_commit_id {
            Some(id) => {
                let (_repo, mut base_changeset, other_changeset) = self
                    .repo_changeset_pair(ctx.clone(), &commit, id)
                    .watched(ctx.logger())
                    .await?;
                add_mutable_renames(&mut base_changeset, &params)
                    .watched(ctx.logger())
                    .await?;
                (base_changeset, Some(other_changeset))
            }
            None => {
                let (repo, mut base_changeset) = self
                    .repo_changeset(ctx.clone(), &commit)
                    .watched(ctx.logger())
                    .await?;
                add_mutable_renames(&mut base_changeset, &params)
                    .watched(ctx.logger())
                    .await?;
                let other_changeset = self
                    .find_commit_compare_parent(&repo, &mut base_changeset, &params)
                    .watched(ctx.logger())
                    .await?;
                (base_changeset, other_changeset)
            }
        };

        // Log the generation difference to drill down on clients making
        // expensive `commit_compare` requests
        let base_generation = base_changeset
            .generation()
            .watched(ctx.logger())
            .await?
            .value();
        let other_generation = match other_changeset {
            Some(ref cs) => cs.generation().watched(ctx.logger()).await?.value(),
            // If there isn't another commit, let's use the same generation
            // to have a difference of 0.
            None => base_generation,
        };

        let generation_diff = base_generation.abs_diff(other_generation);
        let mut scuba = ctx.scuba().clone();
        scuba.log_with_msg(
            "Commit compare generation difference",
            format!("{generation_diff}"),
        );

        let mut last_path = None;
        let mut diff_items: BTreeSet<_> = params
            .compare_items
            .into_iter()
            .filter_map(|item| match item {
                thrift::CommitCompareItem::FILES => Some(ChangesetDiffItem::FILES),
                thrift::CommitCompareItem::TREES => Some(ChangesetDiffItem::TREES),
                _ => None,
            })
            .collect();

        if diff_items.is_empty() {
            diff_items = btreeset! { ChangesetDiffItem::FILES };
        }

        let paths: Option<Vec<MPath>> = match params.paths {
            None => None,
            Some(paths) => Some(
                paths
                    .iter()
                    .map(MPath::try_from)
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|error| MononokeError::InvalidRequest(error.to_string()))?,
            ),
        };
        let (diff_files, diff_trees) = match params.ordered_params {
            None => {
                let diff = match other_changeset {
                    Some(ref other_changeset) => {
                        base_changeset
                            .diff_unordered(
                                other_changeset,
                                !params.skip_copies_renames,
                                paths,
                                diff_items,
                            )
                            .watched(ctx.logger())
                            .await?
                    }
                    None => {
                        base_changeset
                            .diff_root_unordered(paths, diff_items)
                            .watched(ctx.logger())
                            .await?
                    }
                };
                stream::iter(diff)
                    .map(CommitComparePath::from_path_diff)
                    .buffer_unordered(CONCURRENCY_LIMIT)
                    .try_collect::<Vec<_>>()
                    .watched(ctx.logger())
                    .await?
                    .into_iter()
                    .partition_map(|diff| match diff {
                        CommitComparePath::File(entry) => Either::Left(entry),
                        CommitComparePath::Tree(entry) => Either::Right(entry),
                    })
            }
            Some(ordered_params) => {
                let limit: usize = check_range_and_convert(
                    "limit",
                    ordered_params.limit,
                    0..=source_control::COMMIT_COMPARE_ORDERED_MAX_LIMIT,
                )?;
                let after = ordered_params
                    .after_path
                    .map(|after| {
                        MPath::try_from(&after).map_err(|e| {
                            scs_errors::invalid_request(format!(
                                "invalid continuation path '{}': {}",
                                after, e
                            ))
                        })
                    })
                    .transpose()?;
                let diff = match other_changeset {
                    Some(ref other_changeset) => {
                        base_changeset
                            .diff(
                                other_changeset,
                                !params.skip_copies_renames,
                                paths,
                                diff_items,
                                ChangesetFileOrdering::Ordered { after },
                                Some(limit),
                            )
                            .watched(ctx.logger())
                            .await?
                    }
                    None => {
                        base_changeset
                            .diff_root(
                                paths,
                                diff_items,
                                ChangesetFileOrdering::Ordered { after },
                                Some(limit),
                            )
                            .watched(ctx.logger())
                            .await?
                    }
                };
                let diff_items = diff
                    .into_iter()
                    .map(CommitComparePath::from_path_diff)
                    .collect::<FuturesOrdered<_>>()
                    .try_collect::<Vec<_>>()
                    .watched(ctx.logger())
                    .await?;
                if diff_items.len() >= limit {
                    if let Some(item) = diff_items.last() {
                        last_path = Some(item.path()?.to_string());
                    }
                }
                diff_items.into_iter().partition_map(|diff| match diff {
                    CommitComparePath::File(entry) => Either::Left(entry),
                    CommitComparePath::Tree(entry) => Either::Right(entry),
                })
            }
        };

        let other_commit_ids = match other_changeset {
            None => None,
            Some(other_changeset) => Some(
                map_commit_identity(&other_changeset, &params.identity_schemes)
                    .watched(ctx.logger())
                    .await?,
            ),
        };
        Ok(thrift::CommitCompareResponse {
            diff_files,
            diff_trees,
            other_commit_ids,
            last_path,
            ..Default::default()
        })
    }

    /// Returns files that match the criteria
    pub(crate) async fn commit_find_files(
        &self,
        ctx: CoreContext,
        commit: thrift::CommitSpecifier,
        params: thrift::CommitFindFilesParams,
    ) -> Result<thrift::CommitFindFilesResponse, scs_errors::ServiceError> {
        let (_repo, changeset) = self.repo_changeset(ctx.clone(), &commit).await?;
        let limit: usize = check_range_and_convert(
            "limit",
            params.limit,
            0..=source_control::COMMIT_FIND_FILES_MAX_LIMIT,
        )?;
        let prefixes: Option<Vec<_>> = match params.prefixes {
            Some(prefixes) => Some(
                prefixes
                    .into_iter()
                    .map(|prefix| {
                        MPath::try_from(&prefix).map_err(|e| {
                            scs_errors::invalid_request(format!(
                                "invalid prefix '{}': {}",
                                prefix, e
                            ))
                        })
                    })
                    .collect::<Result<Vec<_>, _>>()?,
            ),
            None => None,
        };
        let ordering = match &params.after {
            Some(after) => {
                let after = Some(MPath::try_from(after).map_err(|e| {
                    scs_errors::invalid_request(format!(
                        "invalid continuation path '{}': {}",
                        after, e
                    ))
                })?);
                ChangesetFileOrdering::Ordered { after }
            }
            None => ChangesetFileOrdering::Unordered,
        };

        let files = changeset
            .find_files(
                prefixes,
                params.basenames,
                params.basename_suffixes,
                ordering,
            )
            .await?
            .take(limit)
            .map_ok(|path| path.to_string())
            .try_collect()
            .await?;

        Ok(thrift::CommitFindFilesResponse {
            files,
            ..Default::default()
        })
    }

    /// Returns files that match the criteria
    pub(crate) async fn commit_find_files_stream(
        &self,
        ctx: CoreContext,
        commit: thrift::CommitSpecifier,
        params: thrift::CommitFindFilesParams,
    ) -> Result<
        (
            thrift::CommitFindFilesStreamResponse,
            BoxStream<'static, Result<thrift::CommitFindFilesStreamItem, scs_errors::ServiceError>>,
        ),
        scs_errors::ServiceError,
    > {
        let (_repo, changeset) = self.repo_changeset(ctx.clone(), &commit).await?;
        let limit: usize = check_range_and_convert("limit", params.limit, 0..=i64::MAX)?;
        let prefixes: Option<Vec<_>> = match &params.prefixes {
            Some(prefixes) => Some(
                prefixes
                    .iter()
                    .map(|prefix| {
                        MPath::try_from(prefix).map_err(|e| {
                            scs_errors::invalid_request(format!(
                                "invalid prefix '{}': {}",
                                prefix, e
                            ))
                        })
                    })
                    .collect::<Result<Vec<_>, _>>()?,
            ),
            None => None,
        };
        let ordering = match &params.after {
            Some(after) => {
                let after = Some(MPath::try_from(after).map_err(|e| {
                    scs_errors::invalid_request(format!(
                        "invalid continuation path '{}': {}",
                        after, e
                    ))
                })?);
                ChangesetFileOrdering::Ordered { after }
            }
            None => ChangesetFileOrdering::Unordered,
        };

        let files_stream = (async_stream::stream! {
            let s = changeset
            .find_files(
                prefixes,
                params.basenames,
                params.basename_suffixes,
                ordering,
            )
            .await?
            .take(limit)
            .map_ok(|path| path.to_string())
            .try_chunks(1000)
            .map_ok(|files| thrift::CommitFindFilesStreamItem {
                files,
                ..Default::default()
            })
            .map_err(|err| scs_errors::ServiceError::from(err.1)).boxed();
            pin_mut!(s);
            while let Some(value) = s.next().await {
                yield value;
            }
        })
        .boxed();

        Ok((
            thrift::CommitFindFilesStreamResponse {
                ..Default::default()
            },
            files_stream,
        ))
    }
    /// Returns the history of a commit
    pub(crate) async fn commit_history(
        &self,
        ctx: CoreContext,
        commit: thrift::CommitSpecifier,
        params: thrift::CommitHistoryParams,
    ) -> Result<thrift::CommitHistoryResponse, scs_errors::ServiceError> {
        let (repo, changeset) = self.repo_changeset(ctx.clone(), &commit).await?;
        let (descendants_of, exclude_changeset_and_ancestors) = try_join!(
            async {
                if let Some(descendants_of) = &params.descendants_of {
                    Ok::<_, scs_errors::ServiceError>(Some(
                        self.changeset_id(&repo, descendants_of).await?,
                    ))
                } else {
                    Ok(None)
                }
            },
            async {
                if let Some(exclude_changeset_and_ancestors) =
                    &params.exclude_changeset_and_ancestors
                {
                    Ok::<_, scs_errors::ServiceError>(Some(
                        self.changeset_id(&repo, exclude_changeset_and_ancestors)
                            .await?,
                    ))
                } else {
                    Ok(None)
                }
            }
        )?;

        let limit: usize = check_range_and_convert("limit", params.limit, 0..)?;
        let skip: usize = check_range_and_convert("skip", params.skip, 0..)?;

        // Time filter equal to zero might be mistaken by users for an unset, like None.
        // We will consider negative timestamps as invalid and zeros as unset.
        let after_timestamp = validate_timestamp(params.after_timestamp, "after_timestamp")?;
        let before_timestamp = validate_timestamp(params.before_timestamp, "before_timestamp")?;

        if let (Some(ats), Some(bts)) = (after_timestamp, before_timestamp) {
            if bts < ats {
                return Err(scs_errors::invalid_request(format!(
                    "after_timestamp ({}) cannot be greater than before_timestamp ({})",
                    ats, bts,
                ))
                .into());
            }
        }

        if skip > 0 && (after_timestamp.is_some() || before_timestamp.is_some()) {
            return Err(scs_errors::invalid_request(
                "Time filters cannot be applied if skip is not 0".to_string(),
            )
            .into());
        }

        let history_stream = changeset
            .history(ChangesetHistoryOptions {
                until_timestamp: after_timestamp,
                descendants_of,
                exclude_changeset_and_ancestors,
            })
            .await?;
        let history = collect_history(
            &ctx,
            history_stream,
            skip,
            limit,
            before_timestamp,
            after_timestamp,
            params.format,
            &params.identity_schemes,
        )
        .await?;

        Ok(thrift::CommitHistoryResponse {
            history,
            ..Default::default()
        })
    }

    pub async fn commit_linear_history(
        &self,
        ctx: CoreContext,
        commit: thrift::CommitSpecifier,
        params: thrift::CommitLinearHistoryParams,
    ) -> Result<thrift::CommitLinearHistoryResponse, scs_errors::ServiceError> {
        let (repo, changeset) = self.repo_changeset(ctx.clone(), &commit).await?;
        let (descendants_of, exclude_changeset_and_ancestors) = try_join!(
            async {
                if let Some(descendants_of) = &params.descendants_of {
                    Ok::<_, scs_errors::ServiceError>(Some(
                        self.changeset_id(&repo, descendants_of).await?,
                    ))
                } else {
                    Ok(None)
                }
            },
            async {
                if let Some(exclude_changeset_and_ancestors) =
                    &params.exclude_changeset_and_ancestors
                {
                    Ok::<_, scs_errors::ServiceError>(Some(
                        self.changeset_id(&repo, exclude_changeset_and_ancestors)
                            .await?,
                    ))
                } else {
                    Ok(None)
                }
            }
        )?;

        let limit: usize = check_range_and_convert("limit", params.limit, 0..)?;
        let skip: u64 = check_range_and_convert("skip", params.skip, 0..)?;

        let history_stream = changeset
            .linear_history(ChangesetLinearHistoryOptions {
                descendants_of,
                exclude_changeset_and_ancestors,
                skip,
            })
            .await?;
        let history = collect_history(
            &ctx,
            history_stream,
            // We set the skip to 0 as skipping is already done as part of ChangesetContext::linear_history.
            0,
            limit,
            None,
            None,
            params.format,
            &params.identity_schemes,
        )
        .await?;

        Ok(thrift::CommitLinearHistoryResponse {
            history,
            ..Default::default()
        })
    }

    pub(crate) async fn commit_list_descendant_bookmarks(
        &self,
        ctx: CoreContext,
        commit: thrift::CommitSpecifier,
        params: thrift::CommitListDescendantBookmarksParams,
    ) -> Result<thrift::CommitListDescendantBookmarksResponse, scs_errors::ServiceError> {
        let limit = match check_range_and_convert(
            "limit",
            params.limit,
            0..=source_control::COMMIT_LIST_DESCENDANT_BOOKMARKS_MAX_LIMIT,
        )? {
            0 => None,
            limit => Some(limit),
        };
        let prefix = if !params.bookmark_prefix.is_empty() {
            Some(params.bookmark_prefix)
        } else {
            None
        };
        let (repo, changeset) = self.repo_changeset(ctx, &commit).await?;
        let bookmarks = repo
            .list_bookmarks(
                params.include_scratch,
                prefix.as_deref(),
                params.after.as_deref(),
                limit,
            )
            .await?
            .try_collect::<Vec<_>>()
            .await?;
        let continue_after = match limit {
            Some(limit) if bookmarks.len() as u64 >= limit => {
                bookmarks.last().map(|bookmark| bookmark.0.clone())
            }
            _ => None,
        };

        async fn filter_descendant(
            changeset: Arc<ChangesetContext<Repo>>,
            bookmark: (String, ChangesetId),
        ) -> Result<Option<(String, ChangesetId)>, MononokeError> {
            if changeset.is_ancestor_of(bookmark.1).await? {
                Ok(Some(bookmark))
            } else {
                Ok(None)
            }
        }

        let bookmarks = stream::iter(bookmarks)
            .map({
                // Wrap `changeset` in `Arc` so that cloning it to send to
                // the tasks is cheap.
                let changeset = Arc::new(changeset);
                move |bookmark| {
                    let changeset = changeset.clone();
                    async move {
                        mononoke::spawn_task(filter_descendant(changeset, bookmark))
                            .await
                            .map_err(anyhow::Error::from)?
                    }
                }
            })
            .buffered(20)
            .try_fold(Vec::new(), |mut bookmarks, maybe_bookmark| async move {
                if let Some(bookmark) = maybe_bookmark {
                    bookmarks.push(bookmark);
                }
                Ok(bookmarks)
            })
            .await?;

        let ids = bookmarks.iter().map(|(_name, cs_id)| *cs_id).collect();
        let id_mapping = map_commit_identities(&repo, ids, &params.identity_schemes).await?;

        let bookmarks = bookmarks
            .into_iter()
            .map(|(name, cs_id)| (name, id_mapping.get(&cs_id).cloned().unwrap_or_default()))
            .collect();

        Ok(thrift::CommitListDescendantBookmarksResponse {
            bookmarks,
            continue_after,
            ..Default::default()
        })
    }

    pub(crate) async fn commit_run_hooks(
        &self,
        ctx: CoreContext,
        commit: thrift::CommitSpecifier,
        params: thrift::CommitRunHooksParams,
    ) -> Result<thrift::CommitRunHooksResponse, scs_errors::ServiceError> {
        let (_repo, changeset) = self.repo_changeset(ctx, &commit).await?;
        let pushvars: Option<HashMap<String, Bytes>> = params
            .pushvars
            .map(|p| p.into_iter().map(|(k, v)| (k, Bytes::from(v))).collect());
        let outcomes = changeset
            .run_hooks(params.bookmark, pushvars.as_ref())
            .await?;

        let mut outcomes_map = BTreeMap::new();

        for outcome in outcomes {
            let (name, execution) = match outcome {
                HookOutcome::FileHook(id, exec) => (id.hook_name, exec),
                HookOutcome::BookmarkHook(id, exec) => (id.hook_name, exec),
                HookOutcome::ChangesetHook(id, exec) => (id.hook_name, exec),
            };

            match execution {
                HookExecution::Accepted => {
                    outcomes_map.entry(name).or_insert_with(|| {
                        thrift::HookOutcome::accepted(thrift::HookOutcomeAccepted {
                            ..Default::default()
                        })
                    });
                }
                HookExecution::Rejected(rej) => {
                    let rejection = thrift::HookOutcomeRejected {
                        description: rej.description.to_string(),
                        long_description: rej.long_description,
                        ..Default::default()
                    };

                    match outcomes_map
                        .entry(name)
                        .or_insert_with(|| thrift::HookOutcome::rejections(vec![]))
                    {
                        thrift::HookOutcome::rejections(rejs) => rejs.push(rejection),
                        obj => *obj = thrift::HookOutcome::rejections(vec![rejection]),
                    }
                }
            }
        }

        Ok(thrift::CommitRunHooksResponse {
            outcomes: outcomes_map,
            ..Default::default()
        })
    }

    pub(crate) async fn commit_subtree_changes(
        &self,
        ctx: CoreContext,
        commit: thrift::CommitSpecifier,
        params: thrift::CommitSubtreeChangesParams,
    ) -> Result<thrift::CommitSubtreeChangesResponse, scs_errors::ServiceError> {
        let (repo, changeset) = self.repo_changeset(ctx, &commit).await?;
        let subtree_changes = changeset.subtree_changes().await?;
        let subtree_changes = subtree_changes
            .into_response_with(&(repo, params.identity_schemes))
            .await?;
        Ok(thrift::CommitSubtreeChangesResponse {
            subtree_changes,
            ..Default::default()
        })
    }

    /// Do a cross-repo lookup to see if a commit exists under a different hash in another repo
    pub(crate) async fn commit_lookup_xrepo(
        &self,
        ctx: CoreContext,
        commit: thrift::CommitSpecifier,
        params: thrift::CommitLookupXRepoParams,
    ) -> Result<thrift::CommitLookupResponse, scs_errors::ServiceError> {
        let repo = self.repo(ctx.clone(), &commit.repo).await?;
        let other_repo = self.repo(ctx, &params.other_repo).await?;
        let candidate_selection_hint = match params.candidate_selection_hint {
            Some(ref hint) => Some(CandidateSelectionHintArgs::from_request(hint)?),
            None => None,
        };

        let sync_behaviour = if params.no_ondemand_sync {
            XRepoLookupSyncBehaviour::NeverSync
        } else {
            XRepoLookupSyncBehaviour::SyncIfAbsent
        };
        let exact = if params.exact {
            XRepoLookupExactBehaviour::OnlyExactMapping
        } else {
            XRepoLookupExactBehaviour::WorkingCopyEquivalence
        };
        match repo
            .xrepo_commit_lookup(
                &other_repo,
                ChangesetSpecifier::from_request(&commit.id)?,
                candidate_selection_hint,
                sync_behaviour,
                exact,
            )
            .await?
        {
            Some(cs) => {
                let ids = map_commit_identity(&cs, &params.identity_schemes).await?;
                Ok(thrift::CommitLookupResponse {
                    exists: true,
                    ids: Some(ids),
                    ..Default::default()
                })
            }
            None => Ok(thrift::CommitLookupResponse {
                exists: false,
                ids: None,
                ..Default::default()
            }),
        }
    }
}
