/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Context;
use anyhow::Error;
use futures::future;
use futures::future::FutureExt;
use futures::pin_mut;
use futures::select;
use gotham::state::FromState;
use gotham::state::State;
use gotham_derive::StateData;
use gotham_derive::StaticResponseExtender;
use gotham_ext::body_ext::BodyExt;
use gotham_ext::middleware::RequestStartTime;
use gotham_ext::middleware::ScubaMiddlewareState;
use gotham_ext::response::BytesBody;
use gotham_ext::response::TryIntoResponse;
use http::header::HeaderMap;
use hyper::Body;
use hyper::StatusCode;
use maplit::hashmap;
use rand::Rng;
use redactedblobstore::has_redaction_root_cause;
use serde::Deserialize;
use slog::debug;
use stats::prelude::*;
use std::collections::HashMap;
use std::num::NonZeroU16;
use std::time::Instant;
use time_ext::DurationExt;
use time_window_counter::GlobalTimeWindowCounterBuilder;

use blobstore::Blobstore;
use blobstore::Loadable;
use blobstore::LoadableError;
use filestore::Alias;
use gotham_ext::error::HttpError;
use lfs_protocol::git_lfs_mime;
use lfs_protocol::ObjectAction;
use lfs_protocol::ObjectError;
use lfs_protocol::ObjectStatus;
use lfs_protocol::Operation;
use lfs_protocol::RequestBatch;
use lfs_protocol::RequestObject;
use lfs_protocol::ResponseBatch;
use lfs_protocol::ResponseObject;
use lfs_protocol::Transfer;
use mononoke_types::hash::Sha256;
use mononoke_types::typed_hash::ContentId;
use mononoke_types::BlobstoreKey;
use repo_blobstore::RepoBlobstoreRef;

use crate::errors::ErrorKind;
use crate::lfs_server_context::RepositoryRequestContext;
use crate::lfs_server_context::UriBuilder;
use crate::middleware::LfsMethod;
use crate::popularity::allow_consistent_routing;
use crate::scuba::LfsScubaKey;

define_stats! {
    prefix ="mononoke.lfs.batch";
    download_redirect_internal: timeseries(Rate, Sum),
    download_redirect_upstream: timeseries(Rate, Sum),
    download_unknown: timeseries(Rate, Sum),
    upload_redirect: timeseries(Rate, Sum),
    upload_no_redirect: timeseries(Rate, Sum),
    upload_rejected: timeseries(Rate, Sum),
}

enum Source {
    Internal,
    Upstream,
}

#[derive(Deserialize, StateData, StaticResponseExtender)]
pub struct BatchParams {
    repository: String,
}

/// A collection of objects available in a specific server (internal, or upstream).
struct ServerObjects {
    objects: HashMap<lfs_protocol::Sha256, (u64, ObjectAction)>,
}

impl ServerObjects {
    fn empty() -> Self {
        Self {
            objects: HashMap::new(),
        }
    }

    fn get(&self, oid: &lfs_protocol::Sha256) -> Option<(RequestObject, ObjectAction)> {
        let (size, action) = self.objects.get(oid)?;
        Some((
            RequestObject {
                oid: *oid,
                size: *size,
            },
            action.clone(),
        ))
    }

    fn contains(&self, oid: &lfs_protocol::Sha256) -> bool {
        self.objects.contains_key(oid)
    }
}

impl FromIterator<(RequestObject, ObjectAction)> for ServerObjects {
    fn from_iter<I: IntoIterator<Item = (RequestObject, ObjectAction)>>(iter: I) -> Self {
        let mut objects = HashMap::new();

        for (obj, action) in iter {
            objects.insert(obj.oid, (obj.size, action));
        }

        Self { objects }
    }
}

impl FromIterator<(InternalObject, ObjectAction)> for ServerObjects {
    fn from_iter<I: IntoIterator<Item = (InternalObject, ObjectAction)>>(iter: I) -> Self {
        let mut objects = HashMap::new();

        for (obj, action) in iter {
            objects.insert(obj.oid.into(), (obj.download_size(), action));
        }

        Self { objects }
    }
}

enum UpstreamObjects {
    UpstreamPresence(ServerObjects),
    NoUpstream,
}

impl UpstreamObjects {
    fn should_upload(&self, oid: &lfs_protocol::Sha256) -> bool {
        match self {
            // Upload to upstream if the object is missing there.
            UpstreamObjects::UpstreamPresence(objs) => !objs.contains(oid),
            // Without an upstream, we never need to upload there.
            UpstreamObjects::NoUpstream => false,
        }
    }

