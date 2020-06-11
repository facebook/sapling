/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;
use std::convert::{identity, TryFrom};

use bytes::Bytes;
use chrono::{DateTime, FixedOffset, Local};
use context::CoreContext;
use futures::{compat::Future01CompatExt, future::try_join, future::try_join_all};
use futures_old::stream::Stream;
use futures_util::stream::FuturesOrdered;
use futures_util::TryStreamExt;
use maplit::btreemap;
use mononoke_api::{
    ChangesetPrefixSpecifier, ChangesetSpecifier, ChangesetSpecifierPrefixResolution, CreateChange,
    CreateCopyInfo, FileId, FileType, MononokePath,
};
use mononoke_types::hash::{Sha1, Sha256};
use source_control as thrift;

use crate::commit_id::{map_commit_identities, map_commit_identity, CommitIdExt};
use crate::errors;
use crate::from_request::{check_range_and_convert, FromRequest};
use crate::into_response::AsyncIntoResponse;
use crate::source_control_impl::SourceControlServiceImpl;

impl SourceControlServiceImpl {
    /// Resolve a bookmark to a changeset.
    ///
    /// Returns whether the bookmark exists, and the IDs of the changeset in
    /// the requested indentity schemes.
    pub(crate) async fn repo_resolve_bookmark(
        &self,
        ctx: CoreContext,
        repo: thrift::RepoSpecifier,
        params: thrift::RepoResolveBookmarkParams,
    ) -> Result<thrift::RepoResolveBookmarkResponse, errors::ServiceError> {
        let repo = self.repo(ctx, &repo).await?;
        match repo.resolve_bookmark(params.bookmark_name).await? {
            Some(cs) => {
                let ids = map_commit_identity(&cs, &params.identity_schemes).await?;
                Ok(thrift::RepoResolveBookmarkResponse {
                    exists: true,
                    ids: Some(ids),
                })
            }
            None => Ok(thrift::RepoResolveBookmarkResponse {
                exists: false,
                ids: None,
            }),
        }
    }

    /// Resolve a prefix and its identity scheme to a changeset.
    ///
    /// Returns the IDs of the changeset in the requested identity schemes.
    /// Suggestions for ambiguous prefixes are not provided for now.
    pub(crate) async fn repo_resolve_commit_prefix(
        &self,
        ctx: CoreContext,
        repo: thrift::RepoSpecifier,
        params: thrift::RepoResolveCommitPrefixParams,
    ) -> Result<thrift::RepoResolveCommitPrefixResponse, errors::ServiceError> {
        use ChangesetSpecifierPrefixResolution::*;
        type Response = thrift::RepoResolveCommitPrefixResponse;
        type ResponseType = thrift::RepoResolveCommitPrefixResponseType;

        let same_request_response_schemes = params.identity_schemes.len() == 1
            && params.identity_schemes.contains(&params.prefix_scheme);

        let prefix = ChangesetPrefixSpecifier::from_request(&params)?;
        let repo = self.repo(ctx, &repo).await?;

        // If the response requires exactly the same identity scheme as in the request,
        // the general case works but we don't need to pay extra overhead to resolve
        // ChangesetSpecifier to a changeset.

        match repo.resolve_changeset_id_prefix(prefix).await? {
            Single(ChangesetSpecifier::Bonsai(cs_id)) if same_request_response_schemes => {
                Ok(Response {
                    ids: Some(btreemap! {
                        params.prefix_scheme => thrift::CommitId::bonsai(cs_id.as_ref().into())
                    }),
                    resolved_type: ResponseType::RESOLVED,
                })
            }
            Single(ChangesetSpecifier::Hg(cs_id)) if same_request_response_schemes => {
                Ok(Response {
                    ids: Some(btreemap! {
                        params.prefix_scheme => thrift::CommitId::hg(cs_id.as_ref().into())
                    }),
                    resolved_type: ResponseType::RESOLVED,
                })
            }
            Single(cs_id) => match &repo.changeset(cs_id).await? {
                None => Err(errors::internal_error(
                    "unexpected failure to resolve an existing commit",
                )
                .into()),
                Some(cs) => Ok(Response {
                    ids: Some(map_commit_identity(&cs, &params.identity_schemes).await?),
                    resolved_type: ResponseType::RESOLVED,
                }),
            },
            NoMatch => Ok(Response {
                resolved_type: ResponseType::NOT_FOUND,
                ..Default::default()
            }),
            _ => Ok(Response {
                resolved_type: ResponseType::AMBIGUOUS,
                ..Default::default()
            }),
        }
    }

