// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use failure::Error;
use futures_preview::{compat::Future01CompatExt, compat::Stream01CompatExt, TryStreamExt};
use futures_util::{pin_mut, select, try_future::try_join_all, try_join, FutureExt};
use gotham::state::{FromState, State};
use gotham_derive::{StateData, StaticResponseExtender};
use hyper::{Body, StatusCode};
use maplit::hashmap;
use serde::Deserialize;
use slog::debug;
use stats::{define_stats, Timeseries};
use std::collections::HashMap;

use blobstore::{Blobstore, Loadable, LoadableError};
use failure_ext::chain::ChainExt;
use filestore::Alias;
use mononoke_types::{hash::Sha256, typed_hash::ContentId, MononokeId};

use crate::errors::ErrorKind;
use crate::http::{git_lfs_mime, BytesBody, HttpError, TryIntoResponse};
use crate::lfs_server_context::{RepositoryRequestContext, UriBuilder};
use crate::middleware::{LfsMethod, ScubaMiddlewareState};
use crate::protocol::{
    ObjectAction, ObjectError, ObjectStatus, Operation, RequestBatch, RequestObject, ResponseBatch,
    ResponseObject, Transfer,
};

define_stats! {
    prefix ="mononoke.lfs.batch";
    download_redirect_internal: timeseries(RATE, SUM),
    download_redirect_upstream: timeseries(RATE, SUM),
    download_unknown: timeseries(RATE, SUM),
    upload_redirect: timeseries(RATE, SUM),
    upload_no_redirect: timeseries(RATE, SUM),
}

enum Source {
    Internal,
    Upstream,
}

#[derive(Deserialize, StateData, StaticResponseExtender)]
pub struct BatchParams {
    repository: String,
}

enum UpstreamObjects {
    UpstreamPresence(HashMap<RequestObject, ObjectAction>),
    NoUpstream,
}

impl UpstreamObjects {
    fn should_upload(&self, obj: &RequestObject) -> bool {
        match self {
            // Upload to upstream if the object is missing there.
            UpstreamObjects::UpstreamPresence(map) => !map.contains_key(obj),
            // Without an upstream, we never need to upload there.
            UpstreamObjects::NoUpstream => false,
        }
    }

    fn download_action(&self, obj: &RequestObject) -> Option<&ObjectAction> {
        match self {
            // Passthrough download actions from upstream.
            UpstreamObjects::UpstreamPresence(map) => map.get(obj),
            // In the absence of an upstream, we cannot download from there.
            UpstreamObjects::NoUpstream => None,
        }
    }
}

// TODO: Unit tests for this. We could use a client that lets us do stub things.
async fn upstream_objects(
    ctx: &RepositoryRequestContext,
    objects: &[RequestObject],
) -> Result<UpstreamObjects, Error> {
    let objects = objects.iter().map(|o| *o).collect();

    let batch = RequestBatch {
        operation: Operation::Download,
        r#ref: None,
        transfers: vec![Transfer::Basic],
        objects,
    };

    let res = ctx
        .upstream_batch(&batch)
        .await
        .chain_err(ErrorKind::UpstreamBatchError)?;

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

                    match actions.remove(&Operation::Download) {
                        Some(action) => Some((object, action)),
                        None => None,
                    }
                })
                .collect()
        }
        Transfer::Unknown => HashMap::new(),
    };

    Ok(UpstreamObjects::UpstreamPresence(objects))
}

async fn resolve_internal_object(
    ctx: &RepositoryRequestContext,
    oid: Sha256,
) -> Result<Option<ContentId>, Error> {
    let blobstore = ctx.repo.get_blobstore();

    let content_id = Alias::Sha256(oid)
        .load(ctx.ctx.clone(), &blobstore)
        .compat()
        .await;

    let content_id = match content_id {
        Ok(content_id) => content_id,
        Err(LoadableError::Missing(_)) => return Ok(None),
        Err(e) => return Err(e.chain_err(ErrorKind::LocalAliasLoadError).into()),
    };

    // The filestore may allow aliases to be created before the contents are created (the creation
    // of the content is what makes it logically exists), so we should check for the content's
    // existence before we proceed here. This wouldn't matter if we didn't have an upstream, but it
    // does matter for now to handle the (very much edge-y) case of the content existing in the
    // upstream, its alias existing locally, but not its content.
    let exists = blobstore
        .is_present(ctx.ctx.clone(), content_id.blobstore_key())
        .compat()
        .await?;

    if exists {
        Ok(Some(content_id))
    } else {
        Ok(None)
    }
}

