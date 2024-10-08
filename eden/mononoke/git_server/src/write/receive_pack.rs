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
use gotham_ext::middleware::ScubaMiddlewareState;
use gotham_ext::response::BytesBody;
use gotham_ext::response::TryIntoResponse;
use hyper::Body;
use hyper::Response;
use import_tools::GitImportLfs;
use metaconfig_types::RepoConfigRef;
use packetline::encode::flush_to_write;
use packetline::encode::write_text_packetline;
use protocol::pack_processor::parse_pack;
use repo_blobstore::RepoBlobstoreArc;
use slog::info;

use crate::command::Command;
use crate::command::PushArgs;
use crate::command::RefUpdate;
use crate::command::RequestCommand;
use crate::model::GitMethodInfo;
use crate::model::GitServerContext;
use crate::model::PushValidationErrors;
use crate::model::RepositoryParams;
use crate::model::RepositoryRequestContext;
use crate::service::set_ref;
use crate::service::set_refs;
use crate::service::upload_objects;
use crate::service::GitMappingsStore;
use crate::service::GitObjectStore;
use crate::service::RefUpdateOperation;
use crate::util::empty_body;
use crate::util::get_body;
use crate::util::mononoke_source_of_truth;

const PACK_OK: &[u8] = b"unpack ok";
const REF_OK: &str = "ok";
const REF_ERR: &str = "ng";
const REF_UPDATE_CONCURRENCY: usize = 20;

pub async fn receive_pack(state: &mut State) -> Result<Response<Body>, HttpError> {
    let repo_name = RepositoryParams::borrow_from(state).repo_name();
    ScubaMiddlewareState::try_borrow_add(state, "repo", repo_name.as_str());
    ScubaMiddlewareState::try_borrow_add(state, "method", "push");
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
            GitMethodInfo::from_command(&request_command.command, repo_name.clone()),
        )
        .await?,
    );
    let mut output = vec![];
    if let Command::Push(push_args) = request_command.command {
        // If Mononoke is not the source of truth for this repo, then we need to prevent the push
        if !mononoke_source_of_truth(&request_context.ctx, request_context.repo.clone()).await? {
            return reject_push(repo_name.as_str(), state, &push_args.ref_updates).await;
        }
        let (ctx, blobstore) = (
            &request_context.ctx,
            request_context.repo.repo_blobstore_arc().clone(),
        );
        info!(
            request_context.ctx.logger(),
            "Parsing packfile for repo {}", repo_name
        );
        // Parse the packfile provided as part of the push and verify that its valid
        let parsed_objects = parse_pack(push_args.pack_file, ctx, blobstore.clone()).await?;
        // Generate the GitObjectStore using the parsed objects
        let object_store = Arc::new(GitObjectStore::new(parsed_objects, ctx, blobstore.clone()));
        // Instantiate the LFS configuration
        let git_ctx = GitServerContext::borrow_from(state);
        let lfs = if request_context
            .repo
            .repo_config()
            .git_configs
            .git_lfs_interpret_pointers
        {
            GitImportLfs::new(
                git_ctx
                    .upstream_lfs_server()?
                    .ok_or_else(|| anyhow::anyhow!("No upstream LFS server specified"))?,
                false,    // allow_not_found
                2,        // max attempts
                Some(50), // conn_limit
                git_ctx.tls_args()?,
            )?
        } else {
            GitImportLfs::new_disabled()
        };
        info!(
            request_context.ctx.logger(),
            "Uploading packfile objects for repo {}", repo_name
        );
        // Upload the objects corresponding to the push to the underlying store
        let ref_map = upload_objects(
            ctx,
            request_context.repo.clone(),
            object_store.clone(),
            &push_args.ref_updates,
            lfs,
        )
        .await?;
        info!(
            request_context.ctx.logger(),
            "Uploaded packfile objects for repo {}. Sending PACK_OK to the client", repo_name
        );
        // We were successful in parsing the pack and uploading the objects to underlying store. Indicate this to the client
        write_text_packetline(PACK_OK, &mut output).await?;
        // Create bonsai_git_mapping store to enable mapping lookup during bookmark movement
        let git_bonsai_mapping_store = Arc::new(GitMappingsStore::new(
            ctx,
            request_context.repo.bonsai_git_mapping_arc(),
            ref_map,
        ));
        info!(
            request_context.ctx.logger(),
            "Updating refs for repo {}", repo_name
        );
        let updated_refs = refs_update(
            &push_args,
            request_context.clone(),
            git_bonsai_mapping_store.clone(),
            object_store.clone(),
        )
        .await?;
        info!(
            request_context.ctx.logger(),
            "Updated refs for repo {}. Sending ref update status to the client", repo_name
        );
        let mut validation_errors = PushValidationErrors::default();
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
                    validation_errors
                        .add_error(updated_ref.ref_name.clone(), e.root_cause().to_string());
                    write_text_packetline(
                        format!("{} {} {}", REF_ERR, updated_ref.ref_name, e.root_cause())
                            .as_bytes(),
                        &mut output,
                    )
                    .await?;
                }
            }
        }
        if !validation_errors.is_empty() {
            state.put(validation_errors);
        }
        flush_to_write(&mut output).await?;
    }
    BytesBody::new(Bytes::from(output), mime::TEXT_PLAIN).try_into_response(state)
}