    /// List bookmarks.
    pub(crate) async fn repo_list_bookmarks(
        &self,
        ctx: CoreContext,
        repo: thrift::RepoSpecifier,
        params: thrift::RepoListBookmarksParams,
    ) -> Result<thrift::RepoListBookmarksResponse, errors::ServiceError> {
        let limit = match check_range_and_convert(
            "limit",
            params.limit,
            0..=source_control::REPO_LIST_BOOKMARKS_MAX_LIMIT,
        )? {
            0 => None,
            limit => Some(limit),
        };
        let prefix = if !params.bookmark_prefix.is_empty() {
            Some(params.bookmark_prefix)
        } else {
            None
        };
        let repo = self.repo(ctx, &repo).await?;
        let bookmarks = repo
            .list_bookmarks(params.include_scratch, prefix, limit)
            .collect()
            .compat()
            .await?;
        let ids = bookmarks.iter().map(|(_name, cs_id)| *cs_id).collect();
        let id_mapping = map_commit_identities(&repo, ids, &params.identity_schemes).await?;
        let bookmarks = bookmarks
            .into_iter()
            .map(|(name, cs_id)| match id_mapping.get(&cs_id) {
                Some(ids) => (name, ids.clone()),
                None => (name, BTreeMap::new()),
            })
            .collect();
        Ok(thrift::RepoListBookmarksResponse { bookmarks })
    }

    /// Create a new commit.
    pub(crate) async fn repo_create_commit(
        &self,
        ctx: CoreContext,
        repo: thrift::RepoSpecifier,
        params: thrift::RepoCreateCommitParams,
    ) -> Result<thrift::RepoCreateCommitResponse, errors::ServiceError> {
        let repo = self.repo(ctx, &repo).await?.write().await?;

        let parents: Vec<_> = params
            .parents
            .into_iter()
            .map(|parent| {
                let repo = &repo;
                async move {
                    let changeset_specifier = ChangesetSpecifier::from_request(&parent)?;
                    let changeset = repo
                        .changeset(changeset_specifier)
                        .await?
                        .ok_or_else(|| errors::commit_not_found(parent.to_string()))?;
                    Ok::<_, errors::ServiceError>(changeset.id())
                }
            })
            .collect::<FuturesOrdered<_>>()
            .try_collect()
            .await?;

        if parents.len() != 1 {
            return Err(errors::invalid_request(
                "repo_create_commit can only create commits with a single parent",
            )
            .into());
        }

        // Convert changes to actions
        let file_changes = params
            .changes
            .into_iter()
            .map(|(path, change)| {
                let repo = &repo;
                async move {
                    let path = MononokePath::try_from(&path).map_err(|e| {
                        errors::invalid_request(format!("invalid path '{}': {}", path, e))
                    })?;
                    let change = match change {
                        thrift::RepoCreateCommitParamsChange::changed(c) => {
                            let file_type = FileType::from_request(&c.type_)?;
                            let copy_info = c
                                .copy_info
                                .as_ref()
                                .map(CreateCopyInfo::from_request)
                                .transpose()?;
                            match c.content {
                                thrift::RepoCreateCommitParamsFileContent::id(id) => {
                                    let file_id = FileId::from_request(&id)?;
                                    let file = repo.file(file_id).await?.ok_or_else(|| {
                                        errors::file_not_found(file_id.to_string())
                                    })?;
                                    CreateChange::ExistingContent(
                                        file.id().await?,
                                        file_type,
                                        copy_info,
                                    )
                                }
                                thrift::RepoCreateCommitParamsFileContent::content_sha1(sha) => {
                                    let sha = Sha1::from_request(&sha)?;
                                    let file = repo
                                        .file_by_content_sha1(sha)
                                        .await?
                                        .ok_or_else(|| errors::file_not_found(sha.to_string()))?;
                                    CreateChange::ExistingContent(
                                        file.id().await?,
                                        file_type,
                                        copy_info,
                                    )
                                }
                                thrift::RepoCreateCommitParamsFileContent::content_sha256(sha) => {
                                    let sha = Sha256::from_request(&sha)?;
                                    let file = repo
                                        .file_by_content_sha256(sha)
                                        .await?
                                        .ok_or_else(|| errors::file_not_found(sha.to_string()))?;
                                    CreateChange::ExistingContent(
                                        file.id().await?,
                                        file_type,
                                        copy_info,
                                    )
                                }
                                thrift::RepoCreateCommitParamsFileContent::data(data) => {
                                    CreateChange::NewContent(
                                        Bytes::from(data),
                                        file_type,
                                        copy_info,
                                    )
                                }
                                thrift::RepoCreateCommitParamsFileContent::UnknownField(t) => {
                                    return Err(errors::invalid_request(format!(
                                        "file content type not supported: {}",
                                        t
                                    ))
                                    .into());
                                }
                            }
                        }
                        thrift::RepoCreateCommitParamsChange::deleted(_d) => CreateChange::Delete,
                        thrift::RepoCreateCommitParamsChange::UnknownField(t) => {
                            return Err(errors::invalid_request(format!(
                                "file change type not supported: {}",
                                t
                            ))
                            .into());
                        }
                    };
                    Ok::<_, errors::ServiceError>((path, change))
                }
            })
            .collect::<FuturesOrdered<_>>()
            .try_collect()
            .await?;

        let author = params.info.author;
        let author_date = params
            .info
            .date
            .as_ref()
            .map(<DateTime<FixedOffset>>::from_request)
            .unwrap_or_else(|| {
                let now = Local::now();
                Ok(now.with_timezone(now.offset()))
            })?;
        let committer = None;
        let committer_date = None;
        let message = params.info.message;
        let extra = params.info.extra;

        let changeset = repo
            .create_changeset(
                parents,
                author,
                author_date,
                committer,
                committer_date,
                message,
                extra,
                file_changes,
            )
            .await?;
        let ids = map_commit_identity(&changeset, &params.identity_schemes).await?;
        Ok(thrift::RepoCreateCommitResponse { ids })
    }

