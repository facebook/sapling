/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use std::cmp::min;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::convert::TryFrom;
use std::iter::FromIterator;
use std::sync::Arc;

use async_trait::async_trait;
use bytes::BufMut;
use context::generate_session_id;
use fbinit::FacebookInit;
use futures::stream::Stream;
use futures_preview::compat::Future01CompatExt;
use futures_util::{stream, try_future, try_join, StreamExt, TryStreamExt};
use mononoke_api::{
    unified_diff, ChangesetContext, ChangesetSpecifier, CopyInfo, CoreContext, FileContext, FileId,
    Mononoke, MononokeError, MononokePath, PathEntry, RepoContext, SessionContainer, TreeContext,
    TreeId,
};
use mononoke_types::hash::{Sha1, Sha256};
use scuba_ext::ScubaSampleBuilder;
use slog::Logger;
use source_control as thrift;
use source_control::server::SourceControlService;
use source_control::services::source_control_service as service;
use sshrelay::SshEnvVars;
use tracing::TraceContext;

use crate::commit_id::{map_commit_identities, map_commit_identity, CommitIdExt};
use crate::errors;
use crate::from_request::{check_range_and_convert, FromRequest};
use crate::into_response::{AsyncIntoResponse, IntoResponse};
use crate::specifiers::SpecifierExt;

// Magic number used when we want to limit concurrency with buffer_unordered.
const CONCURRENCY_LIMIT: usize = 100;

#[derive(Clone)]
pub struct SourceControlServiceImpl {
    fb: FacebookInit,
    mononoke: Arc<Mononoke>,
    logger: Logger,
    scuba_builder: ScubaSampleBuilder,
}

impl SourceControlServiceImpl {
    pub fn new(
        fb: FacebookInit,
        mononoke: Arc<Mononoke>,
        logger: Logger,
        scuba_builder: ScubaSampleBuilder,
    ) -> Self {
        Self {
            fb,
            mononoke,
            logger,
            scuba_builder,
        }
    }

    fn create_ctx(&self, specifier: Option<&dyn SpecifierExt>) -> CoreContext {
        let mut scuba = self.scuba_builder.clone();
        scuba.add_common_server_data().add("type", "thrift");
        if let Some(specifier) = specifier {
            if let Some(reponame) = specifier.scuba_reponame() {
                scuba.add("reponame", reponame);
            }
            if let Some(commit) = specifier.scuba_commit() {
                scuba.add("commit", commit);
            }
            if let Some(path) = specifier.scuba_path() {
                scuba.add("path", path);
            }
        }
        let session_id = generate_session_id();
        scuba.add("session_uuid", session_id.to_string());

        let session = SessionContainer::new(
            self.fb,
            session_id,
            TraceContext::default(),
            None,
            None,
            SshEnvVars::default(),
            None,
        );

        session.new_context(self.logger.clone(), scuba)
    }

    /// Get the repo specified by a `thrift::RepoSpecifier`.
    fn repo(
        &self,
        ctx: CoreContext,
        repo: &thrift::RepoSpecifier,
    ) -> Result<RepoContext, errors::ServiceError> {
        let repo = self
            .mononoke
            .repo(ctx, &repo.name)?
            .ok_or_else(|| errors::repo_not_found(repo.description()))?;
        Ok(repo)
    }

    /// Get the repo and changeset specified by a `thrift::CommitSpecifier`.
    async fn repo_changeset(
        &self,
        ctx: CoreContext,
        commit: &thrift::CommitSpecifier,
    ) -> Result<(RepoContext, ChangesetContext), errors::ServiceError> {
        let repo = self.repo(ctx, &commit.repo)?;
        let changeset_specifier = ChangesetSpecifier::from_request(&commit.id)?;
        let changeset = repo
            .changeset(changeset_specifier)
            .await?
            .ok_or_else(|| errors::commit_not_found(commit.description()))?;
        Ok((repo, changeset))
    }