async fn internal_objects(
    ctx: &RepositoryRequestContext,
    objects: &[RequestObject],
) -> Result<HashMap<RequestObject, ObjectAction>, Error> {
    let futs = objects
        .iter()
        .map(|object| resolve_internal_object(ctx, object.oid));

    let content_ids = try_join_all(futs).await?;

    let ret: Result<HashMap<RequestObject, ObjectAction>, _> = objects
        .iter()
        .zip(content_ids.into_iter())
        .filter_map(|(obj, content_id)| match content_id {
            // Map the objects we have locally into an action routing to this LFS server.
            Some(content_id) => {
                let action = ctx
                    .uri_builder
                    .download_uri(&content_id)
                    .map(ObjectAction::new)
                    .map(|action| (*obj, action));
                Some(action)
            }
            None => None,
        })
        .collect();

    Ok(ret.chain_err(ErrorKind::GenerateDownloadUrisError)?)
}

fn batch_upload_response_objects(
    uri_builder: &UriBuilder,
    objects: &[RequestObject],
    upstream: &UpstreamObjects,
    internal: &HashMap<RequestObject, ObjectAction>,
) -> Result<Vec<ResponseObject>, Error> {
    let objects: Result<Vec<ResponseObject>, Error> = objects
        .iter()
        .map(|object| {
            let actions = match (upstream.should_upload(object), internal.get(object)) {
                (false, Some(_)) => {
                    // Object doesn't need to be uploaded anywhere: move on.
                    STATS::upload_no_redirect.add_value(1);
                    hashmap! {}
                }
                _ => {
                    // Object is missing in at least one location. Require uploading it.
                    STATS::upload_redirect.add_value(1);
                    let uri = uri_builder.upload_uri(&object)?;
                    let action = ObjectAction::new(uri);
                    hashmap! { Operation::Upload => action }
                }
            };

            Ok(ResponseObject {
                object: *object,
                status: ObjectStatus::Ok {
                    authenticated: false,
                    actions,
                },
            })
        })
        .collect();

    let objects = objects.chain_err(ErrorKind::GenerateUploadUrisError)?;

    Ok(objects)
}

async fn batch_upload(
    ctx: &RepositoryRequestContext,
    batch: RequestBatch,
) -> Result<ResponseBatch, Error> {
    let (upstream, internal) = try_join!(
        upstream_objects(ctx, &batch.objects),
        internal_objects(ctx, &batch.objects),
    )?;

    let objects =
        batch_upload_response_objects(&ctx.uri_builder, &batch.objects, &upstream, &internal)?;

    Ok(ResponseBatch {
        transfer: Transfer::Basic,
        objects,
    })
}

fn batch_download_response_objects(
    objects: &[RequestObject],
    upstream: &UpstreamObjects,
    internal: &HashMap<RequestObject, ObjectAction>,
) -> Vec<ResponseObject> {
    objects
        .iter()
        .map(|object| {
            // For downloads, see if we can find it from internal or upstream (which means we
            // prefer internal). If we can't find it in either, then that's an error.
            let internal_action = internal.get(object).map(|o| (Source::Internal, o));
            let upstream_action = upstream
                .download_action(object)
                .map(|o| (Source::Upstream, o));

            let status = match internal_action.or(upstream_action) {
                Some((source, action)) => {
                    match source {
                        Source::Internal => STATS::download_redirect_internal.add_value(1),
                        Source::Upstream => STATS::download_redirect_upstream.add_value(1),
                    };

                    ObjectStatus::Ok {
                        authenticated: false,
                        actions: hashmap! { Operation::Download => action.clone() },
                    }
                }
                None => {
                    STATS::download_unknown.add_value(1);
                    ObjectStatus::Err {
                        error: ObjectError {
                            code: StatusCode::NOT_FOUND.as_u16(),
                            message: "Object does not exist".to_string(),
                        },
                    }
                }
            };

            ResponseObject {
                object: *object,
                status,
            }
        })
        .collect()
}

