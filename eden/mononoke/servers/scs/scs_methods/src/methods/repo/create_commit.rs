/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;

use bytes::Bytes;
use context::CoreContext;
use derived_data_manager::DerivableType;
use futures::stream;
use futures::stream::FuturesOrdered;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use metaconfig_types::CommitIdentityScheme;
use mononoke_api::ChangesetId;
use mononoke_api::ChangesetSpecifier;
use mononoke_api::CreateChange;
use mononoke_api::CreateChangeFile;
use mononoke_api::CreateChangeFileContents;
use mononoke_api::CreateChangeGitLfs;
use mononoke_api::CreateChangesetChecks;
use mononoke_api::CreateCopyInfo;
use mononoke_api::CreateInfo;
use mononoke_api::FileId;
use mononoke_api::FileType;
use mononoke_api::Repo;
use mononoke_api::RepoContext;
use mononoke_types::DateTime as MononokeDateTime;
use mononoke_types::hash::GitSha1;
use mononoke_types::hash::Sha1;
use mononoke_types::hash::Sha256;
use mononoke_types::path::MPath;
use scs_errors::ServiceErrorResultExt;
use source_control as thrift;

use crate::commit_id::map_commit_identity;
use crate::from_request::FromRequest;
use crate::source_control_impl::SourceControlServiceImpl;

impl SourceControlServiceImpl {
    async fn convert_create_commit_parents(
        repo: &RepoContext<Repo>,
        parents: &[thrift::CommitId],
    ) -> Result<Vec<ChangesetId>, scs_errors::ServiceError> {
        let parents: Vec<_> = parents
            .iter()
            .map(|parent| async move {
                let changeset_specifier = ChangesetSpecifier::from_request(parent)
                    .context("invalid commit id for parent")?;
                let changeset = repo
                    .changeset(changeset_specifier)
                    .await?
                    .ok_or_else(|| scs_errors::commit_not_found(parent.to_string()))?;
                Ok::<_, scs_errors::ServiceError>(changeset.id())
            })
            .collect::<FuturesOrdered<_>>()
            .try_collect()
            .await?;

        if parents.is_empty() && !repo.allow_no_parent_writes() {
            return Err(scs_errors::invalid_request(
                "this repo does not permit commits without a parent",
            )
            .into());
        }

        Ok(parents)
    }

