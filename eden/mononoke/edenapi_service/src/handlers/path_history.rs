/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;

use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;
use async_trait::async_trait;
use edenapi_types::PathHistoryEntries;
use edenapi_types::PathHistoryEntry;
use edenapi_types::PathHistoryRequest;
use edenapi_types::PathHistoryRequestPaginationCursor;
use edenapi_types::PathHistoryResponse;
use edenapi_types::RepoPathBuf;
use edenapi_types::ServerError;
use futures::StreamExt;
use futures::TryStreamExt;
use futures::stream;
use gotham_ext::handler::SlapiCommitIdentityScheme;
use mononoke_api::ChangesetContext;
use mononoke_api::ChangesetPathHistoryOptions;
use mononoke_api::MononokeRepo;
use mononoke_api::Repo;
use mononoke_api::RepoContext;
use mononoke_api_hg::HgRepoContext;
use mononoke_types::ChangesetId;
use mononoke_types::hash::GitSha1;
use types::HgId;

use super::HandlerResult;
use super::SaplingRemoteApiHandler;
use super::SaplingRemoteApiMethod;
use super::handler::SaplingRemoteApiContext;
use crate::errors::ErrorKind;
use crate::utils::to_mpath;

/// XXX: This number was chosen arbitrarily.
const MAX_CONCURRENT_FETCHES_PER_REQUEST: usize = 10;

pub struct PathHistoryHandler;

#[async_trait]
impl SaplingRemoteApiHandler for PathHistoryHandler {
    type Request = PathHistoryRequest;
    type Response = PathHistoryResponse;

    const HTTP_METHOD: hyper::Method = hyper::Method::POST;
    const API_METHOD: SaplingRemoteApiMethod = SaplingRemoteApiMethod::PathHistory;
    const ENDPOINT: &'static str = "/path_history";
    const SUPPORTED_FLAVOURS: &'static [SlapiCommitIdentityScheme] = &[
        SlapiCommitIdentityScheme::Hg,
        SlapiCommitIdentityScheme::Git,
    ];

    async fn handler(
        ectx: SaplingRemoteApiContext<Self::PathExtractor, Self::QueryStringExtractor, Repo>,
        request: Self::Request,
    ) -> HandlerResult<'async_trait, Self::Response> {
        let slapi_flavour = ectx.slapi_flavour().clone();
        let repo = ectx.repo();
        let PathHistoryRequest {
            commit,
            paths,
            limit,
            cursor,
        } = request;

        let mut path_to_cursor: HashMap<RepoPathBuf, Option<PathHistoryRequestPaginationCursor>> =
            paths.into_iter().map(|p| (p.clone(), None)).collect();
        for c in cursor {
            let v = path_to_cursor.get_mut(&c.path).ok_or_else(|| {
                anyhow!(
                    "Invalid cursor: path {:?} is not part of the original request",
                    c.path
                )
            })?;
            *v = Some(c);
        }

        let fetches = path_to_cursor.into_iter().map(move |(path, cursor)| {
            fetch_history_for_path_wrapper(repo.clone(), commit, path, limit, cursor, slapi_flavour)
        });

        Ok(stream::iter(fetches)
            .buffer_unordered(MAX_CONCURRENT_FETCHES_PER_REQUEST)
            .boxed())
    }
}

async fn fetch_history_for_path_wrapper<R: MononokeRepo>(
    repo: HgRepoContext<R>,
    commit: HgId,
    path: RepoPathBuf,
    limit: Option<u32>,
    cursor: Option<PathHistoryRequestPaginationCursor>,
    flavour: SlapiCommitIdentityScheme,
) -> Result<PathHistoryResponse> {
    Ok(PathHistoryResponse {
        path: path.clone(),
        entries: fetch_history_for_path(repo, commit, path, limit, cursor, flavour)
            .await
            .map_err(|e| ServerError::generic(format!("{:?}", e))),
    })
}