    fn download_action(&self, oid: &lfs_protocol::Sha256) -> Option<(RequestObject, ObjectAction)> {
        match self {
            // Passthrough download actions from upstream.
            UpstreamObjects::UpstreamPresence(map) => map.get(oid),
            // In the absence of an upstream, we cannot download from there.
            UpstreamObjects::NoUpstream => None,
        }
    }
}

// TODO: Unit tests for this. We could use a client that lets us do stub things.
async fn upstream_objects(
    ctx: &RepositoryRequestContext,
    objects: &[RequestObject],
) -> Result<UpstreamObjects, ErrorKind> {
    let objects = objects.to_vec();

    let batch = RequestBatch {
        operation: Operation::Download,
        r#ref: None,
        transfers: vec![Transfer::Basic],
        objects,
    };

    let res = ctx.upstream_batch(&batch).await?;

    let ResponseBatch { transfer, objects } = match res {
        Some(res) => res,
        None => {
            return Ok(UpstreamObjects::NoUpstream);
        }
    };

    let objects = match transfer {
        Transfer::Basic => {
            // Extract valid download actions from upstream. Those are the objects upstream has.
            objects
                .into_iter()
                .filter_map(|object| {
                    let ResponseObject { object, status } = object;

                    let mut actions = match status {
                        ObjectStatus::Ok {
                            authenticated: false,
                            actions,
                        } => actions,
                        _ => HashMap::new(),
                    };

                    actions
                        .remove(&Operation::Download)
                        .map(|action| (object, action))
                })
                .collect()
        }
        Transfer::Unknown => ServerObjects::empty(),
    };

    Ok(UpstreamObjects::UpstreamPresence(objects))
}

/// An object available internally (i.e. on this server).
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct InternalObject {
    id: ContentId,
    oid: Sha256,
    size: Option<u64>,
}

impl InternalObject {
    pub fn new(id: ContentId, oid: Sha256, size: Option<u64>) -> Self {
        Self { id, oid, size }
    }

    pub fn id(&self) -> ContentId {
        self.id
    }

    pub fn download_size(&self) -> u64 {
        self.size.unwrap_or(0)
    }
}

async fn resolve_internal_object(
    ctx: &RepositoryRequestContext,
    oid: Sha256,
) -> Result<Option<InternalObject>, Error> {
    let blobstore = ctx.repo.repo_blobstore();

    let content_id = Alias::Sha256(oid).load(&ctx.ctx, blobstore).await;

    let content_id = match content_id {
        Ok(content_id) => content_id,
        Err(LoadableError::Missing(_)) => return Ok(None),
        Err(e) => return Err(Error::from(e).context(ErrorKind::LocalAliasLoadError)),
    };

    // The filestore may allow aliases to be created before the contents are created (the creation
    // of the content is what makes it logically exists), so we should check for the content's
    // existence before we proceed here. Querying metadata does this implicitly, since metadata is
    // written after content. This wouldn't matter if we didn't have an upstream, but it does
    // matter for now to handle the (very much edge-y) case of the content existing in the
    // upstream, its alias existing locally, but not its content (T57777060).

    let meta = filestore::get_metadata(&blobstore, &ctx.ctx, &(content_id.into()))
        .await
        .with_context(|| format!("Failed fetching content metadata for {:?}", content_id));

    match meta {
        Ok(Some(meta)) => {
            return Ok(Some(InternalObject::new(
                meta.content_id,
                meta.sha256,
                Some(meta.total_size),
            )));
        }
        Ok(None) => {
            return Ok(None);
        }
        Err(e) if has_redaction_root_cause(&e) => {
            // Fallthrough to an existence check.
        }
        Err(e) => {
            return Err(e);
        }
    };

    // If a load error was caused by redaction, then check for existence instead, which isn't
    // subject to redaction (only the content is).

    let exists = blobstore
        .is_present(&ctx.ctx, &content_id.blobstore_key())
        .await
        .with_context(|| format!("Failed to check for existence of: {:?}", content_id))?
        .assume_not_found_if_unsure();

    if exists {
        return Ok(Some(InternalObject::new(content_id, oid, None)));
    }

    Ok(None)
}

