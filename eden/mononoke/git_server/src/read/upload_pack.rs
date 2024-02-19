/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

// use std::io::Read;
//  use std::str::FromStr;
// use std::io::Write;

use anyhow::Error;
use bytes::Bytes;
use gotham::mime;
use gotham::state::FromState;
use gotham::state::State;
use gotham_ext::body_ext::BodyExt;
use gotham_ext::error::HttpError;
use gotham_ext::response::BytesBody;
use gotham_ext::response::EmptyBody;
use gotham_ext::response::TryIntoResponse;
use http::HeaderMap;
use hyper::Body;
use hyper::Response;
use packetline::encode::flush_to_write;
use protocol::generator::ls_refs_response;

use crate::command::Command;
use crate::command::LsRefsArgs;
use crate::command::RequestCommand;
use crate::model::RepositoryParams;
use crate::model::RepositoryRequestContext;
use crate::model::ResponseType;
use crate::model::ServiceType;
use crate::GitServerContext;

pub async fn upload_pack(state: &mut State) -> Result<Response<Body>, HttpError> {
    let body_bytes = Body::take_from(state)
        .try_concat_body(&HeaderMap::new())
        .map_err(HttpError::e500)?
        .await
        .map_err(HttpError::e500)?;
    // We got a flush line packet to keep the connection alive. Just return Ok.
    if body_bytes == packetline::FLUSH_LINE {
        return EmptyBody::new()
            .try_into_response(state)
            .map_err(HttpError::e500);
    }
    let request_command = RequestCommand::parse_from_packetline(&body_bytes).map_err(|e| {
        eprintln!("Failed to parse request from body: {:?}", e);
        HttpError::e400(e)
    })?;
    let repo_name = RepositoryParams::borrow_from(state).repo_name();
    let context = GitServerContext::borrow_from(state);
    let request_context = context.request_context(&repo_name)?;
    println!(
        "Got request for service git-upload-pack with repo {}",
        repo_name
    );
    state.put(ServiceType::new("git-upload-pack".to_string()));
    state.put(ResponseType::new("result".to_string()));
    match request_command.command {
        Command::LsRefs(ls_refs_args) => {
            let output = ls_refs(&request_context, ls_refs_args).await;
            if let Err(e) = &output {
                eprintln!("Failed to generate ls-refs response: {:#}", e);
            }
            let output = output.map_err(HttpError::e500)?.try_into_response(state);
            output.map_err(HttpError::e500)
        }
    }
}

/// Method responsible for generating the response for ls_refs command request
pub async fn ls_refs(
    request_context: &RepositoryRequestContext,
    args: LsRefsArgs,
) -> Result<impl TryIntoResponse, Error> {
    let response = ls_refs_response(
        &request_context.ctx,
        &request_context.repo,
        args.into_request(),
    )
    .await?;
    let mut output = Vec::new();
    response.write_packetline(&mut output).await?;
    flush_to_write(&mut output).await?;
    Ok(BytesBody::new(Bytes::from(output), mime::TEXT_PLAIN))
}
