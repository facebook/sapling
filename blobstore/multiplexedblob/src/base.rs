// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use cloned::cloned;
use failure::{Error, Fail};
use futures::future::{self, Future, Loop};
use futures_ext::{BoxFuture, FutureExt};
use tokio::executor::spawn;

use blobstore::Blobstore;
use context::CoreContext;
use metaconfig::BlobstoreId;
use mononoke_types::BlobstoreBytes;

#[derive(Fail, Debug, Clone)]
pub enum ErrorKind {
    #[fail(display = "Some blobstores failed, and other returned None: {:?}", _0)]
    SomeFailedOthersNone(Arc<HashMap<BlobstoreId, Error>>),
    #[fail(display = "All blobstores faield: {:?}", _0)]
    AllFailed(Arc<HashMap<BlobstoreId, Error>>),
}

/// This handler is called on each successful put to underlying blobstore,
/// for put to be considered successful this handler must return success.
/// It will be used to keep self-healing table up to date.
pub trait MultiplexedBlobstorePutHandler: Send + Sync {
    fn on_put(
        &self,
        ctx: CoreContext,
        blobstore_id: BlobstoreId,
        key: String,
    ) -> BoxFuture<(), Error>;
}

pub struct MultiplexedBlobstoreBase {
    blobstores: Arc<[(BlobstoreId, Arc<Blobstore>)]>,
    handler: Arc<MultiplexedBlobstorePutHandler>,
}

impl MultiplexedBlobstoreBase {
    pub fn new(
        blobstores: Vec<(BlobstoreId, Arc<Blobstore>)>,
        handler: Arc<MultiplexedBlobstorePutHandler>,
    ) -> Self {
        Self {
            blobstores: blobstores.into(),
            handler,
        }
    }
}

impl Blobstore for MultiplexedBlobstoreBase {
    fn get(&self, ctx: CoreContext, key: String) -> BoxFuture<Option<BlobstoreBytes>, Error> {
        let requests = self.blobstores
            .iter()
            .map(|&(blobstore_id, ref blobstore)| {
                blobstore
                    .get(ctx.clone(), key.clone())
                    .map_err(move |error| (blobstore_id, error))
            })
            .collect();
        let state = (
            requests,                             // pending requests
            HashMap::<BlobstoreId, Error>::new(), // previous errors
        );
        let blobstores_count = self.blobstores.len();
        future::loop_fn(state, move |(requests, mut errors)| {
            future::select_all(requests).then({
                move |result| {
                    let requests = match result {
                        Ok((value @ Some(_), ..)) => return future::ok(Loop::Break(value)),
                        Ok((None, _, requests)) => requests,
                        Err(((blobstore_id, error), _, requests)) => {
                            errors.insert(blobstore_id, error);
                            requests
                        }
                    };
                    if requests.is_empty() {
                        if errors.is_empty() {
                            future::ok(Loop::Break(None))
                        } else {
                            let error = if errors.len() == blobstores_count {
                                ErrorKind::AllFailed(errors.into())
                            } else {
                                ErrorKind::SomeFailedOthersNone(errors.into())
                            };
                            future::err(error.into())
                        }
                    } else {
                        future::ok(Loop::Continue((requests, errors)))
                    }
                }
            })
        }).boxify()
    }

    fn put(&self, ctx: CoreContext, key: String, value: BlobstoreBytes) -> BoxFuture<(), Error> {
        let requests = self.blobstores.iter().map(|(blobstore_id, blobstore)| {
            blobstore
                .put(ctx.clone(), key.clone(), value.clone())
                .and_then({
                    cloned!(ctx, key, blobstore_id, self.handler);
                    move |_| handler.on_put(ctx, blobstore_id, key)
                })
        });

        future::select_ok(requests)
            .map(|(_, requests)| {
                let requests_fut = future::join_all(
                    requests.into_iter().map(|request| request.then(|_| Ok(()))),
                ).map(|_| ());
                spawn(requests_fut);
            })
            .boxify()
    }

    fn is_present(&self, ctx: CoreContext, key: String) -> BoxFuture<bool, Error> {
        let requests = self.blobstores
            .iter()
            .map(|&(blobstore_id, ref blobstore)| {
                blobstore
                    .is_present(ctx.clone(), key.clone())
                    .map_err(move |error| (blobstore_id, error))
            })
            .collect();
        let state = (
            requests,                             // pending requests
            HashMap::<BlobstoreId, Error>::new(), // previous errors
        );
        let blobstores_count = self.blobstores.len();
        future::loop_fn(state, move |(requests, mut errors)| {
            future::select_all(requests).then({
                move |result| {
                    let requests = match result {
                        Ok((true, ..)) => return future::ok(Loop::Break(true)),
                        Ok((false, _, requests)) => requests,
                        Err(((blobstore_id, error), _, requests)) => {
                            errors.insert(blobstore_id, error);
                            requests
                        }
                    };
                    if requests.is_empty() {
                        if errors.is_empty() {
                            future::ok(Loop::Break(false))
                        } else {
                            let error = if errors.len() == blobstores_count {
                                ErrorKind::AllFailed(errors.into())
                            } else {
                                ErrorKind::SomeFailedOthersNone(errors.into())
                            };
                            future::err(error.into())
                        }
                    } else {
                        future::ok(Loop::Continue((requests, errors)))
                    }
                }
            })
        }).boxify()
    }
}

impl fmt::Debug for MultiplexedBlobstoreBase {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "MultiplexedBlobstoreBase")?;
        f.debug_map()
            .entries(self.blobstores.iter().map(|(ref k, ref v)| (k, v)))
            .finish()
    }
}
