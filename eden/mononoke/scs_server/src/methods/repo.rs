/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;
use std::str::FromStr;

use anyhow::anyhow;
use bookmarks::BookmarkKey;
use bytes::Bytes;
use chrono::DateTime;
use chrono::FixedOffset;
use context::CoreContext;
use derived_data_manager::DerivableType;
use futures::future::try_join_all;
use futures::stream;
use futures::stream::FuturesOrdered;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use futures::try_join;
use maplit::btreemap;
use metaconfig_types::CommitIdentityScheme;
use mononoke_api::BookmarkFreshness;
use mononoke_api::ChangesetId;
use mononoke_api::ChangesetPrefixSpecifier;
use mononoke_api::ChangesetSpecifier;
use mononoke_api::ChangesetSpecifierPrefixResolution;
use mononoke_api::CreateChange;
use mononoke_api::CreateChangeFile;
use mononoke_api::CreateChangeFileContents;
use mononoke_api::CreateChangeGitLfs;
use mononoke_api::CreateCopyInfo;
use mononoke_api::CreateInfo;
use mononoke_api::FileId;
use mononoke_api::FileType;
use mononoke_api::MononokeError;
use mononoke_api::Repo;
use mononoke_api::RepoContext;
use mononoke_api::StoreRequest;
use mononoke_api::SubmoduleExpansionUpdate;
use mononoke_api::SubmoduleExpansionUpdateCommitInfo;
use mononoke_types::hash::GitSha1;
use mononoke_types::hash::Sha1;
use mononoke_types::hash::Sha256;
use mononoke_types::path::MPath;
use mononoke_types::DateTime as MononokeDateTime;
use mononoke_types::NonRootMPath;
use mononoke_types::ThriftConvert;
use repo_authorization::AuthorizationContext;
use source_control as thrift;

use crate::commit_id::map_commit_identities;
use crate::commit_id::map_commit_identity;
use crate::commit_id::CommitIdExt;
use crate::errors;
use crate::errors::ServiceErrorResultExt;
use crate::from_request::check_range_and_convert;
use crate::from_request::convert_pushvars;
use crate::from_request::FromRequest;
use crate::into_response::AsyncIntoResponseWith;
use crate::source_control_impl::SourceControlServiceImpl;

mod land_stack;