    /// Build stacks for the given list of heads.
    ///
    /// Returns the IDs of the changeset in the requested identity schemes.
    /// Draft nodes and first public roots.
    ///
    /// Best effort: missing changesets are skipped,
    ///              building stack up to provided limit.
    pub(crate) async fn repo_stack_info(
        &self,
        ctx: CoreContext,
        repo: thrift::RepoSpecifier,
        params: thrift::RepoStackInfoParams,
    ) -> Result<thrift::RepoStackInfoResponse, errors::ServiceError> {
        let repo = self.repo(ctx, &repo).await?;

        // parse changeset specifiers from params
        let head_specifiers = params
            .heads
            .iter()
            .map(ChangesetSpecifier::from_request)
            .collect::<Result<Vec<_>, _>>()?;

        // convert changeset specifiers to bonsai changeset ids
        // missing changesets are skipped
        let heads_ids = try_join_all(
            head_specifiers
                .into_iter()
                .map(|specifier| repo.resolve_specifier(specifier)),
        )
        .await?
        .into_iter()
        .filter_map(identity)
        .collect::<Vec<_>>();

        // get stack
        let stack = repo.stack(heads_ids, params.limit as usize).await?;

        // resolve draft changesets & public changesets
        let (draft_commits, public_parents) = try_join(
            try_join_all(
                stack
                    .draft
                    .into_iter()
                    .map(|cs_id| repo.changeset(ChangesetSpecifier::Bonsai(cs_id))),
            ),
            try_join_all(
                stack
                    .public
                    .into_iter()
                    .map(|cs_id| repo.changeset(ChangesetSpecifier::Bonsai(cs_id))),
            ),
        )
        .await?;

        // generate response
        match (
            draft_commits.into_iter().collect::<Option<Vec<_>>>(),
            public_parents.into_iter().collect::<Option<Vec<_>>>(),
        ) {
            (Some(draft_commits), Some(public_parents)) => {
                let (draft_commits, public_parents) = try_join(
                    try_join_all(
                        draft_commits
                            .into_iter()
                            .map(|cs| (cs, &params.identity_schemes).into_response()),
                    ),
                    try_join_all(
                        public_parents
                            .into_iter()
                            .map(|cs| (cs, &params.identity_schemes).into_response()),
                    ),
                )
                .await?;
                Ok(thrift::RepoStackInfoResponse {
                    draft_commits,
                    public_parents,
                })
            }
            _ => Err(
                errors::internal_error("unexpected failure to resolve an existing commit").into(),
            ),
        }
    }

    pub(crate) async fn repo_move_bookmark(
        &self,
        ctx: CoreContext,
        repo: thrift::RepoSpecifier,
        params: thrift::RepoMoveBookmarkParams,
    ) -> Result<thrift::RepoMoveBookmarkResponse, errors::ServiceError> {
        let repo = self.repo(ctx, &repo).await?;
        let repo = match params.service_identity {
            Some(service_identity) => repo.service_write(service_identity).await?,
            None => repo.write().await?,
        };
        let bookmark = &params.bookmark;
        let target = &params.target;
        let changeset_specifier = ChangesetSpecifier::from_request(target)?;
        let changeset = repo
            .changeset(changeset_specifier)
            .await?
            .ok_or_else(|| errors::commit_not_found(target.to_string()))?;

        // TODO(mbthomas): provide a way for the client to optionally specify the old value
        let old_changeset = repo.resolve_bookmark(bookmark).await?.ok_or_else(|| {
            errors::invalid_request(format!("bookmark {} does not exist", bookmark))
        })?;

        repo.move_bookmark(
            bookmark,
            changeset.id(),
            old_changeset.id(),
            params.allow_non_fast_forward_move,
        )
        .await?;
        Ok(thrift::RepoMoveBookmarkResponse {})
    }
}
