/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use context::CoreContext;
use futures_util::future;
use mononoke_api::{ChangesetSpecifier, MononokeError, PathEntry};
use source_control as thrift;
use std::collections::{BTreeSet, HashMap};
use std::iter::FromIterator;

use crate::commit_id::map_commit_identities;
use crate::errors;

use crate::into_response::IntoResponse;

use crate::source_control_impl::SourceControlServiceImpl;

impl SourceControlServiceImpl {
    /// Returns information about the file or directory at a path in a commit.
    pub(crate) async fn commit_path_info(
        &self,
        ctx: CoreContext,
        commit_path: thrift::CommitPathSpecifier,
        _params: thrift::CommitPathInfoParams,
    ) -> Result<thrift::CommitPathInfoResponse, errors::ServiceError> {
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

    pub(crate) async fn commit_path_blame(
        &self,
        ctx: CoreContext,
        commit_path: thrift::CommitPathSpecifier,
        params: thrift::CommitPathBlameParams,
    ) -> Result<thrift::CommitPathBlameResponse, errors::ServiceError> {
        match params.format {
            thrift::BlameFormat::VERBOSE => {
                self.commit_path_blame_verbose(ctx, commit_path, params)
                    .await
            }
            other_format => Err(errors::invalid_request(format!(
                "unsupported blame format {}",
                other_format
            ))
            .into()),
        }
    }

    async fn commit_path_blame_verbose(
        &self,
        ctx: CoreContext,
        commit_path: thrift::CommitPathSpecifier,
        params: thrift::CommitPathBlameParams,
    ) -> Result<thrift::CommitPathBlameResponse, errors::ServiceError> {
        let (repo, changeset) = self.repo_changeset(ctx, &commit_path.commit).await?;
        let path = changeset.path(&commit_path.path)?;

        let (content, blame) = path.blame().await?;
        let csids: Vec<_> = blame.ranges().iter().map(|range| range.csid).collect();
        let identities = map_commit_identities(
            &repo,
            csids.clone(),
            &BTreeSet::from_iter(params.identity_scheme),
        )
        .await?;

        // author and date fields
        let info: HashMap<_, _> = future::try_join_all(csids.into_iter().map(move |csid| {
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
                        .and_then(|ids| {
                            ids.get(
                                &params
                                    .identity_scheme
                                    .unwrap_or(thrift::CommitIdentityScheme::BONSAI),
                            )
                        })
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
}