    /// Get the repo and tree specified by a `thrift::TreeSpecifier`.
    ///
    /// Returns `None` if the tree is specified by commit path and that path
    /// is not a directory in that commit.
    async fn repo_tree(
        &self,
        ctx: CoreContext,
        tree: &thrift::TreeSpecifier,
    ) -> Result<(RepoContext, Option<TreeContext>), errors::ServiceError> {
        let (repo, tree) = match tree {
            thrift::TreeSpecifier::by_commit_path(commit_path) => {
                let (repo, changeset) = self.repo_changeset(ctx, &commit_path.commit).await?;
                let path = changeset.path(&commit_path.path)?;
                (repo, path.tree().await?)
            }
            thrift::TreeSpecifier::by_id(tree_id) => {
                let repo = self.repo(ctx, &tree_id.repo)?;
                let tree_id = TreeId::from_request(&tree_id.id)?;
                let tree = repo
                    .tree(tree_id)
                    .await?
                    .ok_or_else(|| errors::tree_not_found(tree.description()))?;
                (repo, Some(tree))
            }
            thrift::TreeSpecifier::UnknownField(id) => {
                return Err(errors::invalid_request(format!(
                    "tree specifier type not supported: {}",
                    id
                ))
                .into());
            }
        };
        Ok((repo, tree))
    }

    /// Get the repo and file specified by a `thrift::FileSpecifier`.
    ///
    /// Returns `None` if the file is specified by commit path, and that path
    /// is not a file in that commit.
    async fn repo_file(
        &self,
        ctx: CoreContext,
        file: &thrift::FileSpecifier,
    ) -> Result<(RepoContext, Option<FileContext>), errors::ServiceError> {
        let (repo, file) = match file {
            thrift::FileSpecifier::by_commit_path(commit_path) => {
                let (repo, changeset) = self.repo_changeset(ctx, &commit_path.commit).await?;
                let path = changeset.path(&commit_path.path)?;
                (repo, path.file().await?)
            }
            thrift::FileSpecifier::by_id(file_id) => {
                let repo = self.repo(ctx, &file_id.repo)?;
                let file_id = FileId::from_request(&file_id.id)?;
                let file = repo
                    .file(file_id)
                    .await?
                    .ok_or_else(|| errors::file_not_found(file.description()))?;
                (repo, Some(file))
            }
            thrift::FileSpecifier::by_sha1_content_hash(hash) => {
                let repo = self.repo(ctx, &hash.repo)?;
                let file_sha1 = Sha1::from_request(&hash.content_hash)?;
                let file = repo
                    .file_by_content_sha1(file_sha1)
                    .await?
                    .ok_or_else(|| errors::file_not_found(file.description()))?;
                (repo, Some(file))
            }
            thrift::FileSpecifier::by_sha256_content_hash(hash) => {
                let repo = self.repo(ctx, &hash.repo)?;
                let file_sha256 = Sha256::from_request(&hash.content_hash)?;
                let file = repo
                    .file_by_content_sha256(file_sha256)
                    .await?
                    .ok_or_else(|| errors::file_not_found(file.description()))?;
                (repo, Some(file))
            }
            thrift::FileSpecifier::UnknownField(id) => {
                return Err(errors::invalid_request(format!(
                    "file specifier type not supported: {}",
                    id
                ))
                .into());
            }
        };
        Ok((repo, file))
    }
}

#[async_trait]
impl SourceControlService for SourceControlServiceImpl {
    async fn list_repos(
        &self,
        _params: thrift::ListReposParams,
    ) -> Result<Vec<thrift::Repo>, service::ListReposExn> {
        let _ctx = self.create_ctx(None);
        let mut repo_names: Vec<_> = self
            .mononoke
            .repo_names()
            .map(|repo_name| repo_name.to_string())
            .collect();
        repo_names.sort();
        let rsp = repo_names
            .into_iter()
            .map(|repo_name| thrift::Repo { name: repo_name })
            .collect();
        Ok(rsp)
    }