/// Function responsible for updating the refs in the repo
async fn refs_update(
    push_args: &PushArgs<'_>,
    request_context: Arc<RepositoryRequestContext>,
    git_bonsai_mapping_store: Arc<GitMappingsStore>,
    object_store: Arc<GitObjectStore>,
) -> anyhow::Result<Vec<(RefUpdate, anyhow::Result<()>)>> {
    if push_args.settings.atomic {
        atomic_refs_update(
            push_args,
            request_context,
            git_bonsai_mapping_store,
            object_store,
        )
        .await
    } else {
        non_atomic_refs_update(
            push_args,
            request_context,
            git_bonsai_mapping_store,
            object_store,
        )
        .await
    }
}

/// Function responsible for updating the refs in the repo non-atomically.
async fn non_atomic_refs_update(
    push_args: &PushArgs<'_>,
    request_context: Arc<RepositoryRequestContext>,
    git_bonsai_mapping_store: Arc<GitMappingsStore>,
    object_store: Arc<GitObjectStore>,
) -> anyhow::Result<Vec<(RefUpdate, anyhow::Result<()>)>> {
    stream::iter(push_args.ref_updates.clone())
        .map(|ref_update| {
            cloned!(request_context, git_bonsai_mapping_store, object_store);
            async move {
                let ctx = request_context.ctx.clone();
                let ref_info = ref_update.clone();
                info!(
                    ctx.logger(),
                    "Updating ref {} from {} to {}",
                    ref_info.ref_name.as_str(),
                    ref_info.from.to_hex(),
                    ref_info.to.to_hex()
                );
                let output = tokio::spawn(async move {
                    set_ref(
                        request_context,
                        git_bonsai_mapping_store,
                        object_store,
                        RefUpdateOperation::new(ref_update),
                    )
                    .await
                })
                .await?;
                info!(
                    ctx.logger(),
                    "Updated ref {} from {} to {}",
                    ref_info.ref_name.as_str(),
                    ref_info.from.to_hex(),
                    ref_info.to.to_hex()
                );
                anyhow::Ok(output)
            }
        })
        .buffer_unordered(REF_UPDATE_CONCURRENCY)
        .try_collect::<Vec<_>>()
        .await
}

/// Function responsible for updating the refs in the repo atomically.
async fn atomic_refs_update(
    push_args: &PushArgs<'_>,
    request_context: Arc<RepositoryRequestContext>,
    git_bonsai_mapping_store: Arc<GitMappingsStore>,
    object_store: Arc<GitObjectStore>,
) -> anyhow::Result<Vec<(RefUpdate, anyhow::Result<()>)>> {
    let ref_update_ops = push_args
        .ref_updates
        .iter()
        .map(|ref_update| RefUpdateOperation::new(ref_update.clone()))
        .collect::<Vec<_>>();
    let ref_updates = push_args.ref_updates.clone();
    match set_refs(
        request_context,
        git_bonsai_mapping_store,
        object_store,
        ref_update_ops,
    )
    .await
    {
        Ok(_) => Ok(ref_updates
            .into_iter()
            .map(|ref_update| (ref_update, Ok(())))
            .collect()),
        Err(e) => {
            let err_str = format!(
                "Atomic bookmark update failed with error: {}",
                e.root_cause()
            );
            Ok(ref_updates
                .into_iter()
                .map(|ref_update| (ref_update, Err(anyhow::anyhow!(err_str.to_string()))))
                .collect())
        }
    }
}

async fn reject_push(
    repo_name: &str,
    state: &mut State,
    ref_updates: &[RefUpdate],
) -> anyhow::Result<Response<Body>> {
    let mut output = vec![];
    let error_message =
        format!("Push rejected: Mononoke is not the source of truth for repo {repo_name}");
    write_text_packetline(error_message.as_bytes(), &mut output).await?;
    for ref_update in ref_updates {
        write_text_packetline(
            format!("{} {} {}", REF_ERR, ref_update.ref_name, &error_message).as_bytes(),
            &mut output,
        )
        .await?;
    }
    flush_to_write(&mut output).await?;
    BytesBody::new(Bytes::from(output), mime::TEXT_PLAIN).try_into_response(state)
}
