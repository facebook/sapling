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
use async_stream::try_stream;
use bytes::Bytes;
use futures::SinkExt;
use futures::StreamExt;
use gotham::mime;
use gotham::state::FromState;
use gotham::state::State;
use gotham_ext::body_ext::BodyExt;
use gotham_ext::error::HttpError;
use gotham_ext::response::BytesBody;
use gotham_ext::response::EmptyBody;
use gotham_ext::response::ResponseStream;
use gotham_ext::response::ResponseTryStreamExt;
use gotham_ext::response::StreamBody;
use gotham_ext::response::TryIntoResponse;
use http::HeaderMap;
use hyper::Body;
use hyper::Response;
use packetline::encode::flush_to_write;
use packetline::encode::write_binary_packetline;
use packetline::encode::write_data_channel;
use packetline::FLUSH_LINE;
use packfile::pack::DeltaForm;
use packfile::pack::PackfileWriter;
use protocol::generator::generate_pack_item_stream;
use protocol::generator::ls_refs_response;
use tokio::io::ErrorKind;
use tokio::sync::mpsc;
use tokio_util::io::CopyToBytes;
use tokio_util::io::SinkWriter;
use tokio_util::sync::PollSender;

use crate::command::Command;
use crate::command::FetchArgs;
use crate::command::LsRefsArgs;
use crate::command::RequestCommand;
use crate::model::RepositoryParams;
use crate::model::RepositoryRequestContext;
use crate::model::ResponseType;
use crate::model::ServiceType;
use crate::GitServerContext;

const PACKFILE_HEADER: &[u8] = b"packfile\n";

async fn get_body(state: &mut State) -> Result<Bytes, HttpError> {
    Body::take_from(state)
        .try_concat_body(&HeaderMap::new())
        .map_err(HttpError::e500)?
        .await
        .map_err(HttpError::e500)
}

pub async fn upload_pack(state: &mut State) -> Result<Response<Body>, HttpError> {
    let body_bytes = get_body(state).await?;
    // We got a flush line packet to keep the connection alive. Just return Ok.
    if body_bytes == packetline::FLUSH_LINE {
        return EmptyBody::new()
            .try_into_response(state)
            .map_err(HttpError::e500);
    }
    let request_command =
        RequestCommand::parse_from_packetline(&body_bytes).map_err(HttpError::e400)?;
    let repo_name = RepositoryParams::borrow_from(state).repo_name();
    let context = GitServerContext::borrow_from(state);
    let request_context = context.request_context(&repo_name)?;
    state.put(ServiceType::new("git-upload-pack".to_string()));
    state.put(ResponseType::new("result".to_string()));
    match request_command.command {
        Command::LsRefs(ls_refs_args) => {
            let output = ls_refs(&request_context, ls_refs_args).await;
            let output = output.map_err(HttpError::e500)?.try_into_response(state);
            output.map_err(HttpError::e500)
        }
        Command::Fetch(fetch_args) => {
            let output = fetch(&request_context, fetch_args).await;
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

/// Method responsible for generating the response to fetch command request
pub async fn fetch(
    request_context: &RepositoryRequestContext,
    args: FetchArgs,
) -> Result<impl TryIntoResponse, Error> {
    let (writer, reader) = mpsc::channel::<Bytes>(100_000_000);
    let sink_writer = SinkWriter::new(CopyToBytes::new(
        PollSender::new(writer).sink_map_err(|_| std::io::Error::from(ErrorKind::BrokenPipe)),
    ));

    let bytes_stream = ResponseStream::new(try_stream! {
        let mut pack_reader = tokio_stream::wrappers::ReceiverStream::new(reader).ready_chunks(100_000);
        // Write the header without specifying channel
        let chunk = Bytes::copy_from_slice(PACKFILE_HEADER);
        let mut buf = Vec::with_capacity(chunk.len());
        write_binary_packetline(chunk.as_ref(), &mut buf).await?;
        yield Bytes::from(buf);
        while let Some(chunks) = pack_reader.next().await {
            for chunk in chunks {
                let mut buf = Vec::with_capacity(chunk.len());
                // Write the actual packfile content to the data channel
                write_data_channel(chunk.as_ref(), &mut buf).await?;
                yield Bytes::from(buf);
            }
        }
        let mut buf = Vec::with_capacity(FLUSH_LINE.len());
        flush_to_write(&mut buf).await?;
        yield Bytes::from(buf);
    })
    .end_on_err::<anyhow::Error>();
    tokio::spawn({
        let request_context = request_context.clone();
        async move {
            let response_stream = generate_pack_item_stream(
                &request_context.ctx,
                &request_context.repo,
                args.into_request(),
            )
            .await?;
            let mut pack_writer = PackfileWriter::new(
                sink_writer,
                response_stream.num_items as u32,
                5000,
                DeltaForm::RefAndOffset,
            );
            pack_writer.write(response_stream.items).await?;
            pack_writer.finish().await?;
            anyhow::Ok(())
        }
    });

    let body = StreamBody::new(bytes_stream, mime::APPLICATION_OCTET_STREAM);
    Ok(body)
}
