/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use std::collections::{BTreeMap, BTreeSet};
use std::convert::{TryFrom, TryInto};

use futures_util::{future, stream, try_join, StreamExt, TryStreamExt};
use mononoke_api::{
    unified_diff, ChangesetContext, ChangesetSpecifier, CopyInfo, MononokeError, MononokePath,
    RepoContext,
};
use source_control as thrift;
use source_control::services::source_control_service as service;
use srserver::RequestContext;

use crate::commit_id::{map_commit_identities, map_commit_identity, CommitIdExt};
use crate::errors;
use crate::from_request::{check_range_and_convert, FromRequest};
use crate::into_response::{AsyncIntoResponse, IntoResponse};
use crate::source_control_impl::SourceControlServiceImpl;
use crate::specifiers::SpecifierExt;

// Magic number used when we want to limit concurrency with buffer_unordered.
const CONCURRENCY_LIMIT: usize = 100;

impl SourceControlServiceImpl {
    /// Look up commit.
    pub(crate) async fn commit_lookup(
        &self,
        req_ctxt: &RequestContext,
        commit: thrift::CommitSpecifier,
        params: thrift::CommitLookupParams,
    ) -> Result<thrift::CommitLookupResponse, service::CommitLookupExn> {
        let ctx = self.create_ctx(req_ctxt, Some(&commit))?;
        let repo = self.repo(ctx, &commit.repo)?;
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
        req_ctxt: &RequestContext,
        commit: thrift::CommitSpecifier,
        params: thrift::CommitFileDiffsParams,
    ) -> Result<thrift::CommitFileDiffsResponse, service::CommitFileDiffsExn> {
        let ctx = self.create_ctx(req_ctxt, Some(&commit))?;
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
                ))
            })
            .collect::<Result<Vec<_>, errors::ServiceError>>()?;

        // Check the total file size limit
        let flat_paths = paths
            .iter()
            .flat_map(|(base_path, other_path, _)| vec![base_path, other_path])
            .filter_map(|x| x.as_ref());
        let total_input_size: u64 = future::try_join_all(flat_paths.map(|path| {
            async move {
                let r: Result<_, errors::ServiceError> = if let Some(file) = path.file().await? {
                    Ok(file.metadata().await?.total_size)
                } else {
                    Ok(0)
                };
                r
            }
        }))
        .await?
        .into_iter()
        .sum();

        if total_input_size as i64 > thrift::consts::COMMIT_FILE_DIFFS_SIZE_LIMIT {
            Err(errors::diff_input_too_big(total_input_size))?;
        }

        let path_diffs =
            future::try_join_all(paths.into_iter().map(|(base_path, other_path, copy_info)| {
                async move {
                    let diff =
                        unified_diff(&other_path, &base_path, copy_info, context_lines).await?;
                    let r: Result<_, errors::ServiceError> =
                        Ok(thrift::CommitFileDiffsResponseElement {
                            base_path: base_path.map(|p| p.path().to_string()),
                            other_path: other_path.map(|p| p.path().to_string()),
                            diff: diff.into_response(),
                        });
                    r
                }
            }))
            .await?;
        Ok(thrift::CommitFileDiffsResponse { path_diffs })
    }

    /// Get commit info.
    pub(crate) async fn commit_info(
        &self,
        req_ctxt: &RequestContext,
        commit: thrift::CommitSpecifier,
        params: thrift::CommitInfoParams,
    ) -> Result<thrift::CommitInfo, service::CommitInfoExn> {
        let ctx = self.create_ctx(req_ctxt, Some(&commit))?;
        let (repo, changeset) = self.repo_changeset(ctx, &commit).await?;

        async fn map_parent_identities(
            repo: &RepoContext,
            changeset: &ChangesetContext,
            identity_schemes: &BTreeSet<thrift::CommitIdentityScheme>,
        ) -> Result<Vec<BTreeMap<thrift::CommitIdentityScheme, thrift::CommitId>>, MononokeError>
        {
            let parents = changeset.parents().await?;
            let parent_id_mapping =
                map_commit_identities(&repo, parents.clone(), identity_schemes).await?;
            Ok(parents
                .iter()
                .map(|parent_id| {
                    parent_id_mapping
                        .get(parent_id)
                        .map(Clone::clone)
                        .unwrap_or_else(BTreeMap::new)
                })
                .collect())
        }

        let (ids, message, date, author, parents, extra) = try_join!(
            map_commit_identity(&changeset, &params.identity_schemes),
            changeset.message(),
            changeset.author_date(),
            changeset.author(),
            map_parent_identities(&repo, &changeset, &params.identity_schemes),
            changeset.extras(),
        )?;
        Ok(thrift::CommitInfo {
            ids,
            message,
            date: date.timestamp(),
            tz: date.offset().local_minus_utc(),
            author,
            parents,
            extra: extra.into_iter().collect(),
        })
    }

    /// Returns `true` if this commit is an ancestor of `other_commit`.
    pub(crate) async fn commit_is_ancestor_of(
        &self,
        req_ctxt: &RequestContext,
        commit: thrift::CommitSpecifier,
        params: thrift::CommitIsAncestorOfParams,
    ) -> Result<bool, service::CommitIsAncestorOfExn> {
        let ctx = self.create_ctx(req_ctxt, Some(&commit))?;
        let repo = self.repo(ctx, &commit.repo)?;
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
        req_ctxt: &RequestContext,
        commit: thrift::CommitSpecifier,
        params: thrift::CommitCompareParams,
    ) -> Result<thrift::CommitCompareResponse, service::CommitCompareExn> {
        let ctx = self.create_ctx(req_ctxt, Some(&commit))?;
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
        req_ctxt: &RequestContext,
        commit: thrift::CommitSpecifier,
        params: thrift::CommitFindFilesParams,
    ) -> Result<thrift::CommitFindFilesResponse, service::CommitFindFilesExn> {
        let ctx = self.create_ctx(req_ctxt, Some(&commit))?;
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

    /// Do a cross-repo lookup to see if a commit exists under a different hash in another repo
    pub(crate) async fn commit_lookup_xrepo(
        &self,
        req_ctxt: &RequestContext,
        commit: thrift::CommitSpecifier,
        params: thrift::CommitLookupXRepoParams,
    ) -> Result<thrift::CommitLookupResponse, service::CommitLookupXrepoExn> {
        let ctx = self.create_ctx(req_ctxt, Some(&commit))?;
        let repo = self.repo(ctx.clone(), &commit.repo)?;
        let other_repo = self.repo(ctx, &params.other_repo)?;
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
