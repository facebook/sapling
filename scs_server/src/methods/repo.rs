/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use std::collections::BTreeMap;
use std::convert::TryFrom;

use bytes::Bytes;
use chrono::{DateTime, FixedOffset, Local};
use futures::stream::Stream;
use futures_preview::compat::Future01CompatExt;
use futures_util::stream::FuturesOrdered;
use futures_util::TryStreamExt;
use mononoke_api::{
    ChangesetSpecifier, CreateChange, CreateCopyInfo, FileId, FileType, MononokePath,
};
use mononoke_types::hash::{Sha1, Sha256};
use source_control as thrift;
use source_control::services::source_control_service as service;
use srserver::RequestContext;

use crate::commit_id::{map_commit_identities, map_commit_identity, CommitIdExt};
use crate::errors;
use crate::from_request::{check_range_and_convert, FromRequest};
use crate::source_control_impl::SourceControlServiceImpl;

impl SourceControlServiceImpl {
    /// Resolve a bookmark to a changeset.
    ///
    /// Returns whether the bookmark exists, and the IDs of the changeset in
    /// the requested indentity schemes.
    pub(crate) async fn repo_resolve_bookmark(
        &self,
        req_ctxt: &RequestContext,
        repo: thrift::RepoSpecifier,
        params: thrift::RepoResolveBookmarkParams,
    ) -> Result<thrift::RepoResolveBookmarkResponse, service::RepoResolveBookmarkExn> {
        let ctx = self.create_ctx(req_ctxt, Some(&repo))?;
        let repo = self.repo(ctx, &repo)?;
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

    /// List bookmarks.
    pub(crate) async fn repo_list_bookmarks(
        &self,
        req_ctxt: &RequestContext,
        repo: thrift::RepoSpecifier,
        params: thrift::RepoListBookmarksParams,
    ) -> Result<thrift::RepoListBookmarksResponse, service::RepoListBookmarksExn> {
        let ctx = self.create_ctx(req_ctxt, Some(&repo))?;
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
        let repo = self.repo(ctx, &repo)?;
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
        req_ctxt: &RequestContext,
        repo: thrift::RepoSpecifier,
        params: thrift::RepoCreateCommitParams,
    ) -> Result<thrift::RepoCreateCommitResponse, service::RepoCreateCommitExn> {
        let ctx = self.create_ctx(req_ctxt, Some(&repo))?;
        let repo = self.repo(ctx, &repo)?.write().await?;

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
}
