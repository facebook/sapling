/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{Context, Error};
use futures::{stream, Stream, StreamExt, TryStreamExt};
use gotham::state::{FromState, State};
use gotham_derive::{StateData, StaticResponseExtender};
use serde::Deserialize;
use std::str::FromStr;

use edenapi_types::{
    wire::{ToWire, WireFileRequest},
    AnyFileContentId, AnyId, FileEntry, FileRequest, UploadToken,
};
use gotham_ext::{error::HttpError, response::TryIntoResponse};
use load_limiter::Metric;
use mercurial_types::{HgFileNodeId, HgNodeHash};
use mononoke_api_hg::{HgDataContext, HgDataId, HgRepoContext};
use types::Key;

use crate::context::ServerContext;
use crate::errors::ErrorKind;
use crate::middleware::RequestContext;
use crate::utils::{cbor_stream, get_repo, parse_wire_request};

use super::{EdenApiMethod, HandlerInfo};

/// XXX: This number was chosen arbitrarily.
const MAX_CONCURRENT_FILE_FETCHES_PER_REQUEST: usize = 10;

#[derive(Debug, Deserialize, StateData, StaticResponseExtender)]
pub struct FileParams {
    repo: String,
}

#[derive(Debug, Deserialize, StateData, StaticResponseExtender)]
pub struct UploadFileParams {
    repo: String,
    idtype: String,
    id: String,
}

/// Fetch the content of the files requested by the client.
pub async fn files(state: &mut State) -> Result<impl TryIntoResponse, HttpError> {
    let params = FileParams::take_from(state);

    state.put(HandlerInfo::new(&params.repo, EdenApiMethod::Files));

    let rctx = RequestContext::borrow_from(state).clone();
    let sctx = ServerContext::borrow_from(state);

    let repo = get_repo(&sctx, &rctx, &params.repo, Metric::EgressGetpackFiles).await?;
    let request = parse_wire_request::<WireFileRequest>(state).await?;

    Ok(cbor_stream(
        fetch_all_files(repo, request).map(|r| r.map(|v| v.to_wire())),
    ))
}

/// Fetch files for all of the requested keys concurrently.
fn fetch_all_files(
    repo: HgRepoContext,
    request: FileRequest,
) -> impl Stream<Item = Result<FileEntry, Error>> {
    let ctx = repo.ctx().clone();

    let fetches = request
        .keys
        .into_iter()
        .map(move |key| fetch_file(repo.clone(), key));

    stream::iter(fetches)
        .buffer_unordered(MAX_CONCURRENT_FILE_FETCHES_PER_REQUEST)
        .inspect_ok(move |_| {
            ctx.session().bump_load(Metric::EgressGetpackFiles, 1.0);
        })
}

/// Fetch requested file for a single key.
/// Note that this function consumes the repo context in order
/// to construct a file context for the requested blob.
async fn fetch_file(repo: HgRepoContext, key: Key) -> Result<FileEntry, Error> {
    let id = HgFileNodeId::from_node_hash(HgNodeHash::from(key.hgid));

    let ctx = id
        .context(repo)
        .await
        .with_context(|| ErrorKind::FileFetchFailed(key.clone()))?
        .with_context(|| ErrorKind::KeyDoesNotExist(key.clone()))?;

    let (data, metadata) = ctx
        .content()
        .await
        .with_context(|| ErrorKind::FileFetchFailed(key.clone()))?;
    let parents = ctx.hg_parents().into();

    Ok(FileEntry::new(key, data, parents, metadata))
}

/// Generate an upload token for alredy uploaded content
async fn generate_upload_token(
    _repo: HgRepoContext,
    id: AnyFileContentId,
) -> Result<UploadToken, Error> {
    // At first, returns a fake token
    Ok(UploadToken::new_fake_token(AnyId::AnyFileContentId(id)))
}

/// Upload content of a file requested by the client.
pub async fn upload_file(state: &mut State) -> Result<impl TryIntoResponse, HttpError> {
    let params = UploadFileParams::take_from(state);

    state.put(HandlerInfo::new(&params.repo, EdenApiMethod::UploadFile));

    let rctx = RequestContext::borrow_from(state).clone();
    let sctx = ServerContext::borrow_from(state);

    let repo = get_repo(&sctx, &rctx, &params.repo, Metric::EgressGetpackFiles).await?;

    let id = AnyFileContentId::from_str(&format!("{}/{}", &params.idtype, &params.id))
        .map_err(HttpError::e400)?;

    // TODO: implement uploading logic itself
    // await for upload here

    let token = generate_upload_token(repo, id).await.map(|v| v.to_wire());

    Ok(cbor_stream(stream::iter(vec![token])))
}
