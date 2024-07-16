/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use bonsai_git_mapping::BonsaiGitMappingArc;
use bytes::Bytes;
use cloned::cloned;
use futures::stream;
use futures::StreamExt;
use futures::TryStreamExt;
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
use repo_blobstore::RepoBlobstoreArc;

use crate::command::Command;
use crate::command::RequestCommand;
use crate::model::GitMethodInfo;
use crate::model::RepositoryParams;
use crate::model::RepositoryRequestContext;
use crate::service::set_ref;
use crate::service::upload_objects;
use crate::service::GitMappingsStore;
use crate::service::GitObjectStore;
use crate::service::RefUpdateOperation;
use crate::util::empty_body;
use crate::util::get_body;

const PACK_OK: &[u8] = b"unpack ok";
const REF_OK: &str = "ok";
const REF_ERR: &str = "ng";
const REF_UPDATE_CONCURRENCY: usize = 20;

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
    let request_context = Arc::new(
        RepositoryRequestContext::instantiate(
            state,
            GitMethodInfo::from_command(&request_command.command, repo_name),
        )
        .await?,
    );
    let mut output = vec![];
    if let Command::Push(push_args) = request_command.command {
        let (ctx, blobstore) = (
            &request_context.ctx,
            request_context.repo.repo_blobstore_arc().clone(),
        );
        // Parse the packfile provided as part of the push and verify that its valid
        let parsed_objects = parse_pack(push_args.pack_file, ctx, blobstore.clone()).await?;
        // Generate the GitObjectStore using the parsed objects
        let object_store = Arc::new(GitObjectStore::new(parsed_objects, ctx, blobstore.clone()));
        // Upload the objects corresponding to the push to the underlying store
        let git_bonsai_mappings = upload_objects(
            ctx,
            request_context.repo.clone(),
            object_store,
            &push_args.ref_updates,
        )
        .await?;
        let affected_changesets_len = git_bonsai_mappings.len();
        // We were successful in parsing the pack and uploading the objects to underlying store. Indicate this to the client
        write_text_packetline(PACK_OK, &mut output).await?;
        // Create bonsai_git_mapping store to enable mapping lookup during bookmark movement
        let git_bonsai_mapping_store = Arc::new(GitMappingsStore::new(
            ctx,
            request_context.repo.inner.bonsai_git_mapping_arc(),
            git_bonsai_mappings,
        ));
        // Update each ref concurrently (TODO(rajshar): Add support for atomic ref update)
        let updated_refs = stream::iter(push_args.ref_updates.clone())
            .map(|ref_update| {
                cloned!(request_context, git_bonsai_mapping_store);
                async move {
                    let output = tokio::spawn(async move {
                        let ref_update_op = RefUpdateOperation::new(
                            ref_update.clone(),
                            affected_changesets_len,
                            None,
                        ); // TODO(rajshar): Populate pushvars from HTTP headers
                        set_ref(request_context, git_bonsai_mapping_store, ref_update_op).await
                    })
                    .await?;
                    anyhow::Ok(output)
                }
            })
            .buffer_unordered(REF_UPDATE_CONCURRENCY)
            .try_collect::<Vec<_>>()
            .await?;
        // For each ref, update the status as ok or ng based on the result of the bookmark set operation
        for (updated_ref, result) in updated_refs {
            match result {
                Ok(_) => {
                    write_text_packetline(
                        format!("{} {}", REF_OK, updated_ref.ref_name).as_bytes(),
                        &mut output,
                    )
                    .await?;
                }
                Err(e) => {
                    write_text_packetline(
                        format!("{} {} {}", REF_ERR, updated_ref.ref_name, e).as_bytes(),
                        &mut output,
                    )
                    .await?;
                }
            }
        }
        flush_to_write(&mut output).await?;
    }
    BytesBody::new(Bytes::from(output), mime::TEXT_PLAIN).try_into_response(state)
}