fn generate_routing_key(tasks_per_content: NonZeroU16, oid: Sha256) -> String {
    // Randomly generate task number to send to.
    let task_n = rand::thread_rng().gen_range(0..tasks_per_content.get());
    // For the base task, no extension is added to routing key.
    let mut routing_key = format!("{}", oid);
    if task_n > 0 {
        // All other tasks have tailing number in routing key.
        routing_key = format!("{}-{}", routing_key, task_n);
    }
    routing_key
}

async fn internal_objects(
    ctx: &RepositoryRequestContext,
    objects: &[RequestObject],
) -> Result<ServerObjects, ErrorKind> {
    let futs = objects.iter().map(|req| async move {
        let obj = resolve_internal_object(ctx, req.oid.into())
            .await
            .map_err(ErrorKind::Error)?;

        let allow_consistent_routing = match obj {
            Some(obj) => allow_consistent_routing(ctx, obj, GlobalTimeWindowCounterBuilder).await,
            None => true,
        };

        Result::<_, ErrorKind>::Ok((obj, allow_consistent_routing))
    });

    let objs = future::try_join_all(futs).await?;

    objs.into_iter()
        .filter_map(|(maybe_obj, allow_consistent_routing)| match maybe_obj {
            // Map the objects we have locally into an action routing to a Mononoke LFS server.
            Some(obj) => {
                let uri = if allow_consistent_routing && ctx.config.enable_consistent_routing() {
                    let routing_key = generate_routing_key(ctx.config.tasks_per_content(), obj.oid);
                    ctx.uri_builder
                        .consistent_download_uri(&obj.id, routing_key)
                } else {
                    ctx.uri_builder.download_uri(&obj.id)
                };

                Some(uri.map(|uri| (obj, ObjectAction::new(uri))))
            }
            None => None,
        })
        .collect::<Result<ServerObjects, ErrorKind>>()
}

fn batch_upload_response_objects(
    uri_builder: &UriBuilder,
    max_upload_size: Option<u64>,
    objects: &[RequestObject],
    upstream: &UpstreamObjects,
    internal: &ServerObjects,
) -> Result<Vec<ResponseObject>, ErrorKind> {
    let objects: Result<Vec<ResponseObject>, ErrorKind> = objects
        .iter()
        .map(|object| {
            let status = match (
                upstream.should_upload(&object.oid),
                internal.contains(&object.oid),
                max_upload_size,
            ) {
                (false, true, _) => {
                    // Object doesn't need to be uploaded anywhere: move on.
                    STATS::upload_no_redirect.add_value(1);

                    ObjectStatus::Ok {
                        authenticated: false,
                        actions: hashmap! {},
                    }
                }
                (_, _, Some(max_upload_size)) if object.size > max_upload_size => {
                    // Object is too large and upload is required: reject it (note: this doesn't
                    // enforce that uploads cannot be done: the upload endpoint has its own
                    // validation too).
                    STATS::upload_rejected.add_value(1);

                    ObjectStatus::Err {
                        error: ObjectError {
                            code: StatusCode::BAD_REQUEST.as_u16(),
                            message: ErrorKind::UploadTooLarge(object.size, max_upload_size)
                                .to_string(),
                        },
                    }
                }
                _ => {
                    // Object is missing in at least one location. Require uploading it.
                    STATS::upload_redirect.add_value(1);
                    let uri = uri_builder.upload_uri(object)?;
                    let action = ObjectAction::new(uri);

                    ObjectStatus::Ok {
                        authenticated: false,
                        actions: hashmap! { Operation::Upload => action },
                    }
                }
            };

            Ok(ResponseObject {
                object: *object,
                status,
            })
        })
        .collect();
    objects
}

async fn batch_upload(
    ctx: &RepositoryRequestContext,
    batch: RequestBatch,
) -> Result<ResponseBatch, ErrorKind> {
    let (upstream, internal) = future::try_join(
        upstream_objects(ctx, &batch.objects),
        internal_objects(ctx, &batch.objects),
    )
    .await?;

    let objects = batch_upload_response_objects(
        &ctx.uri_builder,
        ctx.max_upload_size(),
        &batch.objects,
        &upstream,
        &internal,
    )?;

    Ok(ResponseBatch {
        transfer: Transfer::Basic,
        objects,
    })
}

