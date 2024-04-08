/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use anyhow::Context;
use anyhow::Error;
use async_stream::try_stream;
use bonsai_git_mapping::BonsaisOrGitShas;
use bytes::Bytes;
use futures::future::try_join4;
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
use packetline::encode::delim_to_write;
use packetline::encode::flush_to_write;
use packetline::encode::write_data_channel;
use packetline::encode::write_text_packetline;
use packetline::FLUSH_LINE;
use packfile::pack::DeltaForm;
use packfile::pack::PackfileWriter;
use protocol::generator::fetch_response;
use protocol::generator::ls_refs_response;
use protocol::generator::ref_oid_mapping;
use protocol::types::PackfileConcurrency;
use tokio::io::ErrorKind;
use tokio::sync::mpsc;
use tokio_util::io::CopyToBytes;
use tokio_util::io::SinkWriter;
use tokio_util::sync::PollSender;

use crate::command::Command;
use crate::command::FetchArgs;
use crate::command::LsRefsArgs;
use crate::command::RequestCommand;
use crate::model::GitMethod;
use crate::model::GitMethodInfo;
use crate::model::GitMethodVariant;
use crate::model::RepositoryParams;
use crate::model::RepositoryRequestContext;
use crate::model::ResponseType;
use crate::model::Service;

/// The header for the packfile section of the response
const PACKFILE_HEADER: &[u8] = b"packfile";
/// The header for the acknowledgements section of the response
const ACKNOWLEDGEMENTS_HEADER: &[u8] = b"acknowledgments";
/// The header for the shallow info section of the response
const WANTED_REFS_HEADER: &[u8] = b"wanted-refs";
/// Acknowledgement that the object sent by the client exists on the server
const ACK: &str = "ACK";
/// Acknowledgement that the object sent by the client does not exist on the server
const NAK: &[u8] = b"NAK";
/// Flag representing that the server is ready to send the required packfile to the client
const READY_FLAG: &[u8] = b"ready";

#[derive(Debug, Clone)]
struct FetchResponseHeaders {
    acknowledgements: Option<Bytes>,
    shallow_info: Option<Bytes>,
    wanted_refs: Option<Bytes>,
    packfile_uris: Option<Bytes>,
    pack_header: Option<Bytes>,
}

async fn pack_header() -> Result<Bytes, Error> {
    let mut buf = Vec::with_capacity(PACKFILE_HEADER.len());
    write_text_packetline(PACKFILE_HEADER, &mut buf).await?;
    Ok(Bytes::from(buf))
}

fn concurrency(context: &RepositoryRequestContext) -> PackfileConcurrency {
    match &context.repo.repo_config.git_concurrency {
        Some(concurrency) => PackfileConcurrency::new(
            concurrency.trees_and_blobs,
            concurrency.commits,
            concurrency.tags,
        ),
        None => PackfileConcurrency::standard(),
    }
}

async fn acknowledgements(
    context: Arc<RepositoryRequestContext>,
    args: Arc<FetchArgs>,
) -> Result<(Option<Bytes>, Option<Bytes>), Error> {
    if args.done {
        // The negotiation has already completed (or was not needed in the first place) so no
        // need to generate the acknowledgements section. We can straight away generate the
        // packfile based on the data sent by the client
        return Ok((None, Some(pack_header().await?)));
    }
    let git_shas = BonsaisOrGitShas::from_object_ids(args.haves.iter())?;
    let entries = context
        .repo
        .bonsai_git_mapping
        .get(&context.ctx, git_shas)
        .await
        .with_context(|| {
            format!(
                "Failed to fetch bonsai_git_mapping for repo {}",
                context.repo.name
            )
        })?;
    let mut output_buffer = vec![];
    let mut header = None;
    write_text_packetline(ACKNOWLEDGEMENTS_HEADER, &mut output_buffer).await?;
    if entries.is_empty() && !args.haves.is_empty() {
        // None of the Git Shas provided by the client are recognized by the server
        write_text_packetline(NAK, &mut output_buffer).await?;
    } else {
        // Provide an acknowledgement for the Git Shas that were recognized by the server
        for entry in entries {
            write_text_packetline(
                format!("{} {}", ACK, entry.git_sha1.to_hex()).as_bytes(),
                &mut output_buffer,
            )
            .await?;
        }
        // Provide the ready flag to indicate that we are ready to send the required
        // packfile to the client
        write_text_packetline(READY_FLAG, &mut output_buffer).await?;
        // Since we identified at least one Git Sha that was requested by the client,
        // we are in a position to send the packfile to the client. Populate the header
        // of the packfile
        header = Some(pack_header().await?);
    }
    // Add a delim line to indicate the end of the acknowledgements section. Note that
    // the delim line will not be followed by a newline character
    delim_to_write(&mut output_buffer).await?;
    Ok((Some(Bytes::from(output_buffer)), header))
}

async fn shallow_info(
    _context: Arc<RepositoryRequestContext>,
    _args: Arc<FetchArgs>,
) -> Result<Option<Bytes>, Error> {
    // TODO(rajshar): Implement shallow-info support
    Ok(None)
}

