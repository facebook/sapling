/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::pin::Pin;

use anyhow::Error;
use bytes::Bytes;
use futures::channel::oneshot::Sender;
use futures::ready;
use futures::stream::Stream;
use futures::task::Context;
use futures::task::Poll;
use pin_project::pin_project;
use pin_project::pinned_drop;
use request_context_ext::CapturedRequestContext;

use super::error_meta::ErrorMeta;
use super::error_meta::ErrorMetaProvider;
use super::response_meta::BodyMeta;

pub trait Sizeable {
    fn size(&self) -> u64;
}

impl Sizeable for Bytes {
    fn size(&self) -> u64 {
        // NOTE: It is reasonable to unwrap here because we're not going to have buffers of bytes
        // that are larger than a u64.
        self.len().try_into().unwrap()
    }
}

/// A stream that will fire to the sender associated upon completing or being dropped. The Sender
/// will receive the amount of data that passed through the stream.
#[pin_project(PinnedDrop, project = SignalStreamProjection)]
pub struct SignalStream<S: ErrorMetaProvider<Error>> {
    #[pin]
    stream: S,
    sender: Option<Sender<BodyMeta>>,
    size_sent: u64,
    request_context: Option<CapturedRequestContext>,
}

impl<S> SignalStream<S>
where
    S: ErrorMetaProvider<Error>,
{
    pub fn new(stream: S, sender: Sender<BodyMeta>) -> Self {
        Self {
            stream,
            sender: Some(sender),
            size_sent: 0,
            request_context: CapturedRequestContext::capture(),
        }
    }
}

impl<S, T> Stream for SignalStream<S>
where
    S: Stream<Item = T> + ErrorMetaProvider<Error>,
    T: Sizeable,
{
    type Item = T;

    fn poll_next(self: Pin<&mut Self>, ctx: &mut Context) -> Poll<Option<Self::Item>> {
        let mut this = self.project();

        let poll = ready!(match this.request_context {
            Some(request_context) => {
                request_context.run(|| this.stream.as_mut().poll_next(ctx))
            }
            None => this.stream.as_mut().poll_next(ctx),
        });

        match poll {
            // We have an item: increment the amount of data we sent.
            Some(ref item) => *this.size_sent += item.size(),
            // No items left: signal our receiver.
            None => send_body_meta(this),
        }

        Poll::Ready(poll)
    }
}

#[pinned_drop]
impl<S> PinnedDrop for SignalStream<S>
where
    S: ErrorMetaProvider<Error>,
{
    fn drop(self: Pin<&mut Self>) {
        let this = self.project();
        send_body_meta(this);
    }
}

fn send_body_meta<S>(this: SignalStreamProjection<S>)
where
    S: ErrorMetaProvider<Error>,
{
    if let Some(sender) = this.sender.take() {
        let bytes_sent = *this.size_sent;
        let mut error_meta = ErrorMeta::new();

        this.stream.report_errors(&mut error_meta);

        let _ = sender.send(BodyMeta {
            bytes_sent,
            error_meta,
        });
    }
}

#[cfg(all(test, fbcode_build))]
mod tests {
    use std::hint::black_box;

    use context::CoreContext;
    use context::PerfCounterType;
    use fbinit::FacebookInit;
    use futures::StreamExt;
    use futures::channel::oneshot;
    use mononoke_macros::mononoke;
    use request_context_ext::with_fresh_request_context;

    use super::*;
    use crate::response::ResponseTryStreamExt;

    #[mononoke::fbinit_test]
    fn charges_stream_cpu_before_signalling_completion(fb: FacebookInit) {
        let ctx = CoreContext::new(fb);
        let (sender, receiver) = oneshot::channel();
        let stream = futures::executor::block_on(with_fresh_request_context(async {
            ctx.track_cpu_time();
            let stream = futures::stream::once(async {
                let mut value = 1u64;
                for _ in 0..100_000 {
                    value = black_box(value.wrapping_mul(6364136223846793005).wrapping_add(1));
                }
                Ok::<_, Error>(Bytes::from_static(b"file contents"))
            });
            SignalStream::new(stream.capture_first_err(), sender)
        }));
        let before = ctx.perf_counters().get_counter(PerfCounterType::CpuTimeNs);
        futures::executor::block_on(async {
            futures::pin_mut!(stream);
            assert_eq!(stream.next().await.unwrap(), "file contents");
            assert!(stream.next().await.is_none());
            assert_eq!(receiver.await.unwrap().bytes_sent, 13);
            assert!(ctx.perf_counters().get_counter(PerfCounterType::CpuTimeNs) > before);
        });
    }
}
