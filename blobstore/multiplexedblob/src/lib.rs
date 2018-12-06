// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]

extern crate cloned;
extern crate failure_ext as failure;
extern crate futures;
extern crate futures_ext;
extern crate tokio;

extern crate blobstore;
extern crate context;
extern crate mononoke_types;

#[cfg(test)]
extern crate async_unit;

use std::collections::HashMap;
use std::fmt::{self, Write};
use std::sync::Arc;

use cloned::cloned;
use failure::{err_msg, Error};
use futures::future::{self, Future, Loop};
use futures_ext::{BoxFuture, FutureExt};
use tokio::executor::spawn;

use blobstore::Blobstore;
use context::CoreContext;
use mononoke_types::BlobstoreBytes;

/// Id used to discriminate diffirent underlying blobstore instances
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlobstoreId(u32);

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

pub struct MultiplexedBlobstore {
    blobstores: Vec<(BlobstoreId, Arc<Blobstore>)>,
    handler: Arc<MultiplexedBlobstorePutHandler>,
}

impl MultiplexedBlobstore {
    pub fn new(
        blobstores: Vec<(BlobstoreId, Arc<Blobstore>)>,
        handler: Arc<MultiplexedBlobstorePutHandler>,
    ) -> Self {
        Self {
            blobstores,
            handler,
        }
    }
}

fn make_composite_error(errors: HashMap<usize, Error>) -> Error {
    let mut error_string = String::from("Some blobstores failed and others ruturned None:\n");
    for (index, error) in errors {
        writeln!(&mut error_string, "{}: {}", index, error).expect("in-memory write failed");
    }
    err_msg(error_string)
}

