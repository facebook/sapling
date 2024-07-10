/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use bytes::Bytes;
use gotham::mime;
use gotham::state::FromState;
use gotham::state::State;
use gotham_ext::error::HttpError;
use gotham_ext::response::BytesBody;
use gotham_ext::response::TryIntoResponse;
use hyper::Body;
use hyper::Response;
use packetline::encode::flush_to_write;
use packetline::encode::write_text_packetline;
use protocol::pack_processor::parse_pack;

use crate::command::Command;
use crate::command::RequestCommand;
use crate::model::GitMethodInfo;
use crate::model::RepositoryParams;
use crate::model::RepositoryRequestContext;
use crate::util::empty_body;
use crate::util::get_body;

const OK_HEADER: &[u8] = b"unpack ok";
const OK_PREFIX: &str = "ok";

pub async fn receive_pack(state: &mut State) -> Result<Response<Body>, HttpError> {
    let body_bytes = get_body(state).await?;
    // We got a flush line packet to keep the connection alive. Just return Ok.
    if body_bytes == packetline::FLUSH_LINE {
        return empty_body(state);
    }
    let request_command =
        RequestCommand::parse_from_packetline(&body_bytes).map_err(HttpError::e400)?;
    push(state, request_command).await.map_err(HttpError::e500)
}

async fn push<'a>(
    state: &mut State,
    request_command: RequestCommand<'a>,
) -> anyhow::Result<Response<Body>> {
    let repo_name = RepositoryParams::borrow_from(state).repo_name();
    let request_context = RepositoryRequestContext::instantiate(
        state,
        GitMethodInfo::from_command(&request_command.command, repo_name),
    )
    .await?;
    // TODO(rajshar): Implement the actual push logic
    let mut output = vec![];
    if let Command::Push(push_args) = request_command.command {
        // Parse the packfile provided as part of the push and verify that its valid
        let _parsed_objects = parse_pack(
            push_args.pack_file,
            &request_context.ctx,
            request_context.repo.repo_blobstore.clone(),
        )
        .await?;
        write_text_packetline(OK_HEADER, &mut output).await?;
        for ref_update in push_args.ref_updates {
            write_text_packetline(
                format!("{} {}", OK_PREFIX, ref_update.ref_name).as_bytes(),
                &mut output,
            )
            .await?;
        }
        flush_to_write(&mut output).await?;
    }
    BytesBody::new(Bytes::from(output), mime::TEXT_PLAIN).try_into_response(state)
}