/// This method peforms the routing logic for a given object being requested, given what's
/// available in upstream and downstream.
fn route_download_for_object<'a>(
    upstream: &'a Result<UpstreamObjects, Error>,
    internal: &'a ServerObjects,
    oid: &'_ lfs_protocol::Sha256,
) -> Result<Option<(Source, RequestObject, ObjectAction)>, Error> {
    // If we have the object internally, then get it from there.
    if let Some((obj, action)) = internal.get(oid) {
        return Ok(Some((Source::Internal, obj, action)));
    }

    match upstream {
        // If our upstream succeded, then we try to get the object from there.
        Ok(ref upstream) => {
            if let Some((obj, action)) = upstream.download_action(oid) {
                return Ok(Some((Source::Upstream, obj, action)));
            }
        }
        // If our upstream failed, then we don't know whether the object is available anywhere, so
        // that's an error.
        Err(ref cause) => {
            let err =
                Error::new(ErrorKind::ObjectNotInternallyAvailableAndUpstreamUnavailable(*oid))
                    .context(cause.to_string());
            return Err(err);
        }
    }

    // If upstream succeeded and we found the result neither locally nor remotely, then that's
    // just a missing object.
    Ok(None)
}

fn batch_download_response_objects(
    objects: &[RequestObject],
    upstream: &Result<UpstreamObjects, Error>,
    internal: &ServerObjects,
    scuba: &mut Option<&mut ScubaMiddlewareState>,
) -> Result<Vec<ResponseObject>, Error> {
    let mut upstream_blobs = vec![];
    let responses = objects
        .iter()
        .map(|request_object| {
            let location = route_download_for_object(upstream, internal, &request_object.oid)?;

            let res = match location {
                Some((source, server_object, action)) => {
                    match source {
                        Source::Internal => STATS::download_redirect_internal.add_value(1),
                        Source::Upstream => {
                            upstream_blobs.push(server_object.oid.to_string());
                            STATS::download_redirect_upstream.add_value(1);
                        }
                    };

                    let status = ObjectStatus::Ok {
                        authenticated: false,
                        actions: hashmap! { Operation::Download => action },
                    };

                    ResponseObject {
                        object: server_object,
                        status,
                    }
                }
                None => {
                    STATS::download_unknown.add_value(1);

                    let status = ObjectStatus::Err {
                        error: ObjectError {
                            code: StatusCode::NOT_FOUND.as_u16(),
                            message: "Object does not exist".to_string(),
                        },
                    };

                    ResponseObject {
                        object: *request_object,
                        status,
                    }
                }
            };

            Result::<_, Error>::Ok(res)
        })
        .collect::<Result<Vec<_>, _>>()?;

    ScubaMiddlewareState::maybe_add(
        scuba,
        LfsScubaKey::BatchInternalMissingBlobs,
        upstream_blobs,
    );

    Ok(responses)
}

/// Try to prepare a batch response with only internal objects. Returns None if any are missing.
fn batch_download_internal_only_response_objects(
    objects: &[RequestObject],
    internal: &ServerObjects,
) -> Option<Vec<ResponseObject>> {
    let res = objects
        .iter()
        .map(|request_object| {
            let (server_object, action) = internal.get(&request_object.oid)?;

            let status = ObjectStatus::Ok {
                authenticated: false,
                actions: hashmap! { Operation::Download => action },
            };

            Some(ResponseObject {
                object: server_object,
                status,
            })
        })
        .collect::<Option<Vec<_>>>()?;

    // Record stats only if all have succeeded.
    STATS::download_redirect_internal.add_value(res.len() as i64);

    Some(res)
}

async fn batch_download(
    ctx: &RepositoryRequestContext,
    batch: RequestBatch,
    scuba: &mut Option<&mut ScubaMiddlewareState>,
) -> Result<ResponseBatch, ErrorKind> {
    let upstream = upstream_objects(ctx, &batch.objects).fuse();
    let internal = internal_objects(ctx, &batch.objects).fuse();
    pin_mut!(upstream, internal);

    let mut update_batch_order = |status| {
        ScubaMiddlewareState::maybe_add(scuba, LfsScubaKey::BatchOrder, status);
    };

    update_batch_order("error");

    let objects = select! {
        upstream_objects = upstream => {
            update_batch_order("upstream");
            debug!(ctx.logger(), "batch: upstream ready");
            let internal_objects = internal.await?;
            debug!(ctx.logger(), "batch: internal ready");
            batch_download_response_objects(&batch.objects, &upstream_objects.map_err(Error::from), &internal_objects, scuba)
        }
        internal_objects = internal => {
            debug!(ctx.logger(), "batch: internal ready");
            let internal_objects = internal_objects?;

            let objects = if ctx.always_wait_for_upstream() {
                None
            } else {
                batch_download_internal_only_response_objects(&batch.objects, &internal_objects)
            };

            if let Some(objects) = objects {
                // We were able to return with just internal, don't wait for upstream.
                update_batch_order("internal");
                debug!(ctx.logger(), "batch: skip upstream");
                Ok(objects)
            } else {
                // We don't have all the objects: wait for upstream.
                update_batch_order("both");
                let upstream_objects = upstream.await;
                debug!(ctx.logger(), "batch: upstream ready");
                batch_download_response_objects(&batch.objects, &upstream_objects.map_err(Error::from), &internal_objects, scuba)
            }
        }
    }.map_err(ErrorKind::Error)?;

    Ok(ResponseBatch {
        transfer: Transfer::Basic,
        objects,
    })
}

