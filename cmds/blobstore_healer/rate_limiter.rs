// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use cloned::cloned;
use failure_ext::Error;
use futures::{
    future::{self, lazy, IntoFuture},
    prelude::*,
    sync::{mpsc, oneshot},
};
use futures_ext::{spawn_future, BoxFuture, FutureExt};
use tokio;

#[derive(Clone)]
pub struct RateLimiter {
    ensure_worker_scheduled: future::Shared<BoxFuture<(), ()>>,
    worker: mpsc::UnboundedSender<BoxFuture<(), Error>>,
}

impl RateLimiter {
    pub fn new(max_num_of_concurrent_futures: usize) -> Self {
        let (send, recv) = mpsc::unbounded();

        let ensure_worker_scheduled = lazy(move || {
            tokio::spawn(
                recv.then(move |work| {
                    let work = match work {
                        Ok(work) => work,
                        Err(()) => Ok(()).into_future().boxify(),
                    };
                    Ok(spawn_future(work).then(|_| Ok(())))
                })
                .buffer_unordered(max_num_of_concurrent_futures)
                .for_each(|()| Ok(()))
                .then(|_: Result<(), !>| -> Result<(), ()> {
                    // The Err is !, this code is to guarantee that a Worker will never stop
                    Ok(())
                }),
            );
            Ok(())
        })
        .boxify()
        .shared();

        Self {
            ensure_worker_scheduled,
            worker: send,
        }
    }

    pub fn execute<F, I, E>(&self, work: F) -> BoxFuture<I, E>
    where
        F: Future<Item = I, Error = E> + Send + 'static,
        I: Send + 'static,
        E: From<::futures::Canceled> + Send + 'static,
    {
        cloned!(self.worker);

        self.ensure_worker_scheduled
            .clone()
            .then(move |scheduling_result| {
                scheduling_result.expect("The scheduling cannot fail");

                let (send, recv) = oneshot::channel();
                worker
                    .unbounded_send(
                        work.then(move |result| {
                            let _ = send.send(result);
                            Ok(())
                        })
                        .boxify(),
                    )
                    .expect("This send should never fail since the receiver is always alive");

                recv.from_err().and_then(|result| result)
            })
            .boxify()
    }
}