/// Try to prepare a batch response with only internal objects. Returns None if any are missing.
fn batch_download_internal_only_response_objects(
    objects: &[RequestObject],
    internal: &HashMap<RequestObject, ObjectAction>,
) -> Option<Vec<ResponseObject>> {
    let res = objects
        .iter()
        .map(|object| {
            let action = internal.get(object)?.clone();

            let status = ObjectStatus::Ok {
                authenticated: false,
                actions: hashmap! { Operation::Download => action },
            };

            Some(ResponseObject {
                object: *object,
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
) -> Result<ResponseBatch, Error> {
    let upstream = upstream_objects(ctx, &batch.objects).fuse();
    let internal = internal_objects(ctx, &batch.objects).fuse();
    pin_mut!(upstream, internal);

    let objects = select! {
        upstream_objects = upstream => {
            let upstream_objects = upstream_objects?;
            debug!(ctx.logger(), "batch: upstream ready");
            let internal_objects = internal.await?;
            debug!(ctx.logger(), "batch: internal ready");
            batch_download_response_objects(&batch.objects, &upstream_objects, &internal_objects)
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
                debug!(ctx.logger(), "batch: skip upstream");
                objects
            } else {
                // We don't have all the objects: wait for upstream.
                let upstream_objects = upstream.await?;
                debug!(ctx.logger(), "batch: upstream ready");
                batch_download_response_objects(&batch.objects, &upstream_objects, &internal_objects)
            }
        }
    };

    Ok(ResponseBatch {
        transfer: Transfer::Basic,
        objects,
    })
}

// TODO: Do we want to validate the client's Accept & Content-Type headers here?
pub async fn batch(state: &mut State) -> Result<impl TryIntoResponse, HttpError> {
    let BatchParams { repository } = state.take();

    let ctx = RepositoryRequestContext::instantiate(state, repository.clone(), LfsMethod::Batch)
        .map_err(HttpError::e400)?;

    let body = Body::take_from(state)
        .compat()
        .try_concat()
        .await
        .chain_err(ErrorKind::ClientCancelled)
        .map_err(HttpError::e400)?
        .into_bytes();

    let request_batch = serde_json::from_slice::<RequestBatch>(&body)
        .chain_err(ErrorKind::InvalidBatch)
        .map_err(HttpError::e400)?;

    if let Some(scuba) = state.try_borrow_mut::<ScubaMiddlewareState>() {
        scuba.add("batch_object_count", request_batch.objects.len());
    }

    let res = match request_batch.operation {
        Operation::Upload => batch_upload(&ctx, request_batch).await,
        Operation::Download => batch_download(&ctx, request_batch).await,
    };

    let res = res.map_err(HttpError::e502)?;
    let body = serde_json::to_string(&res).map_err(HttpError::e500)?;

    Ok(BytesBody::new(body, git_lfs_mime()))
}

#[cfg(test)]
mod test {
    use super::*;

    use hyper::Uri;
    use std::sync::Arc;

    use mononoke_types::hash::Sha256;
    use pretty_assertions::assert_eq;
    use std::str::FromStr;

    use crate::lfs_server_context::ServerUris;

    const ONES_HASH: &str = "1111111111111111111111111111111111111111111111111111111111111111";
    const TWOS_HASH: &str = "2222222222222222222222222222222222222222222222222222222222222222";
    const THREES_HASH: &str = "3333333333333333333333333333333333333333333333333333333333333333";

    fn obj(oid: &str, size: u64) -> Result<RequestObject, Error> {
        let oid = Sha256::from_str(oid)?;
        Ok(RequestObject { oid, size })
    }

    #[test]
    fn test_download() -> Result<(), Error> {
        let o1 = obj(ONES_HASH, 111)?;
        let o2 = obj(TWOS_HASH, 222)?;
        let o3 = obj(THREES_HASH, 333)?;
        let o4 = obj(THREES_HASH, 444)?;

        let req = vec![o1, o2, o3, o4];

        let upstream = hashmap! {
            o1 => ObjectAction::new("http://foo.com/1".parse()?),
            o2 => ObjectAction::new("http://foo.com/2".parse()?),
        };

        let internal = hashmap! {
            o2 => ObjectAction::new("http://bar.com/2".parse()?),
            o3 => ObjectAction::new("http://bar.com/3".parse()?),
        };

        let res = batch_download_response_objects(
            &req,
            &UpstreamObjects::UpstreamPresence(upstream),
            &internal,
        );

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
        let o1 = obj(ONES_HASH, 123)?;
        let o2 = obj(TWOS_HASH, 456)?;
        let o3 = obj(THREES_HASH, 789)?;

        let req = vec![o1, o2, o3];

        let upstream = hashmap! {
            o1 => ObjectAction::new("http://foo.com/1".parse()?),
            o2 => ObjectAction::new("http://foo.com/2".parse()?),
        };

        let internal = hashmap! {
            o2 => ObjectAction::new("http://bar.com/2".parse()?),
            o3 => ObjectAction::new("http://bar.com/3".parse()?),
        };

        let server = ServerUris::new("http://foo.com", Some("http://bar.com"))?;
        let uri_builder = UriBuilder {
            repository: "repo123".to_string(),
            server: Arc::new(server),
        };

        let res = batch_upload_response_objects(
            &uri_builder,
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
                }
            ],
            res
        );

        Ok(())
    }
}