// TODO: Do we want to validate the client's Accept & Content-Type headers here?
pub async fn batch(state: &mut State) -> Result<impl TryIntoResponse, HttpError> {
    let BatchParams { repository } = state.take();
    let start_time = state
        .try_borrow::<RequestStartTime>()
        .map_or_else(Instant::now, |t| t.0);

    let ctx =
        RepositoryRequestContext::instantiate(state, repository.clone(), LfsMethod::Batch).await?;

    ScubaMiddlewareState::maybe_add(
        &mut state.try_borrow_mut::<ScubaMiddlewareState>(),
        LfsScubaKey::BatchRequestContextReadyUs,
        start_time.elapsed().as_micros_unchecked(),
    );

    let body = Body::take_from(state);
    let headers = HeaderMap::try_borrow_from(state);

    let body = body
        .try_concat_body_opt(headers)
        .map_err(HttpError::e400)?
        .await
        .context(ErrorKind::ClientCancelled)
        .map_err(HttpError::e400)?;

    let mut scuba = state.try_borrow_mut::<ScubaMiddlewareState>();

    ScubaMiddlewareState::maybe_add(
        &mut scuba,
        LfsScubaKey::BatchRequestReceivedUs,
        start_time.elapsed().as_micros_unchecked(),
    );

    let request_batch = serde_json::from_slice::<RequestBatch>(&body)
        .context(ErrorKind::InvalidBatch)
        .map_err(HttpError::e400)?;

    ScubaMiddlewareState::maybe_add(
        &mut scuba,
        LfsScubaKey::BatchObjectCount,
        request_batch.objects.len(),
    );

    ScubaMiddlewareState::maybe_add(
        &mut scuba,
        LfsScubaKey::BatchRequestParsedUs,
        start_time.elapsed().as_micros_unchecked(),
    );

    let res = match request_batch.operation {
        Operation::Upload => batch_upload(&ctx, request_batch).await,
        Operation::Download => batch_download(&ctx, request_batch, &mut scuba).await,
    };

    ScubaMiddlewareState::maybe_add(
        &mut scuba,
        LfsScubaKey::BatchResponseReadyUs,
        start_time.elapsed().as_micros_unchecked(),
    );

    let res = res.map_err(|e| match e {
        ErrorKind::HostNotAllowlisted(_) => HttpError::e400(e),
        _ => HttpError::e500(e),
    })?;
    let body = serde_json::to_string(&res).map_err(HttpError::e500)?;

    Ok(BytesBody::new(body, git_lfs_mime()))
}

#[cfg(test)]
mod test {
    use super::*;

    use async_trait::async_trait;
    use blobstore::BlobstoreBytes;
    use blobstore::BlobstoreGetData;
    use bytes::Bytes;
    use context::CoreContext;
    use fbinit::FacebookInit;

    use filestore::FilestoreConfigRef;
    use filestore::StoreRequest;
    use futures::stream;
    use hyper::Uri;
    use memblob::Memblob;
    use mononoke_types::ContentMetadataId;
    use mononoke_types_mocks::hash::FOURS_SHA256;
    use mononoke_types_mocks::hash::ONES_SHA256;
    use mononoke_types_mocks::hash::THREES_SHA256;
    use mononoke_types_mocks::hash::TWOS_SHA256;
    use redactedblobstore::RedactedBlobs;
    use redactedblobstore::RedactedMetadata;
    use std::collections::HashSet;
    use std::sync::atomic::AtomicU64;
    use std::sync::atomic::Ordering;
    use std::sync::Arc;
    use test_repo_factory::TestRepoFactory;