async fn wanted_refs(
    context: Arc<RepositoryRequestContext>,
    args: Arc<FetchArgs>,
) -> Result<Option<Bytes>, Error> {
    // If there are no refs explictly requested, then we can return early
    if args.want_refs.is_empty() {
        return Ok(None);
    }
    let mut output_buffer = vec![];
    write_text_packetline(WANTED_REFS_HEADER, &mut output_buffer).await?;
    let refs = ref_oid_mapping(&context.ctx, &context.repo, args.want_refs.clone())
        .await
        .context("Failed to fetch ref_oid_mapping for wanted-refs")?;
    for (ref_key, oid) in refs {
        write_text_packetline(
            format!("{} {}", ref_key, oid.to_hex()).as_bytes(),
            &mut output_buffer,
        )
        .await?;
    }
    // Add a delim line to indicate the end of the wanted-refs section. Note that
    // the delim line will not be followed by a newline character
    delim_to_write(&mut output_buffer).await?;
    Ok(Some(Bytes::from(output_buffer)))
}

async fn packfile_uris(
    _context: Arc<RepositoryRequestContext>,
    _args: Arc<FetchArgs>,
) -> Result<Option<Bytes>, Error> {
    // TODO(rajshar): Implement packfile-uris support
    Ok(None)
}

impl FetchResponseHeaders {
    async fn from_request(
        context: RepositoryRequestContext,
        args: FetchArgs,
    ) -> Result<Self, Error> {
        let (context, args) = (Arc::new(context), Arc::new(args));
        let acknowledgements_future = async {
            acknowledgements(context.clone(), args.clone())
                .await
                .context("Failed to generate acknowledgements")
        };
        let shallow_info_future = async {
            shallow_info(context.clone(), args.clone())
                .await
                .context("Failed to generate shallow info")
        };
        let wanted_refs_future = async {
            wanted_refs(context.clone(), args.clone())
                .await
                .context("Failed to generate wanted refs")
        };
        let packfile_uris_future = async {
            packfile_uris(context.clone(), args.clone())
                .await
                .context("Failed to generate packfile uris")
        };
        let ((acknowledgements, pack_header), shallow_info, wanted_refs, packfile_uris) =
            try_join4(
                acknowledgements_future,
                shallow_info_future,
                wanted_refs_future,
                packfile_uris_future,
            )
            .await?;
        Ok(Self {
            acknowledgements,
            shallow_info,
            wanted_refs,
            packfile_uris,
            pack_header,
        })
    }

    fn include_pack(&self) -> bool {
        self.pack_header.is_some()
    }
}

impl Iterator for FetchResponseHeaders {
    type Item = Bytes;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(acknowledgements) = self.acknowledgements.take() {
            Some(acknowledgements)
        } else if let Some(shallow_info) = self.shallow_info.take() {
            Some(shallow_info)
        } else if let Some(wanted_refs) = self.wanted_refs.take() {
            Some(wanted_refs)
        } else if let Some(packfile_uris) = self.packfile_uris.take() {
            Some(packfile_uris)
        } else {
            self.pack_header.take()
        }
    }
}

async fn get_body(state: &mut State) -> Result<Bytes, HttpError> {
    Body::take_from(state)
        .try_concat_body(&HeaderMap::new())
        .map_err(HttpError::e500)?
        .await
        .map_err(HttpError::e500)
}

fn git_method_info(command: &Command, repo: String) -> GitMethodInfo {
    let (method, variants) = match command {
        Command::LsRefs(_) => (GitMethod::LsRefs, vec![GitMethodVariant::Standard]),
        Command::Fetch(ref fetch_args) => {
            let method = if fetch_args.haves.is_empty() && fetch_args.done {
                GitMethod::Clone
            } else {
                GitMethod::Pull
            };
            let mut variants = vec![];
            if fetch_args.is_shallow() {
                variants.push(GitMethodVariant::Shallow);
            }
            if fetch_args.is_filter() {
                variants.push(GitMethodVariant::Filter);
            }
            if variants.is_empty() {
                variants.push(GitMethodVariant::Standard);
            }
            (method, variants)
        }
    };
    GitMethodInfo {
        method,
        variants,
        repo,
    }
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
    let request_context = RepositoryRequestContext::instantiate(
        state,
        git_method_info(&request_command.command, repo_name),
    )
    .await?;
    state.put(Service::GitUploadPack);
    state.put(ResponseType::Result);
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
    let fetch_response_headers =
        FetchResponseHeaders::from_request(request_context.clone(), args.clone()).await?;
    let include_pack = fetch_response_headers.include_pack();
    let bytes_stream = ResponseStream::new(try_stream! {
        let mut pack_reader = tokio_stream::wrappers::ReceiverStream::new(reader).ready_chunks(100_000_000);
        for header in fetch_response_headers {
            yield header;
        }
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
            // If we don't need to send back a packfile, just return early
            if !include_pack {
                return Ok(());
            }
            let response_stream = fetch_response(
                request_context.ctx.clone(),
                &request_context.repo,
                args.into_request(concurrency(&request_context)),
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