impl Blobstore for MultiplexedBlobstore {
    fn get(&self, ctx: CoreContext, key: String) -> BoxFuture<Option<BlobstoreBytes>, Error> {
        let requests = self.blobstores
            .iter()
            .map(|(_, blobstore)| blobstore.get(ctx.clone(), key.clone()))
            .collect();
        let state = (
            requests,                       // pending requests
            HashMap::<usize, Error>::new(), // previous errors
        );
        future::loop_fn(state, |(requests, mut errors)| {
            future::select_all(requests).then(move |result| {
                let requests = match result {
                    Ok((value @ Some(_), ..)) => return future::ok(Loop::Break(value)),
                    Ok((None, _, requests)) => requests,
                    Err((error, index, requests)) => {
                        errors.insert(index, error);
                        requests
                    }
                };
                if requests.is_empty() {
                    if errors.is_empty() {
                        future::ok(Loop::Break(None))
                    } else {
                        // some of the underlying blobstores retuned error
                        // which means we can not say for sure that this key does not exist
                        future::err(make_composite_error(errors))
                    }
                } else {
                    future::ok(Loop::Continue((requests, errors)))
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
            .map(|(_, blobstore)| blobstore.is_present(ctx.clone(), key.clone()))
            .collect();
        let state = (
            requests,                       // pending requests
            HashMap::<usize, Error>::new(), // previous errors
        );
        future::loop_fn(state, |(requests, mut errors)| {
            future::select_all(requests).then(move |result| {
                let requests = match result {
                    Ok((true, ..)) => return future::ok(Loop::Break(true)),
                    Ok((false, _, requests)) => requests,
                    Err((error, index, requests)) => {
                        errors.insert(index, error);
                        requests
                    }
                };
                if requests.is_empty() {
                    if errors.is_empty() {
                        future::ok(Loop::Break(false))
                    } else {
                        // some of the underlying blobstores retuned error
                        // which means we can not say for sure that this key does not exist
                        future::err(make_composite_error(errors))
                    }
                } else {
                    future::ok(Loop::Continue((requests, errors)))
                }
            })
        }).boxify()
    }
}

impl fmt::Debug for MultiplexedBlobstore {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "MultiplexedBlobstore")?;
        f.debug_map()
            .entries(self.blobstores.iter().map(|(ref k, ref v)| (k, v)))
            .finish()
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use async_unit;
    use failure::err_msg;
    use futures::Async;
    use futures::future::IntoFuture;
    use futures::sync::oneshot;
    use std::borrow::BorrowMut;
    use std::collections::{HashMap, VecDeque};
    use std::sync::Mutex;

    fn with<T, F, V>(value: &Arc<Mutex<T>>, scope: F) -> V
    where
        F: FnOnce(&mut T) -> V,
    {
        let mut value_guard = value.lock().expect("Lock poisoned");
        scope(value_guard.borrow_mut())
    }

    pub struct TickBlobstore {
        pub storage: Arc<Mutex<HashMap<String, BlobstoreBytes>>>,
        // queue of pending operations
        queue: Arc<Mutex<VecDeque<oneshot::Sender<Option<String>>>>>,
    }

    impl fmt::Debug for TickBlobstore {
        fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
            f.debug_struct("TickBlobstore")
                .field("storage", &self.storage)
                .field("pending", &with(&self.queue, |q| q.len()))
                .finish()
        }
    }

    impl TickBlobstore {
        pub fn new() -> Self {
            Self {
                storage: Default::default(),
                queue: Default::default(),
            }
        }
        pub fn tick(&self, error: Option<&str>) {
            let mut queue = self.queue.lock().unwrap();
            for send in queue.drain(..) {
                send.send(error.map(String::from)).unwrap();
            }
        }
        pub fn on_tick(&self) -> impl Future<Item = (), Error = Error> {
            let (send, recv) = oneshot::channel();
            let mut queue = self.queue.lock().unwrap();
            queue.push_back(send);
            recv.map_err(Error::from).and_then(|error| match error {
                None => Ok(()),
                Some(error) => Err(err_msg(error)),
            })
        }
    }

    impl Blobstore for TickBlobstore {
        fn get(&self, _ctx: CoreContext, key: String) -> BoxFuture<Option<BlobstoreBytes>, Error> {
            let storage = self.storage.clone();
            self.on_tick()
                .map(move |_| with(&storage, |s| s.get(&key).cloned()))
                .boxify()
        }
        fn put(
            &self,
            _ctx: CoreContext,
            key: String,
            value: BlobstoreBytes,
        ) -> BoxFuture<(), Error> {
            let storage = self.storage.clone();
            self.on_tick()
                .map(move |_| {
                    with(&storage, |s| {
                        s.insert(key, value);
                    })
                })
                .boxify()
        }
    }

    struct LogHandler {
        pub log: Arc<Mutex<Vec<(BlobstoreId, String)>>>,
    }

    impl LogHandler {
        fn new() -> Self {
            Self {
                log: Default::default(),
            }
        }
        fn clear(&self) {
            with(&self.log, |log| log.clear())
        }
    }

    impl MultiplexedBlobstorePutHandler for LogHandler {
        fn on_put(
            &self,
            _ctx: CoreContext,
            blobstore_id: BlobstoreId,
            key: String,
        ) -> BoxFuture<(), Error> {
            with(&self.log, move |log| log.push((blobstore_id, key)));
            Ok(()).into_future().boxify()
        }
    }

    fn make_value(value: &str) -> BlobstoreBytes {
        BlobstoreBytes::from_bytes(value.as_bytes())
    }

    #[test]
    fn simple() {
        async_unit::tokio_unit_test(|| {
            let bs0 = Arc::new(TickBlobstore::new());
            let bs1 = Arc::new(TickBlobstore::new());
            let log = Arc::new(LogHandler::new());
            let bs = MultiplexedBlobstore::new(
                vec![(BlobstoreId(0), bs0.clone()), (BlobstoreId(1), bs1.clone())],
                log.clone(),
            );
            let ctx = CoreContext::test_mock();

            // succeed as soon as first blobstore succeeded
            {
                let v0 = make_value("v0");
                let k0 = String::from("k0");

                let mut put_fut = bs.put(ctx.clone(), k0.clone(), v0.clone());
                assert_eq!(put_fut.poll().unwrap(), Async::NotReady);
                bs0.tick(None);
                put_fut.wait().unwrap();
                assert_eq!(
                    with(&bs0.storage, |s| s.get(&k0).cloned()),
                    Some(v0.clone())
                );
                assert!(with(&bs1.storage, |s| s.is_empty()));
                bs1.tick(Some("bs1 failed"));
                assert!(with(&log.log, |log| log == &vec![(BlobstoreId(0), k0.clone())]));

                // should succeed as it is stored in bs1
                let mut get_fut = bs.get(ctx.clone(), k0);
                assert_eq!(get_fut.poll().unwrap(), Async::NotReady);
                bs0.tick(None);
                bs1.tick(None);
                assert_eq!(get_fut.wait().unwrap(), Some(v0));
                assert!(with(&bs1.storage, |s| s.is_empty()));

                log.clear();
            }

            // wait for second if first one failed
            {
                let v1 = make_value("v1");
                let k1 = String::from("k1");

                let mut put_fut = bs.put(ctx.clone(), k1.clone(), v1.clone());
                assert_eq!(put_fut.poll().unwrap(), Async::NotReady);
                bs0.tick(Some("bs0 failed"));
                assert_eq!(put_fut.poll().unwrap(), Async::NotReady);
                bs1.tick(None);
                put_fut.wait().unwrap();
                assert!(with(&bs0.storage, |s| s.get(&k1).is_none()));
                assert_eq!(
                    with(&bs1.storage, |s| s.get(&k1).cloned()),
                    Some(v1.clone())
                );
                assert!(with(&log.log, |log| log == &vec![(BlobstoreId(1), k1.clone())]));

                let mut get_fut = bs.get(ctx.clone(), k1.clone());
                assert_eq!(get_fut.poll().unwrap(), Async::NotReady);
                bs0.tick(None);
                assert_eq!(get_fut.poll().unwrap(), Async::NotReady);
                bs1.tick(None);
                assert_eq!(get_fut.wait().unwrap(), Some(v1));
                assert!(with(&bs0.storage, |s| s.get(&k1).is_none()));

                log.clear();
            }

            // both fail => whole put fail
            {
                let k2 = String::from("k2");
                let v2 = make_value("v2");

                let mut put_fut = bs.put(ctx.clone(), k2.clone(), v2.clone());
                assert_eq!(put_fut.poll().unwrap(), Async::NotReady);
                bs0.tick(Some("bs0 failed"));
                assert_eq!(put_fut.poll().unwrap(), Async::NotReady);
                bs1.tick(Some("bs1 failed"));
                assert!(put_fut.wait().is_err());
            }

            // get: Error + None -> Error
            {
                let k3 = String::from("k3");
                let mut get_fut = bs.get(ctx.clone(), k3);
                assert_eq!(get_fut.poll().unwrap(), Async::NotReady);

                bs0.tick(Some("bs0 failed"));
                assert_eq!(get_fut.poll().unwrap(), Async::NotReady);

                bs1.tick(None);
                assert!(get_fut.wait().is_err());
            }

            // get: None + None -> None
            {
                let k3 = String::from("k3");
                let mut get_fut = bs.get(ctx.clone(), k3);
                assert_eq!(get_fut.poll().unwrap(), Async::NotReady);

                bs0.tick(None);
                assert_eq!(get_fut.poll().unwrap(), Async::NotReady);

                bs1.tick(None);
                assert_eq!(get_fut.wait().unwrap(), None);
            }

            // both put succeed
            {
                let k4 = String::from("k4");
                let v4 = make_value("v4");
                log.clear();

                let mut put_fut = bs.put(ctx.clone(), k4.clone(), v4.clone());
                assert_eq!(put_fut.poll().unwrap(), Async::NotReady);
                bs0.tick(None);
                put_fut.wait().unwrap();
                assert_eq!(
                    with(&bs0.storage, |s| s.get(&k4).cloned()),
                    Some(v4.clone())
                );
                bs1.tick(None);
                while with(&log.log, |log| log.len() != 2) {}
                assert_eq!(
                    with(&bs1.storage, |s| s.get(&k4).cloned()),
                    Some(v4.clone())
                );
            }
        });
    }
}