    use pretty_assertions::assert_eq;

    use crate::lfs_server_context::ServerUris;
    use crate::Repo;

    fn obj(oid: Sha256, size: u64) -> RequestObject {
        RequestObject {
            oid: oid.into(),
            size,
        }
    }

    #[test]
    fn test_download() -> Result<(), Error> {
        let o1 = obj(ONES_SHA256, 111);
        let o2 = obj(TWOS_SHA256, 222);
        let o3 = obj(THREES_SHA256, 333);
        let o4 = obj(FOURS_SHA256, 444);

        let upstream = hashmap! {
            o1 => ObjectAction::new("http://foo.com/1".parse()?),
            o2 => ObjectAction::new("http://foo.com/2".parse()?),
        }
        .into_iter()
        .collect();

        let upstream = Ok(UpstreamObjects::UpstreamPresence(upstream));

        let internal = hashmap! {
            o2 => ObjectAction::new("http://bar.com/2".parse()?),
            o3 => ObjectAction::new("http://bar.com/3".parse()?),
        }
        .into_iter()
        .collect();

        let res =
            batch_download_response_objects(&[o1, o2, o3, o4], &upstream, &internal, &mut None)?;

        assert_eq!(
            vec![
                ResponseObject {
                    object: o1,
                    status: ObjectStatus::Ok {
                        authenticated: false,
                        // This is in upstream only
                        actions: hashmap! { Operation::Download =>  ObjectAction::new("http://foo.com/1".parse()?) }
                    }
                },
                ResponseObject {
                    object: o2,
                    status: ObjectStatus::Ok {
                        authenticated: false,
                        // This is in both, so it'll go to internal
                        actions: hashmap! { Operation::Download =>  ObjectAction::new("http://bar.com/2".parse()?) }
                    }
                },
                ResponseObject {
                    object: o3,
                    status: ObjectStatus::Ok {
                        authenticated: false,
                        // This is in internal only
                        actions: hashmap! { Operation::Download =>  ObjectAction::new("http://bar.com/3".parse()?) }
                    }
                },
                ResponseObject {
                    object: o4,
                    status: ObjectStatus::Err {
                        error: ObjectError {
                            code: 404,
                            message: "Object does not exist".to_string(),
                        }
                    }
                }
            ],
            res
        );

        // Test sending the wrong size. Ensure we still find the object here.

        let res2 = batch_download_response_objects(
            &[obj(ONES_SHA256, 0), obj(THREES_SHA256, 0)],
            &upstream,
            &internal,
            &mut None,
        )?;

        assert_eq!(
            vec![
                ResponseObject {
                    object: o1,
                    status: ObjectStatus::Ok {
                        authenticated: false,
                        actions: hashmap! { Operation::Download =>  ObjectAction::new("http://foo.com/1".parse()?) }
                    }
                },
                ResponseObject {
                    object: o3,
                    status: ObjectStatus::Ok {
                        authenticated: false,
                        actions: hashmap! { Operation::Download =>  ObjectAction::new("http://bar.com/3".parse()?) }
                    }
                },
            ],
            res2
        );

        Ok(())
    }

    #[test]
    fn test_routing_keys() -> Result<(), Error> {
        // allowed keys
        let allowed_routing_key_base: String = format!("{}", ONES_SHA256);
        let allowed_routing_key_one: String = format!("{}-1", allowed_routing_key_base);
        // base case
        let routing_key_base = generate_routing_key(NonZeroU16::new(1).unwrap(), ONES_SHA256);
        assert_eq!(&routing_key_base, &allowed_routing_key_base);

        // random key case
        let allowed_routing_keys = vec![allowed_routing_key_base, allowed_routing_key_one];
        for _ in 0..5 {
            let routing_key = generate_routing_key(NonZeroU16::new(2).unwrap(), ONES_SHA256);
            assert!(allowed_routing_keys.contains(&routing_key))
        }

        Ok(())
    }

    #[test]
    fn test_download_upstream_failed_and_its_ok() -> Result<(), Error> {
        let o1 = obj(ONES_SHA256, 111);

        let req = vec![o1];

        let upstream = Err(Error::msg("Oops"));

        let internal = hashmap! {
            o1 => ObjectAction::new("http://foo.com/1".parse()?),
        }
        .into_iter()
        .collect();

        let res = batch_download_response_objects(&req, &upstream, &internal, &mut None)?;

        assert_eq!(
            vec![ResponseObject {
                object: o1,
                status: ObjectStatus::Ok {
                    authenticated: false,
                    actions: hashmap! { Operation::Download =>  ObjectAction::new("http://foo.com/1".parse()?) }
                }
            },],
            res
        );

        Ok(())
    }

