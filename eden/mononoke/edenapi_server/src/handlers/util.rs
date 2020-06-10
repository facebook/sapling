/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{anyhow, Error};
use bytes::Bytes;
use gotham::state::{FromState, State};
use http::HeaderMap;
use hyper::Body;
use mime::Mime;
use once_cell::sync::Lazy;

use gotham_ext::{body_ext::BodyExt, error::HttpError};
use mononoke_api::{hg::HgRepoContext, path::MononokePath};
use mononoke_types::MPath;
use types::{RepoPath, RepoPathBuf};

use crate::context::ServerContext;
use crate::middleware::RequestContext;

static CBOR_MIME: Lazy<Mime> = Lazy::new(|| "application/cbor".parse().unwrap());

pub fn cbor_mime() -> Mime {
    CBOR_MIME.clone()
}

pub async fn get_repo(
    sctx: &ServerContext,
    rctx: &RequestContext,
    name: impl AsRef<str>,
) -> Result<HgRepoContext, HttpError> {
    let name = name.as_ref();
    sctx.mononoke_api()
        .repo(rctx.core_context().clone(), name)
        .await
        .map_err(HttpError::e403)?
        .map(|repo| repo.hg())
        .ok_or_else(|| HttpError::e404(anyhow!("repo does not exist: {:?}", name)))
}

pub async fn get_request_body(state: &mut State) -> Result<Bytes, HttpError> {
    let body = Body::take_from(state);
    let headers = HeaderMap::try_borrow_from(state);
    body.try_concat_body_opt(headers)
        .map_err(HttpError::e400)?
        .await
        .map_err(HttpError::e400)
}

pub fn to_mononoke_path(path: impl AsRef<RepoPath>) -> Result<MononokePath, Error> {
    let mpath = MPath::new_opt(path.as_ref().as_byte_slice())?;
    Ok(MononokePath::new(mpath))
}

pub fn to_hg_path(path: &MononokePath) -> Result<RepoPathBuf, Error> {
    Ok(match path.as_mpath() {
        Some(mpath) => RepoPathBuf::from_utf8(mpath.to_vec())?,
        None => RepoPathBuf::new(),
    })
}
