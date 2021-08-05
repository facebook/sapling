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
    AnyId, FileContentTokenMetadata, LookupRequest, LookupResponse, UploadToken,
    UploadTokenMetadata,
};
use gotham_ext::{error::HttpError, response::TryIntoResponse};
use mercurial_types::{HgChangesetId, HgFileNodeId, HgManifestId, HgNodeHash};
use mononoke_api_hg::{HgDataId, HgRepoContext};

use crate::context::ServerContext;
use crate::middleware::RequestContext;
use crate::utils::{cbor_stream_filtered_errors, get_repo, parse_wire_request};

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
    enum Lookup {
        NotPresent,
        Present(Option<UploadTokenMetadata>),
    }
    impl From<bool> for Lookup {
        fn from(b: bool) -> Self {
            if b {
                Self::Present(None)
            } else {
                Self::NotPresent
            }
        }
    }
    let lookup = match item.id {
        AnyId::AnyFileContentId(id) => {
            let content_id = repo.convert_file_to_content_id(id).await?;
            if let Some(content_id) = content_id {
                Lookup::Present(Some(
                    FileContentTokenMetadata {
                        content_size: repo.fetch_file_content_size(content_id).await?,
                    }
                    .into(),
                ))
            } else {
                Lookup::NotPresent
            }
        }
        AnyId::HgFilenodeId(id) => repo
            .filenode_exists(HgFileNodeId::from_node_hash(HgNodeHash::from(id)))
            .await?
            .into(),
        AnyId::HgTreeId(id) => repo
            .tree_exists(HgManifestId::new(HgNodeHash::from(id)))
            .await?
            .into(),
        AnyId::HgChangesetId(id) => repo
            .changeset_exists(HgChangesetId::new(HgNodeHash::from(id)))
            .await?
            .into(),
        AnyId::BonsaiChangesetId(id) => repo.changeset_exists_by_bonsai(id.into()).await?.into(),
    };

    Ok(LookupResponse {
        index,
        token: match lookup {
            Lookup::NotPresent => None,
            Lookup::Present(None) => Some(UploadToken::new_fake_token(item.id)),
            Lookup::Present(Some(metadata)) => {
                Some(UploadToken::new_fake_token_with_metadata(item.id, metadata))
            }
        },
    })
}

/// Process lookup (batched) request.
pub async fn lookup(state: &mut State) -> Result<impl TryIntoResponse, HttpError> {
    let params = LookupParams::take_from(state);

    state.put(HandlerInfo::new(&params.repo, EdenApiMethod::Lookup));

    let rctx = RequestContext::borrow_from(state).clone();
    let sctx = ServerContext::borrow_from(state);

    let repo = get_repo(&sctx, &rctx, &params.repo, None).await?;
    let request = parse_wire_request::<WireBatch<WireLookupRequest>>(state).await?;

    let tokens = request
        .batch
        .into_iter()
        .enumerate()
        .map(move |(i, item)| check_request_item(repo.clone(), item, i));

    Ok(cbor_stream_filtered_errors(
        stream::iter(tokens)
            .buffer_unordered(MAX_CONCURRENT_LOOKUPS_PER_REQUEST)
            .map(|r| r.map(|v| v.to_wire())),
    ))
}