    #[test]
    fn test_download_upstream_failed_and_its_not_ok() -> Result<(), Error> {
        let o1 = obj(ONES_SHA256, 111);

        let req = vec![o1];

        let upstream = Err(Error::msg("Oops"));

        let internal = ServerObjects::empty();

        let res = batch_download_response_objects(&req, &upstream, &internal, &mut None);
        assert!(res.is_err());

        Ok(())
    }

    fn upload_uri(object: &RequestObject) -> Result<Uri, Error> {
        let r = format!(
            "http://foo.com/repo123/upload/{}/{}",
            object.oid, object.size
        )
        .parse()?;
        Ok(r)
    }

    #[test]
    fn test_upload() -> Result<(), Error> {
        let o1 = obj(ONES_SHA256, 123);
        let o2 = obj(TWOS_SHA256, 456);
        let o3 = obj(THREES_SHA256, 789);
        let o4 = obj(FOURS_SHA256, 1111);

        let req = vec![o1, o2, o3, o4];

        let upstream = hashmap! {
            o1 => ObjectAction::new("http://foo.com/1".parse()?),
            o2 => ObjectAction::new("http://foo.com/2".parse()?),
        }
        .into_iter()
        .collect();

        let internal = hashmap! {
            o2 => ObjectAction::new("http://bar.com/2".parse()?),
            o3 => ObjectAction::new("http://bar.com/3".parse()?),
        }
        .into_iter()
        .collect();

        let server = ServerUris::new(
            vec!["http://foo.com".to_string()],
            Some("http://bar.com".to_string()),
        )?;
        let uri_builder = UriBuilder {
            repository: "repo123".to_string(),
            server: Arc::new(server),
            host: "foo.com".to_string(),
        };

        let res = batch_upload_response_objects(
            &uri_builder,
            Some(1000),
            &req,
            &UpstreamObjects::UpstreamPresence(upstream),
            &internal,
        )?;

        assert_eq!(
            vec![
                ResponseObject {
                    object: o1,
                    status: ObjectStatus::Ok {
                        authenticated: false,
                        // This is in upstream only, so it needs uploading
                        actions: hashmap! { Operation::Upload =>  ObjectAction::new(upload_uri(&o1)?) }
                    }
                },
                ResponseObject {
                    object: o2,
                    status: ObjectStatus::Ok {
                        authenticated: false,
                        // This is in both, so no actions are required.
                        actions: hashmap! {}
                    }
                },
                ResponseObject {
                    object: o3,
                    status: ObjectStatus::Ok {
                        authenticated: false,
                        // This is in internal only, so it needs uploading
                        actions: hashmap! { Operation::Upload =>  ObjectAction::new(upload_uri(&o3)?) }
                    }
                },
                ResponseObject {
                    object: o4,
                    status: ObjectStatus::Err {
                        error: ObjectError {
                            code: 400,
                            message: "Object size (1111) exceeds max allowed size (1000)"
                                .to_string(),
                        }
                    }
                },
            ],
            res
        );

        Ok(())
    }

    #[fbinit::test]
    async fn test_resolve_missing(fb: FacebookInit) -> Result<(), Error> {
        let ctx = RepositoryRequestContext::test_builder(fb)?.build()?;
        assert_eq!(resolve_internal_object(&ctx, ONES_SHA256).await?, None);
        Ok(())
    }

    #[fbinit::test]
    async fn test_resolve_present(fb: FacebookInit) -> Result<(), Error> {
        let ctx = RepositoryRequestContext::test_builder(fb)?.build()?;

        let meta = filestore::store(
            ctx.repo.repo_blobstore(),
            *ctx.repo.filestore_config(),
            &ctx.ctx,
            &StoreRequest::new(6),
            stream::once(async move { Ok(Bytes::from("foobar")) }),
        )
        .await?;

        assert_eq!(
            resolve_internal_object(&ctx, meta.sha256)
                .await?
                .map(|o| o.id),
            Some(meta.content_id)
        );
        Ok(())
    }

    #[derive(Debug)]
    struct HideBlob<B> {
        inner: B,
        hide: HashSet<String>,
        hits: Arc<AtomicU64>,
    }

