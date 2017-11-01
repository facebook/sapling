// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::error;
use std::marker::PhantomData;
use std::sync::Arc;
use std::time::Duration;

use futures::{Future, IntoFuture};
use futures::future::{loop_fn, Loop};
use futures::sync::oneshot;
use futures_ext::{BoxFuture, FutureExt};
use tokio_core::reactor::{Remote, Timeout};

use super::Blobstore;
use boxed::ArcBlobstore;

struct SyncPhantomData<E>(PhantomData<E>);

unsafe impl<E> Sync for SyncPhantomData<E> {}

/// Blobstore that retries failed put/get operations with provided delay
pub struct RetryingBlobstore<K, Vi, Vo, E, EInner> {
    blobstore: ArcBlobstore<K, Vi, Vo, EInner>,
    remote: Remote,
    get_retry_delay: Arc<Fn(usize) -> Option<Duration> + Send + Sync>,
    put_retry_delay: Arc<Fn(usize) -> Option<Duration> + Send + Sync>,
    _phantom: SyncPhantomData<E>,
}

impl<K, Vi, Vo, E, EInner> RetryingBlobstore<K, Vi, Vo, E, EInner> {
    /// Take a blobstore and add retry functionality to it
    pub fn new(
        blobstore: ArcBlobstore<K, Vi, Vo, EInner>,
        remote: &Remote,
        get_retry_delay: Arc<Fn(usize) -> Option<Duration> + Send + Sync>,
        put_retry_delay: Arc<Fn(usize) -> Option<Duration> + Send + Sync>,
    ) -> Self {
        RetryingBlobstore {
            blobstore,
            remote: remote.clone(),
            get_retry_delay,
            put_retry_delay,
            _phantom: SyncPhantomData(PhantomData),
        }
    }
}

fn delay_for<E>(remote: &Remote, delay: Duration) -> BoxFuture<(), E>
where
    E: From<::std::io::Error> + From<oneshot::Canceled> + Send + 'static,
{
    let (tx, rx) = oneshot::channel();
    remote.spawn(move |handle| {
        Timeout::new(delay, handle)
            .into_future()
            .and_then(|x| x)
            .map_err(E::from)
            .then(move |finished| tx.send(finished))
            .map_err(|_| ())
    });

    rx.from_err().and_then(|x| x).boxify()
}

impl<K, Vi, Vo, E, EInner> Blobstore for RetryingBlobstore<K, Vi, Vo, E, EInner>
where
    K: Clone + Send + 'static,
    Vi: Clone + Send + 'static,
    Vo: AsRef<[u8]> + Send + 'static,
    E: From<EInner> + From<::std::io::Error> + From<oneshot::Canceled> + error::Error + Send + 'static,
    EInner: error::Error + Send + 'static,
{
    type Key = K;
    type ValueIn = Vi;
    type ValueOut = Vo;
    type Error = E;

    type GetBlob = BoxFuture<Option<Self::ValueOut>, Self::Error>;
    type PutBlob = BoxFuture<(), Self::Error>;

    fn get(&self, key: &Self::Key) -> Self::GetBlob {
        loop_fn((0, None), {
            let blobstore = self.blobstore.clone();
            let remote = self.remote.clone();
            let key = key.clone();
            let retry_delay = self.get_retry_delay.clone();
            move |(attempt, _): (usize, Option<Vo>)| {
                blobstore.get(&key).from_err().then({
                    let remote = remote.clone();
                    let retry_delay = retry_delay.clone();
                    move |result| match result {
                        Ok(resp) => Ok(Loop::Break((attempt, resp))).into_future().boxify(),
                        Err(err) => match retry_delay(attempt) {
                            None => Err(err).into_future().boxify(),
                            Some(dur) => delay_for(&remote, dur)
                                .map(move |()| Loop::Continue((attempt + 1, None)))
                                .boxify(),
                        },
                    }
                })
            }
        }).map(|(_, resp)| resp)
            .boxify()
    }

    fn put(&self, key: Self::Key, value: Self::ValueIn) -> Self::PutBlob {
        loop_fn(0, {
            let blobstore = self.blobstore.clone();
            let remote = self.remote.clone();
            let retry_delay = self.put_retry_delay.clone();
            move |attempt| {
                blobstore.put(key.clone(), value.clone()).from_err().then({
                    let remote = remote.clone();
                    let retry_delay = retry_delay.clone();
                    move |result| match result {
                        Ok(()) => Ok(Loop::Break(attempt)).into_future().boxify(),
                        Err(err) => match retry_delay(attempt) {
                            None => Err(err).into_future().boxify(),
                            Some(dur) => delay_for(&remote, dur)
                                .map(move |()| Loop::Continue(attempt + 1))
                                .boxify(),
                        },
                    }
                })
            }
        }).map(|_| ())
            .boxify()
    }
}