impl SourceControlServiceImpl {
    /// Detailed repo info.
    ///
    /// Returns detailed information about a repository.
    pub(crate) async fn repo_info(
        &self,
        ctx: CoreContext,
        repo: thrift::RepoSpecifier,
        _params: thrift::RepoInfoParams,
    ) -> Result<thrift::RepoInfo, errors::ServiceError> {
        let authz = AuthorizationContext::new_bypass_access_control();
        let repo = self
            .repo_impl(ctx, &repo, authz, |_| async { Ok(None) })
            .await?;
        let repo_name = repo.name();

        let default_commit_identity_scheme_conf = &repo.config().default_commit_identity_scheme;

        let default_commit_identity_scheme = match default_commit_identity_scheme_conf {
            CommitIdentityScheme::HG => thrift::CommitIdentityScheme::HG,
            CommitIdentityScheme::GIT => thrift::CommitIdentityScheme::GIT,
            CommitIdentityScheme::BONSAI => thrift::CommitIdentityScheme::BONSAI,
            CommitIdentityScheme::UNKNOWN => thrift::CommitIdentityScheme::UNKNOWN,
        };

        Ok(thrift::RepoInfo {
            name: repo_name.to_string(),
            default_commit_identity_scheme,
            ..Default::default()
        })
    }

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
        match repo
            .resolve_bookmark(
                &BookmarkKey::new(&params.bookmark_name).map_err(Into::<MononokeError>::into)?,
                BookmarkFreshness::MaybeStale,
            )
            .await?
        {
            Some(cs) => {
                let ids = map_commit_identity(&cs, &params.identity_schemes).await?;
                Ok(thrift::RepoResolveBookmarkResponse {
                    exists: true,
                    ids: Some(ids),
                    ..Default::default()
                })
            }
            None => Ok(thrift::RepoResolveBookmarkResponse {
                exists: false,
                ids: None,
                ..Default::default()
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
                    ..Default::default()
                })
            }
            Single(ChangesetSpecifier::Hg(cs_id)) if same_request_response_schemes => {
                Ok(Response {
                    ids: Some(btreemap! {
                        params.prefix_scheme => thrift::CommitId::hg(cs_id.as_ref().into())
                    }),
                    resolved_type: ResponseType::RESOLVED,
                    ..Default::default()
                })
            }
            Single(ChangesetSpecifier::GitSha1(cs_id)) if same_request_response_schemes => {
                Ok(Response {
                    ids: Some(btreemap! {
                        params.prefix_scheme => thrift::CommitId::git(cs_id.as_ref().into())
                    }),
                    resolved_type: ResponseType::RESOLVED,
                    ..Default::default()
                })
            }
            Single(cs_id) => match &repo.changeset(cs_id).await? {
                None => Err(errors::internal_error(
                    "unexpected failure to resolve an existing commit",
                )
                .into()),
                Some(cs) => Ok(Response {
                    ids: Some(map_commit_identity(cs, &params.identity_schemes).await?),
                    resolved_type: ResponseType::RESOLVED,
                    ..Default::default()
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

    /// Comprehensive bookmark info.
    ///
    /// Returns value of the bookmark (both fresh and warm) and the timestamp of
    /// last update.
    pub(crate) async fn repo_bookmark_info(
        &self,
        ctx: CoreContext,
        repo: thrift::RepoSpecifier,
        params: thrift::RepoBookmarkInfoParams,
    ) -> Result<thrift::RepoBookmarkInfoResponse, errors::ServiceError> {
        let repo = self.repo(ctx, &repo).await?;
        let info = repo.bookmark_info(params.bookmark_name).await?;
        Ok(thrift::RepoBookmarkInfoResponse {
            info: match info {
                Some(info) => Some(info.into_response_with(&params.identity_schemes).await?),
                None => None,
            },
            ..Default::default()
        })
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
        let ids = bookmarks.iter().map(|(_name, cs_id)| *cs_id).collect();
        let id_mapping = map_commit_identities(&repo, ids, &params.identity_schemes).await?;
        let bookmarks = bookmarks
            .into_iter()
            .map(|(name, cs_id)| match id_mapping.get(&cs_id) {
                Some(ids) => (name, ids.clone()),
                None => (name, BTreeMap::new()),
            })
            .collect();
        Ok(thrift::RepoListBookmarksResponse {
            bookmarks,
            continue_after,
            ..Default::default()
        })
    }

    async fn convert_create_commit_parents(
        repo: &RepoContext<Repo>,
        parents: &[thrift::CommitId],
    ) -> Result<Vec<ChangesetId>, errors::ServiceError> {
        let parents: Vec<_> = parents
            .iter()
            .map(|parent| async move {
                let changeset_specifier = ChangesetSpecifier::from_request(parent)
                    .context("invalid commit id for parent")?;
                let changeset = repo
                    .changeset(changeset_specifier)
                    .await?
                    .ok_or_else(|| errors::commit_not_found(parent.to_string()))?;
                Ok::<_, errors::ServiceError>(changeset.id())
            })
            .collect::<FuturesOrdered<_>>()
            .try_collect()
            .await?;

        if parents.is_empty() && !repo.allow_no_parent_writes() {
            return Err(errors::invalid_request(
                "this repo does not permit commits without a parent",
            )
            .into());
        }

        Ok(parents)
    }

    async fn convert_create_commit_file_content(
        repo: &RepoContext<Repo>,
        content: thrift::RepoCreateCommitParamsFileContent,
    ) -> Result<CreateChangeFileContents, errors::ServiceError> {
        let contents = match content {
            thrift::RepoCreateCommitParamsFileContent::id(id) => {
                let file_id = FileId::from_request(&id)?;
                let file = repo
                    .file(file_id)
                    .await?
                    .ok_or_else(|| errors::file_not_found(file_id.to_string()))?;
                CreateChangeFileContents::Existing {
                    file_id: file.id().await?,
                    maybe_size: None,
                }
            }
            thrift::RepoCreateCommitParamsFileContent::content_sha1(sha) => {
                let sha = Sha1::from_request(&sha)?;
                let file = repo
                    .file_by_content_sha1(sha)
                    .await?
                    .ok_or_else(|| errors::file_not_found(sha.to_string()))?;
                CreateChangeFileContents::Existing {
                    file_id: file.id().await?,
                    maybe_size: None,
                }
            }
            thrift::RepoCreateCommitParamsFileContent::content_sha256(sha) => {
                let sha = Sha256::from_request(&sha)?;
                let file = repo
                    .file_by_content_sha256(sha)
                    .await?
                    .ok_or_else(|| errors::file_not_found(sha.to_string()))?;
                CreateChangeFileContents::Existing {
                    file_id: file.id().await?,
                    maybe_size: None,
                }
            }
            thrift::RepoCreateCommitParamsFileContent::content_gitsha1(sha) => {
                let sha = GitSha1::from_request(&sha)?;
                let file = repo
                    .file_by_content_gitsha1(sha)
                    .await?
                    .ok_or_else(|| errors::file_not_found(sha.to_string()))?;
                CreateChangeFileContents::Existing {
                    file_id: file.id().await?,
                    maybe_size: None,
                }
            }
            thrift::RepoCreateCommitParamsFileContent::data(data) => {
                CreateChangeFileContents::New {
                    bytes: Bytes::from(data),
                }
            }
            thrift::RepoCreateCommitParamsFileContent::UnknownField(t) => {
                return Err(errors::invalid_request(format!(
                    "file content type not supported: {}",
                    t
                ))
                .into());
            }
        };
        Ok(contents)
    }

    async fn convert_create_commit_change(
        repo: &RepoContext<Repo>,
        change: thrift::RepoCreateCommitParamsChange,
    ) -> Result<CreateChange, errors::ServiceError> {
        let change = match change {
            thrift::RepoCreateCommitParamsChange::changed(c) => {
                let file_type = FileType::from_request(&c.r#type)?;
                let git_lfs = match c.git_lfs {
                    // Right now the default is to use full content when client didn't explicitly
                    // request LFS but we can change it in the future to something smarter.
                    None => None,
                    // User explicitly prefers full content
                    Some(git_lfs) => Some(match git_lfs {
                        thrift::RepoCreateCommitParamsGitLfs::full_content(_unused) => {
                            CreateChangeGitLfs::FullContent
                        }
                        thrift::RepoCreateCommitParamsGitLfs::lfs_pointer(_unused) => {
                            CreateChangeGitLfs::GitLfsPointer {
                                non_canonical_pointer: None,
                            }
                        }
                        thrift::RepoCreateCommitParamsGitLfs::non_canonical_lfs_pointer(
                            non_canonical_lfs_pointer,
                        ) => CreateChangeGitLfs::GitLfsPointer {
                            non_canonical_pointer: Some(
                                Self::convert_create_commit_file_content(
                                    repo,
                                    non_canonical_lfs_pointer,
                                )
                                .await?,
                            ),
                        },
                        thrift::RepoCreateCommitParamsGitLfs::UnknownField(t) => {
                            return Err(errors::invalid_request(format!(
                                "git lfs variant not supported: {}",
                                t
                            ))
                            .into());
                        }
                    }),
                };

                let copy_info = c
                    .copy_info
                    .as_ref()
                    .map(CreateCopyInfo::from_request)
                    .transpose()?;
                let contents = Self::convert_create_commit_file_content(repo, c.content).await?;
                CreateChange::Tracked(
                    CreateChangeFile {
                        contents,
                        file_type,
                        git_lfs,
                    },
                    copy_info,
                )
            }
            thrift::RepoCreateCommitParamsChange::deleted(_d) => CreateChange::Deletion,
            thrift::RepoCreateCommitParamsChange::UnknownField(t) => {
                return Err(errors::invalid_request(format!(
                    "file change type not supported: {}",
                    t
                ))
                .into());
            }
        };
        Ok(change)
    }

    async fn convert_create_commit_changes(
        repo: &RepoContext<Repo>,
        changes: BTreeMap<String, thrift::RepoCreateCommitParamsChange>,
    ) -> Result<BTreeMap<MPath, CreateChange>, errors::ServiceError> {
        let changes = changes
            .into_iter()
            .map(|(path, change)| async move {
                let path = MPath::try_from(&path).map_err(|e| {
                    errors::invalid_request(format!("invalid path '{}': {}", path, e))
                })?;
                let change = Self::convert_create_commit_change(repo, change).await?;
                Ok::<_, errors::ServiceError>((path, change))
            })
            .collect::<FuturesOrdered<_>>()
            .try_collect()
            .await?;

        Ok(changes)
    }

    /// Create a new commit.
    pub(crate) async fn repo_create_commit(
        &self,
        ctx: CoreContext,
        repo: thrift::RepoSpecifier,
        params: thrift::RepoCreateCommitParams,
    ) -> Result<thrift::RepoCreateCommitResponse, errors::ServiceError> {
        let repo = self
            .repo_for_service(ctx, &repo, params.service_identity.clone())
            .await?;

        let parents = Self::convert_create_commit_parents(&repo, &params.parents).await?;
        let info = CreateInfo::from_request(&params.info)?;
        let changes = Self::convert_create_commit_changes(&repo, params.changes).await?;
        let bubble = None;

        let (hg_extra, changeset) = repo
            .create_changeset(parents, info, changes, bubble)
            .await?;

        // If you ask for a git identity back, then we'll assume that you supplied one to us
        // and set it. Later, when we can derive a git commit hash, this'll become more
        // open, because we'll only do the check if you ask for a hash different to the
        // one we would derive
        if params
            .identity_schemes
            .contains(&thrift::CommitIdentityScheme::GIT)
        {
            repo.set_git_mapping_from_changeset(&changeset, &hg_extra)
                .await?;
        }
        let ids = map_commit_identity(&changeset, &params.identity_schemes).await?;
        Ok(thrift::RepoCreateCommitResponse {
            ids,
            ..Default::default()
        })
    }

    /// Create a new stack of commits.
    pub(crate) async fn repo_create_stack(
        &self,
        ctx: CoreContext,
        repo: thrift::RepoSpecifier,
        params: thrift::RepoCreateStackParams,
    ) -> Result<thrift::RepoCreateStackResponse, errors::ServiceError> {
        let batch_size = params.commits.len() as u64;
        let repo = self
            .repo_for_service(ctx.clone(), &repo, params.service_identity.clone())
            .await?;
        let repo = &repo;

        let stack_parents = Self::convert_create_commit_parents(repo, &params.parents).await?;
        let info_stack = params
            .commits
            .iter()
            .map(|commit| CreateInfo::from_request(&commit.info))
            .collect::<Result<Vec<_>, _>>()?;
        let changes_stack =
            stream::iter(
                params.commits.into_iter().map({
                    |commit| async move {
                        Self::convert_create_commit_changes(repo, commit.changes).await
                    }
                }),
            )
            .buffered(10)
            .try_collect::<Vec<_>>()
            .await?;
        let bubble = None;
        let stack = repo
            .create_changeset_stack(stack_parents, info_stack, changes_stack, bubble)
            .await?;
        // If you ask for a git identity back, then we'll assume that you supplied one to us
        // and set it. Later, when we can derive a git commit hash, this'll become more
        // open, because we'll only do the check if you ask for a hash different to the
        // one we would derive
        if params
            .identity_schemes
            .contains(&thrift::CommitIdentityScheme::GIT)
        {
            for (hg_extra, changeset_ctx) in stack.iter() {
                repo.set_git_mapping_from_changeset(changeset_ctx, hg_extra)
                    .await?;
            }
        }

        if let Some(prepare_types) = &params.prepare_derived_data_types {
            let csids = stack
                .iter()
                .map(|(_hg_extra, c)| c.id())
                .collect::<Vec<_>>();
            let derived_data_types = prepare_types
                .iter()
                .map(DerivableType::from_request)
                .collect::<Result<Vec<_>, _>>()?;
            repo.derive_bulk(&ctx, csids, &derived_data_types, Some(batch_size))
                .await?;
        }

        let identity_schemes = &params.identity_schemes;
        let commit_ids = stream::iter(stack.into_iter().map(|(_hg_extra, changeset)| async move {
            map_commit_identity(&changeset, identity_schemes).await
        }))
        .buffered(10)
        .try_collect::<Vec<_>>()
        .await?;
        Ok(thrift::RepoCreateStackResponse {
            commit_ids,
            ..Default::default()
        })
    }

    /// Build stacks for the given list of heads.
    ///
    /// Returns the IDs of the changeset in the requested identity schemes.
    /// Draft nodes and first public roots.
    /// The changesets are returned in topological order.
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

        // Check the limit
        let limit = check_range_and_convert(
            "limit",
            params.limit,
            0..=thrift::consts::REPO_STACK_INFO_MAX_LIMIT,
        )?;

        // parse changeset specifiers from params
        let head_specifiers = params
            .heads
            .iter()
            .map(ChangesetSpecifier::from_request)
            .collect::<Result<Vec<_>, _>>()?;

        // convert changeset specifiers to bonsai changeset ids
        // missing changesets are skipped
        #[allow(clippy::filter_map_identity)]
        let heads_ids = try_join_all(
            head_specifiers
                .into_iter()
                .map(|specifier| repo.resolve_specifier(specifier)),
        )
        .await?
        .into_iter()
        .filter_map(std::convert::identity)
        .collect::<Vec<_>>();

        // get stack
        let stack = repo.stack(heads_ids, limit).await?;

        // resolve draft changesets & public changesets
        let (draft_commits, public_parents, leftover_heads) = try_join!(
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
            try_join_all(
                stack
                    .leftover_heads
                    .into_iter()
                    .map(|cs_id| repo.changeset(ChangesetSpecifier::Bonsai(cs_id))),
            ),
        )?;

        if draft_commits.len() <= params.heads.len() && !leftover_heads.is_empty() {
            Err(errors::limit_too_low(limit))?;
        }

        // generate response
        match (
            draft_commits.into_iter().collect::<Option<Vec<_>>>(),
            public_parents.into_iter().collect::<Option<Vec<_>>>(),
            leftover_heads.into_iter().collect::<Option<Vec<_>>>(),
        ) {
            (Some(draft_commits), Some(public_parents), Some(leftover_heads)) => {
                let (mut draft_commits, public_parents, leftover_heads) = try_join!(
                    try_join_all(
                        draft_commits
                            .into_iter()
                            .map(|cs| cs.into_response_with(&params.identity_schemes)),
                    ),
                    try_join_all(
                        public_parents
                            .into_iter()
                            .map(|cs| cs.into_response_with(&params.identity_schemes)),
                    ),
                    leftover_heads.into_response_with(&params.identity_schemes),
                )?;

                // Need to return the draft commits in topological order to meet the API definition
                // at https://fburl.com/code/a017qoam.
                draft_commits.sort_by_key(|commit| commit.generation);
                draft_commits.reverse();

                Ok(thrift::RepoStackInfoResponse {
                    draft_commits,
                    public_parents,
                    leftover_heads,
                    ..Default::default()
                })
            }
            _ => Err(
                errors::internal_error("unexpected failure to resolve an existing commit").into(),
            ),
        }
    }

    pub(crate) async fn repo_create_bookmark(
        &self,
        ctx: CoreContext,
        repo: thrift::RepoSpecifier,
        params: thrift::RepoCreateBookmarkParams,
    ) -> Result<thrift::RepoCreateBookmarkResponse, errors::ServiceError> {
        let repo = self
            .repo_for_service(ctx, &repo, params.service_identity)
            .await?;
        let target = &params.target;
        let changeset = repo
            .changeset(ChangesetSpecifier::from_request(target)?)
            .await?
            .ok_or_else(|| errors::commit_not_found(target.to_string()))?;
        let pushvars = convert_pushvars(params.pushvars);

        repo.create_bookmark(
            &BookmarkKey::new(&params.bookmark).map_err(Into::<MononokeError>::into)?,
            changeset.id(),
            pushvars.as_ref(),
        )
        .await?;
        Ok(thrift::RepoCreateBookmarkResponse {
            ..Default::default()
        })
    }

    pub(crate) async fn repo_move_bookmark(
        &self,
        ctx: CoreContext,
        repo: thrift::RepoSpecifier,
        params: thrift::RepoMoveBookmarkParams,
    ) -> Result<thrift::RepoMoveBookmarkResponse, errors::ServiceError> {
        let repo = self
            .repo_for_service(ctx, &repo, params.service_identity)
            .await?;
        let target = &params.target;
        let changeset = repo
            .changeset(ChangesetSpecifier::from_request(target)?)
            .await?
            .ok_or_else(|| errors::commit_not_found(target.to_string()))?;
        let old_changeset_id = match &params.old_target {
            Some(old_target) => Some(
                repo.changeset(ChangesetSpecifier::from_request(old_target)?)
                    .await
                    .context("failed to resolve old target")?
                    .ok_or_else(|| errors::commit_not_found(old_target.to_string()))?
                    .id(),
            ),
            None => None,
        };
        let pushvars = convert_pushvars(params.pushvars);

        repo.move_bookmark(
            &BookmarkKey::new(&params.bookmark).map_err(Into::<MononokeError>::into)?,
            changeset.id(),
            old_changeset_id,
            params.allow_non_fast_forward_move,
            pushvars.as_ref(),
        )
        .await?;
        Ok(thrift::RepoMoveBookmarkResponse {
            ..Default::default()
        })
    }

    pub(crate) async fn repo_delete_bookmark(
        &self,
        ctx: CoreContext,
        repo: thrift::RepoSpecifier,
        params: thrift::RepoDeleteBookmarkParams,
    ) -> Result<thrift::RepoDeleteBookmarkResponse, errors::ServiceError> {
        let repo = self
            .repo_for_service(ctx, &repo, params.service_identity)
            .await?;
        let old_changeset_id = match &params.old_target {
            Some(old_target) => Some(
                repo.changeset(ChangesetSpecifier::from_request(old_target)?)
                    .await?
                    .ok_or_else(|| errors::commit_not_found(old_target.to_string()))?
                    .id(),
            ),
            None => None,
        };
        let pushvars = convert_pushvars(params.pushvars);

        repo.delete_bookmark(
            &BookmarkKey::new(&params.bookmark).map_err(Into::<MononokeError>::into)?,
            old_changeset_id,
            pushvars.as_ref(),
        )
        .await?;

        Ok(thrift::RepoDeleteBookmarkResponse {
            ..Default::default()
        })
    }

    /// Prepare commits for future operations.
    ///
    /// Perform any necessary pre-processing on the mononoke side to ensure that the commits
    /// are ready to be used later without incurring a performance penalty for repeated
    /// preparation.
    ///
    /// For now, concretely, "preparing" means deriving the provided derived data type for a batch of commits.
    ///
    /// * The dependencies and ancestors of most commits in the batch must have already been derived.
    ///
    /// If these conditions are not met, this endpoint may take an unbounded time to derive all
    /// ancestors and timeout.
    pub(crate) async fn repo_prepare_commits(
        &self,
        ctx: CoreContext,
        repo: thrift::RepoSpecifier,
        params: thrift::RepoPrepareCommitsParams,
    ) -> Result<thrift::RepoPrepareCommitsResponse, errors::ServiceError> {
        let repo = self.repo(ctx.clone(), &repo).await?;
        // Convert thrift commit ids to bonsai changeset ids
        let changesets = try_join_all(
            params
                .commits
                .iter()
                .map(ChangesetSpecifier::from_request)
                .collect::<Result<Vec<_>, _>>()?
                .into_iter()
                .map(|specifier| repo.changeset(specifier)),
        )
        .await?;
        let csids = std::iter::zip(params.commits, changesets)
            .map(|(commit, cs)| {
                cs.map(|cs| cs.id())
                    .ok_or_else(|| errors::commit_not_found(commit.to_string()))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let derived_data_type = DerivableType::from_request(&params.derived_data_type)?;

        const CONCURRENCY: u64 = 1000;
        repo.derive_bulk(&ctx, csids, &[derived_data_type], Some(CONCURRENCY))
            .await?;

        Ok(thrift::RepoPrepareCommitsResponse {
            ..Default::default()
        })
    }

    pub(crate) async fn repo_upload_file_content(
        &self,
        ctx: CoreContext,
        repo: thrift::RepoSpecifier,
        params: thrift::RepoUploadFileContentParams,
    ) -> Result<thrift::RepoUploadFileContentResponse, errors::ServiceError> {
        let repo = self
            .repo_for_service(ctx, &repo, params.service_identity)
            .await?;
        let mut store_request = StoreRequest::new(
            params
                .data
                .len()
                .try_into()
                .expect("usize should convert to u64"),
        );
        if let Some(expected_content_sha1) = &params.expected_content_sha1 {
            store_request.sha1 = Some(Sha1::from_request(expected_content_sha1)?);
        }
        if let Some(expected_content_sha256) = &params.expected_content_sha256 {
            store_request.sha256 = Some(Sha256::from_request(expected_content_sha256)?);
        }
        if params.expected_content_seeded_blake3.is_some() {
            return Err(errors::invalid_request(
                "Seeded blake3 not yet implemented for file upload",
            )
            .into());
        }

        let id = repo
            .upload_file_content(params.data.into(), &store_request)
            .await?
            .as_ref()
            .to_vec();
        Ok(thrift::RepoUploadFileContentResponse {
            id,
            ..Default::default()
        })
    }

    /// Update a submodule expansion in the large repo, i.e. change the commit
    /// being expanded or delete the expansion entirely.
    pub(crate) async fn repo_update_submodule_expansion(
        &self,
        ctx: CoreContext,
        params: thrift::RepoUpdateSubmoduleExpansionParams,
    ) -> Result<thrift::RepoUpdateSubmoduleExpansionResponse, errors::ServiceError> {
        let large_repo_ctx = self.repo(ctx.clone(), &params.large_repo).await?;

        let base_cs_specifier = ChangesetSpecifier::from_request(&params.base_commit_id)?;
        let base_changeset_id = large_repo_ctx
            .resolve_specifier(base_cs_specifier)
            .await?
            .ok_or_else(|| {
                MononokeError::InvalidRequest(format!(
                    "unknown commit specifier {}",
                    base_cs_specifier
                ))
            })?;
        let submodule_expansion_path =
            NonRootMPath::new(params.submodule_expansion_path.as_bytes())
                .map_err(MononokeError::from)?;

        let commit_info_params = params.commit_info.unwrap_or_default();
        // TODO(T179531912): expose more metadata fields in API
        let author = if commit_info_params.author.is_some() {
            commit_info_params.author
        } else {
            ctx.session().metadata().unix_name().map(String::from)
        };

        let author_date = commit_info_params
            .author_date
            .as_ref()
            .map(<DateTime<FixedOffset>>::from_request)
            .transpose()?
            .map(MononokeDateTime::new);

        let commit_info = SubmoduleExpansionUpdateCommitInfo {
            author,
            message: commit_info_params.message,
            author_date,
        };

        let submodule_expansion_update = match params.new_submodule_commit_or_delete {
            Some(thrift::CommitId::git(commit_hash_data)) => {
                let commit_hash_string = String::from_utf8(commit_hash_data)
                    .map_err(anyhow::Error::from)
                    .map_err(MononokeError::from)
                    .context("Git commit hash encoding")?;
                // TODO(T179531912): support other hashes
                let git_commit_id_bytes = GitSha1::from_str(commit_hash_string.as_str())
                    .map_err(anyhow::Error::from)
                    .map_err(MononokeError::from)
                    .context("GitSha1 creation")?;
                SubmoduleExpansionUpdate::UpdateCommit(git_commit_id_bytes)
            }
            Some(_) => {
                return Err(errors::invalid_request(anyhow!(
                    "New submodule commit is not a valid git commit hash"
                ))
                .into());
            }
            None => SubmoduleExpansionUpdate::Delete,
        };

        let cs_ctx = large_repo_ctx
            .update_submodule_expansion(
                base_changeset_id,
                submodule_expansion_path,
                submodule_expansion_update,
                commit_info,
            )
            .await?;

        let mut commit_ids = btreemap! {};
        for scheme in params.identity_schemes {
            let commit_id = match scheme {
                thrift::CommitIdentityScheme::BONSAI => {
                    thrift::CommitId::bonsai(cs_ctx.id().into_bytes().into())
                }
                thrift::CommitIdentityScheme::HG => thrift::CommitId::hg(
                    cs_ctx
                        .hg_id()
                        .await?
                        .ok_or_else(|| {
                            errors::internal_error(format!(
                                "No hg mapping found for changeset {}",
                                cs_ctx.id()
                            ))
                        })?
                        .as_bytes()
                        .to_vec(),
                ),
                thrift::CommitIdentityScheme::GIT => thrift::CommitId::git(
                    cs_ctx
                        .git_sha1()
                        .await?
                        .ok_or_else(|| {
                            errors::internal_error(format!(
                                "No git mapping found for changeset {}",
                                cs_ctx.id()
                            ))
                        })?
                        .into_inner()
                        .to_vec(),
                ),
                _ => {
                    return Err(errors::invalid_request(format!(
                        "{scheme} scheme is not supported"
                    ))
                    .into());
                }
            };
            commit_ids.insert(scheme, commit_id);
        }

        Ok(thrift::RepoUpdateSubmoduleExpansionResponse {
            ids: commit_ids,
            ..Default::default()
        })
    }
}
