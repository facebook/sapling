/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;
use std::convert::{TryFrom, TryInto};

use context::CoreContext;
use futures::compat::Stream01CompatExt;
use futures::stream::{self, StreamExt, TryStreamExt};
use futures::{future, try_join};
use mononoke_api::{unified_diff, ChangesetSpecifier, CopyInfo, MononokePath, UnifiedDiffMode};
use source_control as thrift;

use crate::commit_id::{map_commit_identities, map_commit_identity, CommitIdExt};
use crate::errors;
use crate::from_request::{check_range_and_convert, validate_timestamp, FromRequest};
use crate::history::collect_history;
use crate::into_response::{AsyncIntoResponse, IntoResponse};
use crate::source_control_impl::SourceControlServiceImpl;
use crate::specifiers::SpecifierExt;

// Magic number used when we want to limit concurrency with buffer_unordered.
const CONCURRENCY_LIMIT: usize = 100;

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
        let repo = self.repo(ctx, &commit.repo).await?;
        let changeset_specifier = ChangesetSpecifier::from_request(&commit.id)?;
        let other_changeset_specifier = ChangesetSpecifier::from_request(&params.other_commit_id)?;
        let (changeset, other_changeset_id) = try_join!(
            repo.changeset(changeset_specifier),
            repo.resolve_specifier(other_changeset_specifier),
        )?;
        let changeset = changeset.ok_or_else(|| errors::commit_not_found(commit.description()))?;
        let other_changeset_id = other_changeset_id.ok_or_else(|| {
            errors::commit_not_found(format!(
                "repo={} commit={}",
                commit.repo.name,
                params.other_commit_id.to_string()
            ))
        })?;
        let lca = changeset.common_base_with(other_changeset_id).await?;
        Ok(thrift::CommitLookupResponse {
            exists: lca.is_some(),
            ids: if let Some(lca) = lca {
                Some(map_commit_identity(&lca, &params.identity_schemes).await?)
            } else {
                None
            },
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
                })
            }
            None => Ok(thrift::CommitLookupResponse {
                exists: false,
                ids: None,
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
        let other_commit = thrift::CommitSpecifier {
            repo: commit.repo.clone(),
            id: params.other_commit_id.clone(),
        };
        let ((_repo1, base_commit), (_repo2, other_commit)) = try_join!(
            self.repo_changeset(ctx.clone(), &commit),
            self.repo_changeset(ctx.clone(), &other_commit,)
        )?;

        // Resolve the path into ChangesetPathContext
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
                        Some(path) => Some(base_commit.path(&path)?),
                        None => None,
                    },
                    match path_pair.other_path {
                        Some(path) => Some(other_commit.path(&path)?),
                        None => None,
                    },
                    CopyInfo::from_request(&path_pair.copy_info)?,
                    mode,
                ))
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
                    unified_diff(&other_path, &base_path, copy_info, context_lines, mode).await?;
                let r: Result<_, errors::ServiceError> =
                    Ok(thrift::CommitFileDiffsResponseElement {
                        base_path: base_path.map(|p| p.path().to_string()),
                        other_path: other_path.map(|p| p.path().to_string()),
                        diff: diff.into_response(),
                    });
                r
            },
        ))
        .await?;
        Ok(thrift::CommitFileDiffsResponse { path_diffs })
    }

    /// Get commit info.
    pub(crate) async fn commit_info(
        &self,
        ctx: CoreContext,
        commit: thrift::CommitSpecifier,
        params: thrift::CommitInfoParams,
    ) -> Result<thrift::CommitInfo, errors::ServiceError> {
        let (_repo, changeset) = self.repo_changeset(ctx, &commit).await?;
        (changeset, &params.identity_schemes).into_response().await
    }

    /// Returns `true` if this commit is an ancestor of `other_commit`.
    pub(crate) async fn commit_is_ancestor_of(
        &self,
        ctx: CoreContext,
        commit: thrift::CommitSpecifier,
        params: thrift::CommitIsAncestorOfParams,
    ) -> Result<bool, errors::ServiceError> {
        let repo = self.repo(ctx, &commit.repo).await?;
        let changeset_specifier = ChangesetSpecifier::from_request(&commit.id)?;
        let other_changeset_specifier = ChangesetSpecifier::from_request(&params.other_commit_id)?;
        let (changeset, other_changeset_id) = try_join!(
            repo.changeset(changeset_specifier),
            repo.resolve_specifier(other_changeset_specifier),
        )?;
        let changeset = changeset.ok_or_else(|| errors::commit_not_found(commit.description()))?;
        let other_changeset_id = other_changeset_id.ok_or_else(|| {
            errors::commit_not_found(format!(
                "repo={} commit={}",
                commit.repo.name,
                params.other_commit_id.to_string()
            ))
        })?;
        let is_ancestor_of = changeset.is_ancestor_of(other_changeset_id).await?;
        Ok(is_ancestor_of)
    }

    // Diff two commits
    pub(crate) async fn commit_compare(
        &self,
        ctx: CoreContext,
        commit: thrift::CommitSpecifier,
        params: thrift::CommitCompareParams,
    ) -> Result<thrift::CommitCompareResponse, errors::ServiceError> {
        let (repo, base_changeset) = self.repo_changeset(ctx, &commit).await?;

        let other_changeset_id = match &params.other_commit_id {
            Some(id) => {
                let specifier = ChangesetSpecifier::from_request(id)?;
                repo.resolve_specifier(specifier).await?.ok_or_else(|| {
                    errors::commit_not_found(format!(
                        "repo={} commit={}",
                        commit.repo.name,
                        id.to_string()
                    ))
                })?
            }
            None => base_changeset
                .parents()
                .await?
                .into_iter()
                .next()
                .ok_or_else(|| {
                    // TODO: compare with empty manifest in this case
                    errors::commit_not_found(format!(
                        "parent commit not found: {}",
                        commit.description()
                    ))
                })?,
        };
        let paths: Option<Vec<MononokePath>> = match params.paths {
            None => None,
            Some(paths) => Some(
                paths
                    .iter()
                    .map(|path| path.try_into())
                    .collect::<Result<Vec<_>, _>>()?,
            ),
        };
        let diff = base_changeset
            .diff(other_changeset_id, !params.skip_copies_renames, paths)
            .await?;
        let diff_files = stream::iter(diff)
            .map(|d| d.into_response())
            .buffer_unordered(CONCURRENCY_LIMIT)
            .try_collect()
            .await?;

        let other_changeset = repo
            .changeset(ChangesetSpecifier::Bonsai(other_changeset_id))
            .await?
            .ok_or_else(|| errors::internal_error("other changeset is missing"))?;
        let other_commit_ids =
            map_commit_identity(&other_changeset, &params.identity_schemes).await?;
        Ok(thrift::CommitCompareResponse {
            diff_files,
            other_commit_ids,
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

        let files: Vec<_> = changeset
            .find_files(prefixes, params.basenames)
            .await?
            .take(limit)
            .map_ok(|path| path.to_string())
            .try_collect()
            .await?;
        Ok(thrift::CommitFindFilesResponse { files })
    }

    /// Returns the history of a commit
    pub(crate) async fn commit_history(
        &self,
        ctx: CoreContext,
        commit: thrift::CommitSpecifier,
        params: thrift::CommitHistoryParams,
    ) -> Result<thrift::CommitHistoryResponse, errors::ServiceError> {
        let (repo, changeset) = self.repo_changeset(ctx, &commit).await?;
        let descendants_of = if let Some(descendants_of) = params.descendants_of {
            Some(self.changeset_id(&repo, &descendants_of).await?)
        } else {
            None
        };

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

        let history_stream = changeset.history(after_timestamp, descendants_of).await;
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

        Ok(thrift::CommitHistoryResponse { history })
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
            )?
            .compat()
            .try_collect::<Vec<_>>()
            .await?;
        let continue_after = match limit {
            Some(limit) if bookmarks.len() as u64 >= limit => {
                bookmarks.last().map(|bookmark| bookmark.0.clone())
            }
            _ => None,
        };
        let ids = bookmarks.iter().map(|(_name, cs_id)| *cs_id).collect();
        let id_mapping = map_commit_identities(&repo, ids, &params.identity_schemes).await?;
        let bookmarks = stream::iter(bookmarks)
            .map({
                let changeset = &changeset;
                move |(name, cs_id)| async move {
                    Ok::<_, errors::ServiceError>(
                        changeset
                            .is_ancestor_of(cs_id)
                            .await?
                            .then_some((name, cs_id)),
                    )
                }
            })
            .buffered(100)
            .try_filter_map({
                let id_mapping = &id_mapping;
                move |bookmark| async move {
                    Ok(bookmark.map(|(name, cs_id)| match id_mapping.get(&cs_id) {
                        Some(ids) => (name, ids.clone()),
                        None => (name, BTreeMap::new()),
                    }))
                }
            })
            .try_collect()
            .await?;
        Ok(thrift::CommitListDescendantBookmarksResponse {
            bookmarks,
            continue_after,
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
        match repo
            .xrepo_commit_lookup(&other_repo, ChangesetSpecifier::from_request(&commit.id)?)
            .await?
        {
            Some(cs) => {
                let ids = map_commit_identity(&cs, &params.identity_schemes).await?;
                Ok(thrift::CommitLookupResponse {
                    exists: true,
                    ids: Some(ids),
                })
            }
            None => Ok(thrift::CommitLookupResponse {
                exists: false,
                ids: None,
            }),
        }
    }
}