    /// Resolve a bookmark to a changeset.
    ///
    /// Returns whether the bookmark exists, and the IDs of the changeset in
    /// the requested indentity schemes.
    async fn repo_resolve_bookmark(
        &self,
        repo: thrift::RepoSpecifier,
        params: thrift::RepoResolveBookmarkParams,
    ) -> Result<thrift::RepoResolveBookmarkResponse, service::RepoResolveBookmarkExn> {
        let ctx = self.create_ctx(Some(&repo));
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
    async fn repo_list_bookmarks(
        &self,
        repo: thrift::RepoSpecifier,
        params: thrift::RepoListBookmarksParams,
    ) -> Result<thrift::RepoListBookmarksResponse, service::RepoListBookmarksExn> {
        let ctx = self.create_ctx(Some(&repo));
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

    /// Look up commit.
    async fn commit_lookup(
        &self,
        commit: thrift::CommitSpecifier,
        params: thrift::CommitLookupParams,
    ) -> Result<thrift::CommitLookupResponse, service::CommitLookupExn> {
        let ctx = self.create_ctx(Some(&commit));
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
    async fn commit_file_diffs(
        &self,
        commit: thrift::CommitSpecifier,
        params: thrift::CommitFileDiffsParams,
    ) -> Result<thrift::CommitFileDiffsResponse, service::CommitFileDiffsExn> {
        let ctx = self.create_ctx(Some(&commit));
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
        let total_input_size: u64 = try_future::try_join_all(flat_paths.map(|path| {
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

        let path_diffs = try_future::try_join_all(paths.into_iter().map(
            |(base_path, other_path, copy_info)| {
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
            },
        ))
        .await?;
        Ok(thrift::CommitFileDiffsResponse { path_diffs })
    }

    /// Get commit info.
    async fn commit_info(
        &self,
        commit: thrift::CommitSpecifier,
        params: thrift::CommitInfoParams,
    ) -> Result<thrift::CommitInfo, service::CommitInfoExn> {
        let ctx = self.create_ctx(Some(&commit));
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
    async fn commit_is_ancestor_of(
        &self,
        commit: thrift::CommitSpecifier,
        params: thrift::CommitIsAncestorOfParams,
    ) -> Result<bool, service::CommitIsAncestorOfExn> {
        let ctx = self.create_ctx(Some(&commit));
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
    async fn commit_compare(
        &self,
        commit: thrift::CommitSpecifier,
        params: thrift::CommitCompareParams,
    ) -> Result<thrift::CommitCompareResponse, service::CommitCompareExn> {
        let ctx = self.create_ctx(Some(&commit));
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
        let diff = base_changeset
            .diff(other_changeset_id, !params.skip_copies_renames)
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
    async fn commit_find_files(
        &self,
        commit: thrift::CommitSpecifier,
        params: thrift::CommitFindFilesParams,
    ) -> Result<thrift::CommitFindFilesResponse, service::CommitFindFilesExn> {
        let ctx = self.create_ctx(Some(&commit));
        let (_repo, changeset) = self.repo_changeset(ctx, &commit).await?;
        let limit: u64 = check_range_and_convert(
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

        let files = changeset
            .find_files(prefixes, params.basenames, limit)
            .await?
            .map(|path| path.to_string())
            .collect()
            .compat()
            .await?;
        Ok(thrift::CommitFindFilesResponse { files })
    }

    /// Returns information about the file or directory at a path in a commit.
    async fn commit_path_info(
        &self,
        commit_path: thrift::CommitPathSpecifier,
        _params: thrift::CommitPathInfoParams,
    ) -> Result<thrift::CommitPathInfoResponse, service::CommitPathInfoExn> {
        let ctx = self.create_ctx(Some(&commit_path));
        let (_repo, changeset) = self.repo_changeset(ctx, &commit_path.commit).await?;
        let path = changeset.path(&commit_path.path)?;
        let response = match path.entry().await? {
            PathEntry::NotPresent => thrift::CommitPathInfoResponse {
                exists: false,
                type_: None,
                info: None,
            },
            PathEntry::Tree(tree) => {
                let summary = tree.summary().await?;
                let tree_info = thrift::TreeInfo {
                    id: tree.id().as_ref().to_vec(),
                    simple_format_sha1: summary.simple_format_sha1.as_ref().to_vec(),
                    simple_format_sha256: summary.simple_format_sha256.as_ref().to_vec(),
                    child_files_count: summary.child_files_count as i64,
                    child_files_total_size: summary.child_files_total_size as i64,
                    child_dirs_count: summary.child_dirs_count as i64,
                    descendant_files_count: summary.descendant_files_count as i64,
                    descendant_files_total_size: summary.descendant_files_total_size as i64,
                };
                thrift::CommitPathInfoResponse {
                    exists: true,
                    type_: Some(thrift::EntryType::TREE),
                    info: Some(thrift::EntryInfo::tree(tree_info)),
                }
            }
            PathEntry::File(file, file_type) => {
                let metadata = file.metadata().await?;
                let file_info = thrift::FileInfo {
                    id: metadata.content_id.as_ref().to_vec(),
                    file_size: metadata.total_size as i64,
                    content_sha1: metadata.sha1.as_ref().to_vec(),
                    content_sha256: metadata.sha256.as_ref().to_vec(),
                };
                thrift::CommitPathInfoResponse {
                    exists: true,
                    type_: Some(file_type.into_response()),
                    info: Some(thrift::EntryInfo::file(file_info)),
                }
            }
        };
        Ok(response)
    }

    async fn commit_path_blame(
        &self,
        commit_path: thrift::CommitPathSpecifier,
        params: thrift::CommitPathBlameParams,
    ) -> Result<thrift::CommitPathBlameResponse, service::CommitPathBlameExn> {
        let ctx = self.create_ctx(Some(&commit_path));
        let (repo, changeset) = self.repo_changeset(ctx, &commit_path.commit).await?;
        let path = changeset.path(&commit_path.path)?;

        let (content, blame) = path.blame().await?;
        let csids: Vec<_> = blame.ranges().iter().map(|range| range.csid).collect();
        let identities = map_commit_identities(
            &repo,
            csids.clone(),
            &BTreeSet::from_iter(Some(params.identity_scheme)),
        )
        .await?;

        // author and date fields
        let info: HashMap<_, _> = try_future::try_join_all(csids.into_iter().map(move |csid| {
            let repo = repo.clone();
            async move {
                let changeset = repo
                    .changeset(ChangesetSpecifier::Bonsai(csid))
                    .await?
                    .ok_or_else(|| {
                        MononokeError::InvalidRequest(format!("failed to resolve commit: {}", csid))
                    })?;
                let date = changeset.author_date().await?;
                let date = thrift::DateTime {
                    timestamp: date.timestamp(),
                    tz: date.offset().local_minus_utc(),
                };
                let author = changeset.author().await?;
                Ok::<_, MononokeError>((csid, (author, date)))
            }
        }))
        .await?
        .into_iter()
        .collect();

        let lines = String::from_utf8_lossy(content.as_ref())
            .lines()
            .zip(blame.lines())
            .enumerate()
            .map(
                |(line, (contents, (csid, path)))| -> Result<_, thrift::RequestError> {
                    let commit = identities
                        .get(&csid)
                        .and_then(|ids| ids.get(&params.identity_scheme))
                        .ok_or_else(|| {
                            errors::commit_not_found(format!("failed to resolve commit: {}", csid))
                        })?;
                    let (author, date) = info.get(&csid).ok_or_else(|| {
                        errors::commit_not_found(format!("failed to resolve commit: {}", csid))
                    })?;
                    Ok(thrift::BlameVerboseLine {
                        line: (line + 1) as i32,
                        contents: contents.to_string(),
                        commit: commit.clone(),
                        path: path.to_string(),
                        author: author.clone(),
                        date: date.clone(),
                    })
                },
            )
            .collect::<Result<Vec<_>, _>>()?;
        let blame = thrift::BlameVerbose { lines };

        Ok(thrift::CommitPathBlameResponse {
            blame: thrift::Blame::blame_verbose(blame),
        })
    }

    /// List the contents of a directory.
    async fn tree_list(
        &self,
        tree: thrift::TreeSpecifier,
        params: thrift::TreeListParams,
    ) -> Result<thrift::TreeListResponse, service::TreeListExn> {
        let ctx = self.create_ctx(Some(&tree));
        let (_repo, tree) = self.repo_tree(ctx, &tree).await?;
        let offset: usize = check_range_and_convert("offset", params.offset, 0..)?;
        let limit: usize = check_range_and_convert(
            "limit",
            params.limit,
            0..=source_control::TREE_LIST_MAX_LIMIT,
        )?;
        if let Some(tree) = tree {
            let summary = tree.summary().await?;
            let entries = tree
                .list()
                .await?
                .skip(offset)
                .take(limit)
                .map(IntoResponse::into_response)
                .collect();
            let response = thrift::TreeListResponse {
                entries,
                count: (summary.child_files_count + summary.child_dirs_count) as i64,
            };
            Ok(response)
        } else {
            // Listing a path that is not a directory just returns an empty list.
            Ok(thrift::TreeListResponse {
                entries: Vec::new(),
                count: 0,
            })
        }
    }

    /// Test whether a file exists.
    async fn file_exists(
        &self,
        file: thrift::FileSpecifier,
        _params: thrift::FileExistsParams,
    ) -> Result<bool, service::FileExistsExn> {
        let ctx = self.create_ctx(Some(&file));
        let (_repo, file) = self.repo_file(ctx, &file).await?;
        Ok(file.is_some())
    }

    /// Get file info.
    async fn file_info(
        &self,
        file: thrift::FileSpecifier,
        _params: thrift::FileInfoParams,
    ) -> Result<thrift::FileInfo, service::FileInfoExn> {
        let ctx = self.create_ctx(Some(&file));
        match self.repo_file(ctx, &file).await? {
            (_repo, Some(file)) => Ok(file.metadata().await?.into_response()),
            (_repo, None) => Err(errors::file_not_found(file.description()).into()),
        }
    }

    /// Get a chunk of file content.
    async fn file_content_chunk(
        &self,
        file: thrift::FileSpecifier,
        params: thrift::FileContentChunkParams,
    ) -> Result<thrift::FileChunk, service::FileContentChunkExn> {
        let ctx = self.create_ctx(Some(&file));
        let offset: u64 = check_range_and_convert("offset", params.offset, 0..)?;
        let size: u64 = check_range_and_convert(
            "size",
            params.size,
            0..=source_control::FILE_CONTENT_CHUNK_SIZE_LIMIT,
        )?;
        match self.repo_file(ctx, &file).await? {
            (_repo, Some(file)) => {
                let metadata = file.metadata().await?;
                let expected_size = min(size, metadata.total_size.saturating_sub(offset));
                let mut data = Vec::with_capacity(expected_size as usize);
                file.content_range(offset, size)
                    .await
                    .for_each(|bytes| {
                        data.put(bytes);
                        Ok(())
                    })
                    .compat()
                    .await
                    .map_err(errors::internal_error)?;
                Ok(thrift::FileChunk {
                    offset: params.offset,
                    file_size: metadata.total_size as i64,
                    data,
                })
            }
            (_repo, None) => Err(errors::file_not_found(file.description()).into()),
        }
    }

    /// Do a cross-repo lookup to see if a commit exists under a different hash in another repo
    async fn commit_lookup_xrepo(
        &self,
        commit: thrift::CommitSpecifier,
        params: thrift::CommitLookupXRepoParams,
    ) -> Result<thrift::CommitLookupResponse, service::CommitLookupXrepoExn> {
        let ctx = self.create_ctx(Some(&commit));
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
