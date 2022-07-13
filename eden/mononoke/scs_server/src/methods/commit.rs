/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeSet;
use std::collections::HashMap;
use std::sync::Arc;

use bytes::Bytes;
use context::CoreContext;
use futures::future;
use futures::stream;
use futures::stream::FuturesOrdered;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use futures::try_join;
use hooks::HookExecution;
use hooks::HookOutcome;
use itertools::Either;
use itertools::Itertools;
use maplit::btreeset;
use mononoke_api::unified_diff;
use mononoke_api::CandidateSelectionHintArgs;
use mononoke_api::ChangesetContext;
use mononoke_api::ChangesetDiffItem;
use mononoke_api::ChangesetFileOrdering;
use mononoke_api::ChangesetHistoryOptions;
use mononoke_api::ChangesetId;
use mononoke_api::ChangesetPathDiffContext;
use mononoke_api::ChangesetSpecifier;
use mononoke_api::CopyInfo;
use mononoke_api::MononokeError;
use mononoke_api::MononokePath;
use mononoke_api::RepoContext;
use mononoke_api::UnifiedDiffMode;
use source_control as thrift;

use crate::commit_id::map_commit_identities;
use crate::commit_id::map_commit_identity;
use crate::errors;
use crate::errors::ServiceErrorResultExt;
use crate::from_request::check_range_and_convert;
use crate::from_request::validate_timestamp;
use crate::from_request::FromRequest;
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
    fn path(&self) -> Result<&str, errors::ServiceError> {
        // Use the base path where available.  If it is not available, then
        // this is a deletion and the other path should be used.
        match self {
            CommitComparePath::File(file) => file
                .base_file
                .as_ref()
                .or(file.other_file.as_ref())
                .map(|file| file.path.as_str())
                .ok_or_else(|| {
                    errors::internal_error("programming error, file entry has no file").into()
                }),

            CommitComparePath::Tree(tree) => tree
                .base_tree
                .as_ref()
                .or(tree.other_tree.as_ref())
                .map(|tree| tree.path.as_str())
                .ok_or_else(|| {
                    errors::internal_error("programming error, tree entry has no tree").into()
                }),
        }
    }
}

// helper used by commit_compare
async fn into_compare_path(
    path_diff: ChangesetPathDiffContext,
) -> Result<CommitComparePath, errors::ServiceError> {
    let mut file: Option<(
        Option<thrift::FilePathInfo>,
        Option<thrift::FilePathInfo>,
        thrift::CopyInfo,
    )> = None;
    let mut tree: Option<(Option<thrift::TreePathInfo>, Option<thrift::TreePathInfo>)> = None;
    match path_diff {
        ChangesetPathDiffContext::Added(base_context) => {
            if base_context.is_file().await? {
                let entry = base_context.into_response().await?;
                file = Some((None, entry, thrift::CopyInfo::NONE));
            } else {
                let entry = base_context.into_response().await?;
                tree = Some((None, entry));
            }
        }
        ChangesetPathDiffContext::Removed(other_context) => {
            if other_context.is_file().await? {
                let entry = other_context.into_response().await?;
                file = Some((entry, None, thrift::CopyInfo::NONE));
            } else {
                let entry = other_context.into_response().await?;
                tree = Some((entry, None));
            }
        }
        ChangesetPathDiffContext::Changed(base_context, other_context) => {
            if other_context.is_file().await? {
                let (other_entry, base_entry) =
                    try_join!(other_context.into_response(), base_context.into_response(),)?;
                file = Some((other_entry, base_entry, thrift::CopyInfo::NONE));
            } else {
                let (other_entry, base_entry) =
                    try_join!(other_context.into_response(), base_context.into_response(),)?;
                tree = Some((other_entry, base_entry));
            }
        }
        ChangesetPathDiffContext::Copied(base_context, other_context) => {
            if other_context.is_file().await? {
                let (other_entry, base_entry) =
                    try_join!(other_context.into_response(), base_context.into_response(),)?;
                file = Some((other_entry, base_entry, thrift::CopyInfo::COPY));
            } else {
                let (other_entry, base_entry) =
                    try_join!(other_context.into_response(), base_context.into_response(),)?;
                tree = Some((other_entry, base_entry));
            }
        }
        ChangesetPathDiffContext::Moved(base_context, other_context) => {
            if other_context.is_file().await? {
                let (other_entry, base_entry) =
                    try_join!(other_context.into_response(), base_context.into_response(),)?;
                file = Some((other_entry, base_entry, thrift::CopyInfo::MOVE));
            } else {
                let (other_entry, base_entry) =
                    try_join!(other_context.into_response(), base_context.into_response(),)?;
                tree = Some((other_entry, base_entry));
            }
        }
    };
    if let Some((other_file, base_file, copy_info)) = file {
        return Ok(CommitComparePath::File(thrift::CommitCompareFile {
            base_file,
            other_file,
            copy_info,
            ..Default::default()
        }));
    }
    if let Some((other_tree, base_tree)) = tree {
        return Ok(CommitComparePath::Tree(thrift::CommitCompareTree {
            base_tree,
            other_tree,
            ..Default::default()
        }));
    }
    Err(errors::internal_error("programming error, diff is neither tree nor file").into())
}

