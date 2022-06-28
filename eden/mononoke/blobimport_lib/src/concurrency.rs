/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::format_err;
use anyhow::Error;
use anyhow::Result;
use futures::compat::Future01CompatExt;
use futures_01_ext::BoxFuture;
use futures_01_ext::FutureExt;
use futures_old::sync::mpsc;
use futures_old::sync::oneshot;
use futures_old::Future;
use futures_old::IntoFuture;
use futures_old::Stream;
use tokio::runtime::Handle;

type Job<In, Out> = (In, oneshot::Sender<Result<Out>>);

/// JobProcessor allows for limiting concurrency for a particular kind of action, implemented in
/// the handler passed when instantiating a JobProcessor. The JobProcessor does not enforce any
/// limits on buffering of jobs: only on their execution! This is useful when different pieces of
/// code need to share an underlying resource but aren't modelled as an individual stream that can
/// be buffered.
pub struct JobProcessor<In, Out> {
    sender: mpsc::UnboundedSender<Job<In, Out>>,
}

impl<In, Out> JobProcessor<In, Out>
where
    In: Send + 'static,
    Out: Send + 'static,
{
    pub fn new<H>(handler: H, handle: &Handle, concurrency: usize) -> Result<Self>
    where
        H: Fn(In) -> BoxFuture<Out, Error> + Send + 'static,
    {
        // NOTE: This buffer is unbounded, because we allow buffering as many entries as possible
        // on this stream, we just don't process all of them at once. We do implicitly have some
        // limits on the size of this buffer set e.g. the number of concurrent changesets we
        // process.
        let (sender, receiver) = mpsc::unbounded::<Job<In, Out>>();

        let processor = receiver
            .map(move |(input, sender)| {
                handler(input).then(move |res| {
                    let _ = sender.send(res); // Don't kill the stream if one receiver is gone.
                    Ok(())
                })
            })
            .buffer_unordered(concurrency)
            .for_each(|()| Ok(()))
            .discard()
            .compat();

        handle.spawn(processor);

        Ok(Self { sender })
    }

    pub fn process(&self, input: In) -> impl Future<Item = Out, Error = Error> {
        let (sender, receiver) = oneshot::channel::<Result<Out>>();

        match self.sender.unbounded_send((input, sender)) {
            Ok(()) => receiver
                .into_future()
                .map_err(|e| format_err!("JobProcessor: Receiver failed: {:?}", e))
                .and_then(|r| r)
                .left_future(),
            Err(e) => Err(format_err!("JobProcessor Sender failed: {:?}", e))
                .into_future()
                .right_future(),
        }
    }
}
