/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use futures::{stream, StreamExt};
use gotham::state::{FromState, State};
use gotham_derive::{StateData, StaticResponseExtender};
use serde::Deserialize;

use edenapi_types::{
    wire::{ToWire, WireBatch, WireLookupRequest},
    AnyFileContentId, AnyId, LookupRequest, LookupResponse, UploadToken,
};
use gotham_ext::{error::HttpError, response::TryIntoResponse};
use load_limiter::Metric;
use mercurial_types::{HgChangesetId, HgFileNodeId, HgNodeHash};
use mononoke_api_hg::HgDataId;
use mononoke_api_hg::HgRepoContext;

use crate::context::ServerContext;
use crate::middleware::RequestContext;
use crate::utils::{cbor_stream, get_repo, parse_wire_request};

use super::{EdenApiMethod, HandlerInfo};

const MAX_CONCURRENT_LOOKUPS_PER_REQUEST: usize = 10000;

#[derive(Debug, Deserialize, StateData, StaticResponseExtender)]
pub struct LookupParams {
    repo: String,
}

/// Check if the item is present already and generate a token if it is.
/// Return None if the item has to be uploaded
/// Item can be of any type:
///     * file content id (canonical and sha1, sha256 aliases)
///     * hg filenode id
///     * hg tree id
///     * hg changeset id
async fn check_request_item(
    repo: HgRepoContext,
    item: LookupRequest,
    index: usize,
) -> Result<LookupResponse, Error> {
    let is_present = match item.id {
        AnyId::AnyFileContentId(id) => match id {
            AnyFileContentId::ContentId(id) => {
                repo.is_present(mononoke_types::ContentId::from(id)).await?
            }
            AnyFileContentId::Sha1(id) => {
                repo.is_present_sha1(mononoke_types::hash::Sha1::from(id))
                    .await?
            }
            AnyFileContentId::Sha256(id) => {
                repo.is_present_sha256(mononoke_types::hash::Sha256::from(id))
                    .await?
            }
        },
        AnyId::HgFilenodeId(id) => {
            let _file_node_id = HgFileNodeId::from_node_hash(HgNodeHash::from(id));
            unimplemented!()
        }
        AnyId::HgTreeId(_id) => unimplemented!(),
        AnyId::HgChangesetId(id) => {
            repo.changeset_exists(HgChangesetId::new(HgNodeHash::from(id)))
                .await?
        }
    };

    Ok(LookupResponse {
        index,
        token: if is_present {
            Some(UploadToken::new_fake_token(item.id))
        } else {
            None
        },
    })
}

/// Process lookup (batched) request.
pub async fn lookup(state: &mut State) -> Result<impl TryIntoResponse, HttpError> {
    let params = LookupParams::take_from(state);

    state.put(HandlerInfo::new(&params.repo, EdenApiMethod::Lookup));

    let rctx = RequestContext::borrow_from(state).clone();
    let sctx = ServerContext::borrow_from(state);

    let repo = get_repo(&sctx, &rctx, &params.repo, Metric::EgressGetpackFiles).await?;
    let request = parse_wire_request::<WireBatch<WireLookupRequest>>(state).await?;

    let tokens = request
        .batch
        .into_iter()
        .enumerate()
        .map(move |(i, item)| check_request_item(repo.clone(), item, i));

    Ok(cbor_stream(
        stream::iter(tokens)
            .buffer_unordered(MAX_CONCURRENT_LOOKUPS_PER_REQUEST)
            .map(|r| r.map(|v| v.to_wire())),
    ))
}
