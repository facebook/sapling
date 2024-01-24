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
use mononoke_api::ChangesetId;
use mononoke_api_hg::HgRepoContext;
use mononoke_types::blame_v2::BlameV2;

use super::handler::EdenApiContext;
use super::EdenApiHandler;
use super::EdenApiMethod;
use super::HandlerResult;
use crate::errors::ErrorKind;
use crate::utils::to_hg_path;
use crate::utils::to_mpath;

// I don't expect big blame requests, so let's keep this low.
const MAX_CONCURRENT_BLAMES_PER_REQUEST: usize = 10;

pub struct BlameHandler;

#[async_trait]
impl EdenApiHandler for BlameHandler {
    type Request = BlameRequest;
    type Response = BlameResult;

    const HTTP_METHOD: hyper::Method = hyper::Method::POST;
    const API_METHOD: EdenApiMethod = EdenApiMethod::Blame;
    const ENDPOINT: &'static str = "/blame";

    fn sampling_rate(_request: &Self::Request) -> NonZeroU64 {
        nonzero_ext::nonzero!(100u64)
    }

    async fn handler(
        ectx: EdenApiContext<Self::PathExtractor, Self::QueryStringExtractor>,
        request: Self::Request,
    ) -> HandlerResult<'async_trait, Self::Response> {
        let repo = ectx.repo();
        let blames = request
            .files
            .into_iter()
            .map(move |key| blame_file(repo.clone(), key));

        Ok(stream::iter(blames)
            .buffer_unordered(MAX_CONCURRENT_BLAMES_PER_REQUEST)
            .boxed())
    }
}

async fn blame_file(repo: HgRepoContext, key: Key) -> Result<BlameResult> {
    Ok(BlameResult {
        file: key.clone(),
        data: blame_file_data(repo, key.clone())
            .await
            .map_err(|e| ServerError::generic(format!("{:?}", e))),
    })
}

async fn blame_file_data(repo: HgRepoContext, key: Key) -> Result<BlameData> {
    let repo = repo.repo();

    let cs = repo
        .changeset(key.hgid)
        .await
        .context("failed to resolve blame hgid")?
        .ok_or(ErrorKind::HgIdNotFound(key.hgid))?;

    let disable_mutable_blame: bool = justknobs::eval(
        "scm/mononoke:edenapi_disable_mutable_blame",
        None,
        Some(repo.name()),
    )
    .unwrap_or(false);

    let blame = cs
        .path_with_history(to_mpath(&key.path)?.context(ErrorKind::UnexpectedEmptyPath)?)
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

    // Convert to hg csid, maintaining order in csids.
    let mut to_hg: HashMap<_, _> = repo
        .many_changeset_hg_ids(csids.clone())
        .await?
        .into_iter()
        .collect();
    let hg_csids = csids
        .iter()
        .map(|csid| {
            to_hg
                .remove(csid)
                .map(Into::into)
                .ok_or_else(|| anyhow!("no hg mapping for blame csid {:?}", csid))
        })
        .collect::<Result<Vec<_>>>()?;

    Ok(BlameData {
        line_ranges: ranges,
        commits: hg_csids,
        paths,
    })
}
