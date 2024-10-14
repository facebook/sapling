/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::hash_map::Entry::Occupied;
use std::collections::hash_map::Entry::Vacant;
use std::collections::HashMap;
use std::num::NonZeroU64;

use anyhow::anyhow;
use anyhow::bail;
use anyhow::Context;
use anyhow::Result;
use async_trait::async_trait;
use edenapi_types::BlameData;
use edenapi_types::BlameRequest;
use edenapi_types::BlameResult;
use edenapi_types::Key;
use edenapi_types::ServerError;
use futures::stream;
use futures::StreamExt;
use gotham_ext::handler::SlapiCommitIdentityScheme;
use mononoke_api::ChangesetId;
use mononoke_api::MononokeRepo;
use mononoke_api::Repo;
use mononoke_api_hg::HgRepoContext;
use mononoke_types::blame_v2::BlameV2;
use mononoke_types::hash::GitSha1;
use types::HgId;

use super::handler::SaplingRemoteApiContext;
use super::HandlerResult;
use super::SaplingRemoteApiHandler;
use super::SaplingRemoteApiMethod;
use crate::errors::ErrorKind;
use crate::utils::to_hg_path;
use crate::utils::to_mpath;

// I don't expect big blame requests, so let's keep this low.
const MAX_CONCURRENT_BLAMES_PER_REQUEST: usize = 10;

pub struct BlameHandler;

#[async_trait]
impl SaplingRemoteApiHandler for BlameHandler {
    type Request = BlameRequest;
    type Response = BlameResult;

    const HTTP_METHOD: hyper::Method = hyper::Method::POST;
    const API_METHOD: SaplingRemoteApiMethod = SaplingRemoteApiMethod::Blame;
    const ENDPOINT: &'static str = "/blame";
    const SUPPORTED_FLAVOURS: &'static [SlapiCommitIdentityScheme] = &[
        SlapiCommitIdentityScheme::Hg,
        SlapiCommitIdentityScheme::Git,
    ];

    fn sampling_rate(_request: &Self::Request) -> NonZeroU64 {
        nonzero_ext::nonzero!(100u64)
    }

    async fn handler(
        ectx: SaplingRemoteApiContext<Self::PathExtractor, Self::QueryStringExtractor, Repo>,
        request: Self::Request,
    ) -> HandlerResult<'async_trait, Self::Response> {
        let slapi_flavour = ectx.slapi_flavour().clone();
        let repo = ectx.repo();

        let blames = request
            .files
            .into_iter()
            .map(move |key| blame_file(repo.clone(), key, slapi_flavour));

        Ok(stream::iter(blames)
            .buffer_unordered(MAX_CONCURRENT_BLAMES_PER_REQUEST)
            .boxed())
    }
}

async fn blame_file<R: MononokeRepo>(
    repo: HgRepoContext<R>,
    key: Key,
    flavour: SlapiCommitIdentityScheme,
) -> Result<BlameResult> {
    Ok(BlameResult {
        file: key.clone(),
        data: blame_file_data(repo, key.clone(), flavour)
            .await
            .map_err(|e| ServerError::generic(format!("{:?}", e))),
    })
}

async fn blame_file_data<R: MononokeRepo>(
    repo: HgRepoContext<R>,
    key: Key,
    flavour: SlapiCommitIdentityScheme,
) -> Result<BlameData> {
    let repo = repo.repo_ctx();

    let cs = match flavour {
        SlapiCommitIdentityScheme::Git => repo
            .changeset(GitSha1::from_byte_array(key.hgid.into_byte_array()))
            .await
            .context("failed to resolve blame git hash")?
            .ok_or(ErrorKind::HgIdNotFound(key.hgid))?,
        SlapiCommitIdentityScheme::Hg => repo
            .changeset(key.hgid)
            .await
            .context("failed to resolve blame hgid")?
            .ok_or(ErrorKind::HgIdNotFound(key.hgid))?,
    };

    let disable_mutable_blame: bool = justknobs::eval(
        "scm/mononoke:edenapi_disable_mutable_blame",
        None,
        Some(repo.name()),
    )
    .unwrap_or(false);

    let blame = cs
        .path_with_history(
            to_mpath(&key.path)?
                .into_optional_non_root_path()
                .context(ErrorKind::UnexpectedEmptyPath)?,
        )
        .await?
        .blame(!disable_mutable_blame)
        .await?;

    let blame = match blame {
        BlameV2::Blame(blame) => blame,
        BlameV2::Rejected(rejected) => return Err(rejected.into()),
    };

    let old_csid_index = blame.csid_index();
    let mut csid_remap = HashMap::new();
    let mut csids: Vec<ChangesetId> = Vec::new();
    let ranges = blame
        .ranges()
        .iter()
        .map(|range| {
            let new_csid_idx = match csid_remap.entry(range.csid_index) {
                Occupied(entry) => *entry.get(),
                Vacant(vac) => {
                    let csid = match old_csid_index.get(range.csid_index as usize) {
                        Some(csid) => csid,
                        None => bail!("invalid blame range csid_index {}", range.csid_index),
                    };
                    csids.push(*csid);
                    *vac.insert(csids.len() - 1)
                }
            };
            Ok(edenapi_types::BlameLineRange {
                line_offset: range.offset,
                line_count: range.length,
                commit_index: new_csid_idx
                    .try_into()
                    .context("blame commit count overflows u32")?,
                path_index: range.path_index,
                origin_line_offset: range.origin_offset,
            })
        })
        .collect::<Result<Vec<_>>>()?;

    let paths = blame
        .paths()
        .iter()
        .map(to_hg_path)
        .collect::<Result<Vec<_>>>()?;

    // Convert to source control csid, maintaining order in csids.
    let sl_csids = match flavour {
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
                        .ok_or_else(|| anyhow!("no git mapping for blame csid {:?}", csid))
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
                        .ok_or_else(|| anyhow!("no hg mapping for blame csid {:?}", csid))
                })
                .collect::<Result<Vec<_>>>()?
        }
    };

    Ok(BlameData {
        line_ranges: ranges,
        commits: sl_csids,
        paths,
    })
}