/// Helper for commit_compare to add mutable rename information if appropriate
async fn add_mutable_renames(
    base_changeset: &mut ChangesetContext,
    params: &thrift::CommitCompareParams,
) -> Result<(), errors::ServiceError> {
    if params.follow_mutable_file_history.unwrap_or(false) {
        if let Some(paths) = &params.paths {
            let paths: Vec<_> = paths
                .iter()
                .map(MononokePath::try_from)
                .collect::<Result<_, MononokeError>>()?;
            base_changeset
                .add_mutable_renames(paths.into_iter())
                .await?;
        }
    }
    Ok(())
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
    ) -> Result<thrift::CommitLookupResponse, errors::ServiceError> {
        let (_repo, changeset, other_changeset) = self
            .repo_changeset_pair(ctx, &commit, &params.other_commit_id)
            .await?;
        let lca = changeset.common_base_with(other_changeset.id()).await?;
        Ok(thrift::CommitLookupResponse {
            exists: lca.is_some(),
            ids: if let Some(lca) = lca {
                Some(map_commit_identity(&lca, &params.identity_schemes).await?)
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
    ) -> Result<thrift::CommitLookupResponse, errors::ServiceError> {
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
    ) -> Result<thrift::CommitFileDiffsResponse, errors::ServiceError> {
        let context_lines = params.context as usize;

        // Check the path count limit
        if params.paths.len() as i64 > thrift::consts::COMMIT_FILE_DIFFS_PATH_COUNT_LIMIT {
            Err(errors::diff_input_too_many_paths(params.paths.len()))?;
        }

        // Resolve the CommitSpecfier into ChangesetContext
        let (base_commit, other_commit) = match params.other_commit_id {
            Some(other_commit_id) => {
                let (_repo, base_commit, other_commit) = self
                    .repo_changeset_pair(ctx, &commit, &other_commit_id)
                    .await?;
                (base_commit, Some(other_commit))
            }
            None => {
                let (_repo, base_commit) = self.repo_changeset(ctx, &commit).await?;
                (base_commit, None)
            }
        };

        // Resolve the path into ChangesetPathContentContext
        // To make it more efficient we do a batch request
        // to resolve all paths into path contexts
        let mut base_commit_paths = vec![];
        let mut other_commit_paths = vec![];
        let paths = params
            .paths
            .into_iter()
            .map(|path_pair| {
                let mode = if path_pair.generate_placeholder_diff.unwrap_or(false) {
                    UnifiedDiffMode::OmitContent
                } else {
                    UnifiedDiffMode::Inline
                };
                Ok((
                    match path_pair.base_path {
                        Some(path) => {
                            let mpath = MononokePath::try_from(&path)
                                .context("invalid base commit path")?;
                            base_commit_paths.push(mpath.clone());
                            Some(mpath)
                        }
                        None => None,
                    },
                    match &other_commit {
                        Some(_other_commit) => match path_pair.other_path {
                            Some(path) => {
                                let mpath = MononokePath::try_from(&path)
                                    .context("invalid other commit path")?;
                                other_commit_paths.push(mpath.clone());
                                Some(mpath)
                            }
                            None => None,
                        },
                        None => None,
                    },
                    CopyInfo::from_request(&path_pair.copy_info)?,
                    mode,
                ))
            })
            .collect::<Result<Vec<_>, errors::ServiceError>>()?;

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

        let paths = paths
            .into_iter()
            .map(|(base_path, other_path, copy_info, mode)| {
                let base_path = match base_path {
                    Some(base_path) => {
                        Some(base_commit_contexts.get(&base_path).ok_or_else(|| {
                            errors::invalid_request(format!(
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
                                errors::invalid_request(format!(
                                    "{} not found in {:?}",
                                    other_path, other_commit
                                ))
                            })?)
                        }
                        None => None,
                    },
                    None => None,
                };

                Ok((base_path, other_path, copy_info, mode))
            })
            .collect::<Result<Vec<_>, errors::ServiceError>>()?;

        // Check the total file size limit
        let flat_paths = paths
            .iter()
            .filter_map(|(base_path, other_path, _, mode)| match mode {
                UnifiedDiffMode::OmitContent => None,
                UnifiedDiffMode::Inline => Some((base_path, other_path)),
            })
            .flat_map(|(base_path, other_path)| vec![base_path, other_path])
            .filter_map(|x| x.as_ref());
        let total_input_size: u64 = future::try_join_all(flat_paths.map(|path| async move {
            let r: Result<_, errors::ServiceError> = if let Some(file) = path.file().await? {
                Ok(file.metadata().await?.total_size)
            } else {
                Ok(0)
            };
            r
        }))
        .await?
        .into_iter()
        .sum();

        if total_input_size as i64 > thrift::consts::COMMIT_FILE_DIFFS_SIZE_LIMIT {
            Err(errors::diff_input_too_big(total_input_size))?;
        }

        let path_diffs = future::try_join_all(paths.into_iter().map(
            |(base_path, other_path, copy_info, mode)| async move {
                let diff =
                    unified_diff(other_path, base_path, copy_info, context_lines, mode).await?;
                let r: Result<_, errors::ServiceError> =
                    Ok(thrift::CommitFileDiffsResponseElement {
                        base_path: base_path.map(|p| p.path().to_string()),
                        other_path: other_path.map(|p| p.path().to_string()),
                        diff: diff.into_response(),
                        ..Default::default()
                    });
                r
            },
        ))
        .await?;
        Ok(thrift::CommitFileDiffsResponse {
            path_diffs,
            ..Default::default()
        })
    }

    /// Get commit info.
    pub(crate) async fn commit_info(
        &self,
        ctx: CoreContext,
        commit: thrift::CommitSpecifier,
        params: thrift::CommitInfoParams,
    ) -> Result<thrift::CommitInfo, errors::ServiceError> {
        let (_repo, changeset) = self.repo_changeset(ctx, &commit).await?;
        changeset.into_response_with(&params.identity_schemes).await
    }

    /// Returns `true` if this commit is an ancestor of `other_commit`.
    pub(crate) async fn commit_is_ancestor_of(
        &self,
        ctx: CoreContext,
        commit: thrift::CommitSpecifier,
        params: thrift::CommitIsAncestorOfParams,
    ) -> Result<bool, errors::ServiceError> {
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
        repo: &RepoContext,
        base_changeset: &mut ChangesetContext,
        params: &thrift::CommitCompareParams,
    ) -> Result<Option<ChangesetContext>, errors::ServiceError> {
        let commit_parents = base_changeset.parents().await?;
        let mut other_changeset_id = commit_parents.get(0).copied();

        if params.follow_mutable_file_history.unwrap_or(false) {
            let mutable_parents = base_changeset.mutable_parents();

            // If there are multiple choices to make, then bail - the user needs to be
            // clear to avoid the ambiguity
            if mutable_parents.len() > 1 {
                return Err(errors::invalid_request(
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
                    .ok_or_else(|| errors::internal_error("other changeset is missing"))?;
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
    ) -> Result<thrift::CommitCompareResponse, errors::ServiceError> {
        let (base_changeset, other_changeset) = match &params.other_commit_id {
            Some(id) => {
                let (_repo, mut base_changeset, other_changeset) =
                    self.repo_changeset_pair(ctx, &commit, id).await?;
                add_mutable_renames(&mut base_changeset, &params).await?;
                (base_changeset, Some(other_changeset))
            }
            None => {
                let (repo, mut base_changeset) = self.repo_changeset(ctx, &commit).await?;
                add_mutable_renames(&mut base_changeset, &params).await?;
                let other_changeset = self
                    .find_commit_compare_parent(&repo, &mut base_changeset, &params)
                    .await?;
                (base_changeset, other_changeset)
            }
        };

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

        let paths: Option<Vec<MononokePath>> = match params.paths {
            None => None,
            Some(paths) => Some(
                paths
                    .iter()
                    .map(|path| path.try_into())
                    .collect::<Result<Vec<_>, _>>()?,
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
                            .await?
                    }
                    None => {
                        base_changeset
                            .diff_root_unordered(paths, diff_items)
                            .await?
                    }
                };
                stream::iter(diff)
                    .map(into_compare_path)
                    .buffer_unordered(CONCURRENCY_LIMIT)
                    .try_collect::<Vec<_>>()
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
                        MononokePath::try_from(&after).map_err(|e| {
                            errors::invalid_request(format!(
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
                            .await?
                    }
                };
                let diff_items = diff
                    .into_iter()
                    .map(into_compare_path)
                    .collect::<FuturesOrdered<_>>()
                    .try_collect::<Vec<_>>()
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
            Some(other_changeset) => {
                Some(map_commit_identity(&other_changeset, &params.identity_schemes).await?)
            }
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
    ) -> Result<thrift::CommitFindFilesResponse, errors::ServiceError> {
        let (_repo, changeset) = self.repo_changeset(ctx, &commit).await?;
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
                        MononokePath::try_from(&prefix).map_err(|e| {
                            errors::invalid_request(format!("invalid prefix '{}': {}", prefix, e))
                        })
                    })
                    .collect::<Result<Vec<_>, _>>()?,
            ),
            None => None,
        };
        let ordering = match &params.after {
            Some(after) => {
                let after = Some(MononokePath::try_from(after).map_err(|e| {
                    errors::invalid_request(format!("invalid continuation path '{}': {}", after, e))
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

    /// Returns the history of a commit
    pub(crate) async fn commit_history(
        &self,
        ctx: CoreContext,
        commit: thrift::CommitSpecifier,
        params: thrift::CommitHistoryParams,
    ) -> Result<thrift::CommitHistoryResponse, errors::ServiceError> {
        let (repo, changeset) = self.repo_changeset(ctx, &commit).await?;
        let (descendants_of, exclude_changeset_and_ancestors) = try_join!(
            async {
                if let Some(descendants_of) = &params.descendants_of {
                    Ok::<_, errors::ServiceError>(Some(
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
                    Ok::<_, errors::ServiceError>(Some(
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
                return Err(errors::invalid_request(format!(
                    "after_timestamp ({}) cannot be greater than before_timestamp ({})",
                    ats, bts,
                ))
                .into());
            }
        }

        if skip > 0 && (after_timestamp.is_some() || before_timestamp.is_some()) {
            return Err(errors::invalid_request(
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
            .await;
        let history = collect_history(
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

    pub(crate) async fn commit_list_descendant_bookmarks(
        &self,
        ctx: CoreContext,
        commit: thrift::CommitSpecifier,
        params: thrift::CommitListDescendantBookmarksParams,
    ) -> Result<thrift::CommitListDescendantBookmarksResponse, errors::ServiceError> {
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
            changeset: Arc<ChangesetContext>,
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
                        tokio::task::spawn(filter_descendant(changeset, bookmark))
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
    ) -> Result<thrift::CommitRunHooksResponse, errors::ServiceError> {
        let (_repo, changeset) = self.repo_changeset(ctx, &commit).await?;
        let pushvars: Option<HashMap<String, Bytes>> = params
            .pushvars
            .map(|p| p.into_iter().map(|(k, v)| (k, Bytes::from(v))).collect());
        let outcomes = changeset
            .run_hooks(params.bookmark, pushvars.as_ref())
            .await?;
        Ok(thrift::CommitRunHooksResponse {
            outcomes: outcomes
                .into_iter()
                .map(|outcome| {
                    let (name, execution) = match outcome {
                        HookOutcome::FileHook(id, exec) => (id.hook_name, exec),
                        HookOutcome::ChangesetHook(id, exec) => (id.hook_name, exec),
                    };
                    let thrift_outcome = match execution {
                        HookExecution::Accepted => {
                            thrift::HookOutcome::accepted(thrift::HookOutcomeAccepted {
                                ..Default::default()
                            })
                        }
                        HookExecution::Rejected(rej) => {
                            thrift::HookOutcome::rejected(thrift::HookOutcomeRejected {
                                description: rej.description.to_string(),
                                long_description: rej.long_description,
                                ..Default::default()
                            })
                        }
                    };
                    (name, thrift_outcome)
                })
                .collect(),
            ..Default::default()
        })
    }

    /// Do a cross-repo lookup to see if a commit exists under a different hash in another repo
    pub(crate) async fn commit_lookup_xrepo(
        &self,
        ctx: CoreContext,
        commit: thrift::CommitSpecifier,
        params: thrift::CommitLookupXRepoParams,
    ) -> Result<thrift::CommitLookupResponse, errors::ServiceError> {
        let repo = self.repo(ctx.clone(), &commit.repo).await?;
        let other_repo = self.repo(ctx, &params.other_repo).await?;
        let candidate_selection_hint = match params.candidate_selection_hint {
            Some(ref hint) => Some(CandidateSelectionHintArgs::from_request(hint)?),
            None => None,
        };

        match repo
            .xrepo_commit_lookup(
                &other_repo,
                ChangesetSpecifier::from_request(&commit.id)?,
                candidate_selection_hint,
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