    async fn convert_create_commit_file_content(
        repo: &RepoContext<Repo>,
        content: thrift::RepoCreateCommitParamsFileContent,
    ) -> Result<CreateChangeFileContents, scs_errors::ServiceError> {
        let contents = match content {
            thrift::RepoCreateCommitParamsFileContent::id(id) => {
                let file_id = FileId::from_request(&id)?;
                let file = repo
                    .file(file_id)
                    .await?
                    .ok_or_else(|| scs_errors::file_not_found(file_id.to_string()))?;
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
                    .ok_or_else(|| scs_errors::file_not_found(sha.to_string()))?;
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
                    .ok_or_else(|| scs_errors::file_not_found(sha.to_string()))?;
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
                    .ok_or_else(|| scs_errors::file_not_found(sha.to_string()))?;
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
                return Err(scs_errors::invalid_request(format!(
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
    ) -> Result<CreateChange, scs_errors::ServiceError> {
        let change = match change {
            thrift::RepoCreateCommitParamsChange::changed(c) => {
                if c.r#type == thrift::RepoCreateCommitParamsFileType::GIT_SUBMODULE
                    && repo.config().default_commit_identity_scheme == CommitIdentityScheme::HG
                {
                    return Err(scs_errors::invalid_request(
                        "cannot create git submodule in hg repo",
                    )
                    .into());
                }

                if c.git_lfs.is_some()
                    && repo.config().default_commit_identity_scheme == CommitIdentityScheme::HG
                {
                    return Err(scs_errors::invalid_request(
                        "cannot create git lfs file in hg repo",
                    )
                    .into());
                }

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
                            return Err(scs_errors::invalid_request(format!(
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
                return Err(scs_errors::invalid_request(format!(
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
    ) -> Result<BTreeMap<MPath, CreateChange>, scs_errors::ServiceError> {
        let changes = changes
            .into_iter()
            .map(|(path, change)| async move {
                let path = MPath::try_from(&path).map_err(|e| {
                    scs_errors::invalid_request(format!("invalid path '{}': {}", path, e))
                })?;
                let change = Self::convert_create_commit_change(repo, change).await?;
                Ok::<_, scs_errors::ServiceError>((path, change))
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
    ) -> Result<thrift::RepoCreateCommitResponse, scs_errors::ServiceError> {
        let repo = self
            .repo_for_service(ctx, &repo, params.service_identity.clone())
            .await?;

        let parents = Self::convert_create_commit_parents(&repo, &params.parents).await?;
        let mut info = CreateInfo::from_request(&params.info)?;
        let changes = Self::convert_create_commit_changes(&repo, params.changes).await?;
        let bubble = None;

        // For git, we need the committer info to be set - we'll copy the
        // author info.
        if repo.config().default_commit_identity_scheme == CommitIdentityScheme::GIT {
            if info.committer.is_none() {
                info.committer = Some(info.author.clone());
            }
            if info.committer_date.is_none() {
                info.committer_date = Some(MononokeDateTime::now().into());
            }
        }

        let created_changeset = repo
            .create_changeset(
                parents,
                info,
                changes,
                bubble,
                CreateChangesetChecks::from_request(&params.checks)?,
            )
            .await?;

        // If you ask for a git identity back, then we'll assume that you supplied one to us
        // and set it. Later, when we can derive a git commit hash, this'll become more
        // open, because we'll only do the check if you ask for a hash different to the
        // one we would derive
        if params
            .identity_schemes
            .contains(&thrift::CommitIdentityScheme::GIT)
        {
            repo.set_git_mapping_from_changeset(
                &created_changeset.changeset_ctx,
                &created_changeset.hg_extras,
            )
            .await?;
        }
        let ids =
            map_commit_identity(&created_changeset.changeset_ctx, &params.identity_schemes).await?;
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
    ) -> Result<thrift::RepoCreateStackResponse, scs_errors::ServiceError> {
        let batch_size = params.commits.len() as u64;
        let repo = self
            .repo_for_service(ctx.clone(), &repo, params.service_identity.clone())
            .await?;
        let repo = &repo;

        let stack_parents = Self::convert_create_commit_parents(repo, &params.parents).await?;
        let mut info_stack = params
            .commits
            .iter()
            .map(|commit| CreateInfo::from_request(&commit.info))
            .collect::<Result<Vec<_>, _>>()?;

        // For git, we need the committer info to be set - we'll copy the
        // author info.
        if repo.config().default_commit_identity_scheme == CommitIdentityScheme::GIT {
            for info in info_stack.iter_mut() {
                if info.committer.is_none() {
                    info.committer = Some(info.author.clone());
                }
                if info.committer_date.is_none() {
                    info.committer_date = Some(MononokeDateTime::now().into());
                }
            }
        }

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
            .create_changeset_stack(
                stack_parents,
                info_stack,
                changes_stack,
                bubble,
                CreateChangesetChecks::from_request(&params.checks)?,
            )
            .await?;
        // If you ask for a git identity back, then we'll assume that you supplied one to us
        // and set it. Later, when we can derive a git commit hash, this'll become more
        // open, because we'll only do the check if you ask for a hash different to the
        // one we would derive
        if params
            .identity_schemes
            .contains(&thrift::CommitIdentityScheme::GIT)
        {
            for created_changeset in stack.iter() {
                repo.set_git_mapping_from_changeset(
                    &created_changeset.changeset_ctx,
                    &created_changeset.hg_extras,
                )
                .await?;
            }
        }

        if let Some(prepare_types) = &params.prepare_derived_data_types {
            let csids = stack
                .iter()
                .map(|created_changeset| created_changeset.changeset_ctx.id())
                .collect::<Vec<_>>();
            let derived_data_types = prepare_types
                .iter()
                .map(DerivableType::from_request)
                .collect::<Result<Vec<_>, _>>()?;
            repo.derive_bulk_locally(&ctx, csids, &derived_data_types, Some(batch_size))
                .await?;
        }

        let identity_schemes = &params.identity_schemes;
        let commit_ids = stream::iter(stack.into_iter().map(|created_changeset| async move {
            map_commit_identity(&created_changeset.changeset_ctx, identity_schemes).await
        }))
        .buffered(10)
        .try_collect::<Vec<_>>()
        .await?;
        Ok(thrift::RepoCreateStackResponse {
            commit_ids,
            ..Default::default()
        })
    }
}