    impl<B: std::fmt::Display> std::fmt::Display for HideBlob<B> {
        fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            write!(f, "HideBlob<{}>", &self.inner)
        }
    }

    #[async_trait]
    impl<B: Blobstore> Blobstore for HideBlob<B> {
        async fn get<'a>(
            &'a self,
            ctx: &'a CoreContext,
            key: &'a str,
        ) -> Result<Option<BlobstoreGetData>, Error> {
            if self.hide.contains(key) {
                self.hits.fetch_add(1, Ordering::SeqCst);
                return Ok(None);
            }
            self.inner.get(ctx, key).await
        }

        async fn put<'a>(
            &'a self,
            ctx: &'a CoreContext,
            key: String,
            value: BlobstoreBytes,
        ) -> Result<(), Error> {
            self.inner.put(ctx, key, value).await
        }
    }

    #[fbinit::test]
    async fn test_resolve_redacted(fb: FacebookInit) -> Result<(), Error> {
        // Create a single factory with a blobstore we've created.
        let mut factory = TestRepoFactory::new(fb)?;
        let stub_blobstore = Arc::new(Memblob::default());
        factory.with_blobstore(stub_blobstore.clone());

        // First, have the filestore tell us what the hash for this blob would be, so we can create
        // a new repo and redact it.
        let stub: Repo = factory.build()?;

        let meta = filestore::store(
            stub.repo_blobstore(),
            *stub.filestore_config(),
            &CoreContext::test_mock(fb),
            &StoreRequest::new(6),
            stream::once(async move { Ok(Bytes::from("foobar")) }),
        )
        .await?;

        // Now, create a new blob repo with the same blob store, but with some data redacted.
        let repo = factory
            .redacted(Some(RedactedBlobs::FromSql(Arc::new(hashmap! {
                meta.content_id.blobstore_key() => RedactedMetadata {
                    task: "test".to_string(),
                    log_only: false,
                }
            }))))
            .build()?;

        let ctx = RepositoryRequestContext::test_builder(fb)?
            .repo(repo)
            .build()?;

        assert_eq!(
            resolve_internal_object(&ctx, meta.sha256)
                .await?
                .map(|o| o.id),
            Some(meta.content_id)
        );

        // Now, create another one with the same redaction, but this time pretend the metadata does
        // not exist. Building the key is a bit hacky here, but we have an assertion on the number
        // of hits later to make sure it works.

        let key = format!(
            "repo0000.{}",
            ContentMetadataId::from(meta.content_id).blobstore_key()
        );

        let hits = Arc::new(AtomicU64::new(0));

        let hide_blobstore = HideBlob {
            inner: stub_blobstore,
            hide: HashSet::from_iter(Some(key)),
            hits: hits.clone(),
        };

        let repo = factory.with_blobstore(Arc::new(hide_blobstore)).build()?;

        let ctx = RepositoryRequestContext::test_builder(fb)?
            .repo(repo)
            .build()?;

        assert_eq!(
            resolve_internal_object(&ctx, meta.sha256)
                .await?
                .map(|o| o.id),
            Some(meta.content_id)
        );

        assert_eq!(hits.load(Ordering::SeqCst), 1);

        Ok(())
    }

    #[fbinit::test]
    async fn test_resolve_size(fb: FacebookInit) -> Result<(), Error> {
        let repo: Repo = test_repo_factory::build_empty(fb)?;

        let meta = filestore::store(
            repo.repo_blobstore(),
            *repo.filestore_config(),
            &CoreContext::test_mock(fb),
            &StoreRequest::new(6),
            stream::once(async move { Ok(Bytes::from("foobar")) }),
        )
        .await?;

        let ctx = RepositoryRequestContext::test_builder(fb)?
            .repo(repo)
            .build()?;

        let obj = RequestObject {
            oid: meta.sha256.into(),
            size: 6,
        };

        let (obj1, action1) = internal_objects(&ctx, &[obj])
            .await?
            .get(&meta.sha256.into())
            .context("Missing v1")?;

        // Note: give the server the wrong size here.
        let (obj2, action2) = internal_objects(&ctx, &[RequestObject { size: 0, ..obj }])
            .await?
            .get(&meta.sha256.into())
            .context("Missing v2")?;

        assert_eq!(obj1, obj);
        assert_eq!(obj2, obj);
        assert_eq!(action1, action2);

        Ok(())
    }
}