async fn fetch_history_for_path<R: MononokeRepo>(
    repo: HgRepoContext<R>,
    commit: HgId,
    path: RepoPathBuf,
    limit: Option<u32>,
    cursor: Option<PathHistoryRequestPaginationCursor>,
    flavour: SlapiCommitIdentityScheme,
) -> Result<PathHistoryEntries> {
    let repo = repo.repo_ctx();

    // TODO(lyang)
    //   1. handle cursor with renamed path
    //   2. handle non-linear history
    let starting_hgid = match cursor.and_then(|c| c.starting_commits.first().map(|t| t.0.clone())) {
        Some(c) => c,
        _ => commit,
    };
    let starting_cs = hgid_to_changeset(repo, flavour, starting_hgid).await?;

    let changesets = starting_cs
        .path_with_history(
            to_mpath(&path)?
                .into_optional_non_root_path()
                .context(ErrorKind::UnexpectedEmptyPath)?,
        )
        .await?
        .history(
            repo.ctx(),
            ChangesetPathHistoryOptions {
                follow_mutable_file_history: true,
                ..Default::default()
            },
        )
        .await?
        .try_collect::<Vec<_>>()
        .await?;

    let query_limit = match limit {
        // Take one more so we know whether there's more results after the current request
        Some(limit) => (limit as usize).saturating_add(1),
        None => u32::MAX as usize,
    };
    let csids = changesets
        .iter()
        .take(query_limit)
        .map(|cs| cs.id())
        .collect::<Vec<_>>();
    let hgids = csids_to_hgids(repo, flavour, csids).await?;

    let mut entries: Vec<_> = hgids
        .iter()
        .map(|hgid| PathHistoryEntry { commit: *hgid })
        .collect();
    let has_more = !hgids.is_empty() && hgids.len() == query_limit;
    let next_commits = if has_more {
        let last_entry = entries.pop().unwrap();
        vec![(last_entry.commit, None)]
    } else {
        vec![]
    };

    Ok(PathHistoryEntries {
        entries,
        has_more,
        next_commits,
    })
}

async fn hgid_to_changeset<R: MononokeRepo>(
    repo: &RepoContext<R>,
    flavour: SlapiCommitIdentityScheme,
    commit: HgId,
) -> Result<ChangesetContext<R>> {
    let cs = match flavour {
        SlapiCommitIdentityScheme::Git => repo
            .changeset(GitSha1::from_byte_array(commit.into_byte_array()))
            .await
            .context("Failed to resolve git hash")?
            .ok_or(ErrorKind::HgIdNotFound(commit))?,
        SlapiCommitIdentityScheme::Hg => repo
            .changeset(commit)
            .await
            .context("Failed to resolve hgid")?
            .ok_or(ErrorKind::HgIdNotFound(commit))?,
    };

    Ok(cs)
}

async fn csids_to_hgids<R: MononokeRepo>(
    repo: &RepoContext<R>,
    flavour: SlapiCommitIdentityScheme,
    csids: Vec<ChangesetId>,
) -> Result<Vec<HgId>> {
    let hgids = match flavour {
        SlapiCommitIdentityScheme::Git => {
            let mut to_id: HashMap<_, _> = repo
                .many_changeset_git_sha1s(csids.clone())
                .await?
                .into_iter()
                .collect();
            csids
                .iter()
                .map(|csid| {
                    to_id
                        .remove(csid)
                        .map(|git_sha1| HgId::from_byte_array(git_sha1.into_inner()))
                        .ok_or_else(|| anyhow!("No git mapping for csid {:?}", csid))
                })
                .collect::<Result<Vec<_>>>()?
        }
        SlapiCommitIdentityScheme::Hg => {
            let mut to_id: HashMap<_, _> = repo
                .many_changeset_hg_ids(csids.clone())
                .await?
                .into_iter()
                .collect();
            csids
                .iter()
                .map(|csid| {
                    to_id
                        .remove(csid)
                        .map(Into::into)
                        .ok_or_else(|| anyhow!("No hg mapping for csid {:?}", csid))
                })
                .collect::<Result<Vec<_>>>()?
        }
    };

    Ok(hgids)
}
