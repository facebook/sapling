/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;
use std::sync::Weak;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::task::Poll;
use std::time::Duration;

use anyhow::Result;
use bytes::Bytes;
use futures::Future;
use futures::StreamExt;
use futures::stream;
use gotham_ext::response::ResponseStream;
use gotham_ext::response::ResponseTryStreamExt;
use tokio::sync::oneshot;
use tokio::time::timeout;

use super::with_packfile_generator;

struct Allocation {
    bytes: Option<Arc<Vec<u8>>>,
    dropped: Option<oneshot::Sender<()>>,
}

impl Drop for Allocation {
    fn drop(&mut self) {
        drop(self.bytes.take());
        if let Some(dropped) = self.dropped.take() {
            let _ = dropped.send(());
        }
    }
}

struct GeneratorProbe {
    started: oneshot::Receiver<()>,
    dropped: oneshot::Receiver<()>,
    release: oneshot::Sender<()>,
    allocation: Weak<Vec<u8>>,
}

impl GeneratorProbe {
    async fn wait_started(&mut self) {
        timeout(Duration::from_secs(2), &mut self.started)
            .await
            .expect("generator should start while its response is alive")
            .expect("generator should signal that it started");
    }

    async fn assert_cancelled(mut self) {
        let dropped_before_release = matches!(
            timeout(Duration::from_secs(1), &mut self.dropped).await,
            Ok(Ok(()))
        );
        let retained_before_release = self.allocation.upgrade().is_some();
        let _ = self.release.send(());
        if !dropped_before_release {
            timeout(Duration::from_secs(2), &mut self.dropped)
                .await
                .expect("release the legacy detached generator before failing")
                .expect("released generator should drop its allocation");
        }
        assert!(
            dropped_before_release && !retained_before_release,
            "dropped_before_release={dropped_before_release}, retained_before_release={retained_before_release}"
        );
    }
}

fn generator_probe() -> (impl Future<Output = Result<()>>, GeneratorProbe) {
    let (started_tx, started) = oneshot::channel();
    let (dropped_tx, dropped) = oneshot::channel();
    let (release, release_rx) = oneshot::channel();
    let bytes = Arc::new(vec![0_u8; 1024 * 1024]);
    let allocation = Arc::downgrade(&bytes);
    let held = Allocation {
        bytes: Some(bytes),
        dropped: Some(dropped_tx),
    };
    let generator = async move {
        let _held = held;
        let _ = started_tx.send(());
        let _ = release_rx.await;
        Ok(())
    };
    (
        generator,
        GeneratorProbe {
            started,
            dropped,
            release,
            allocation,
        },
    )
}

#[tokio::test]
async fn unpolled_response_drop_cancels_packfile_generator() {
    let (generator, mut probe) = generator_probe();
    let response = Box::pin(with_packfile_generator(
        stream::pending::<Result<Bytes>>(),
        generator,
    ));
    probe.wait_started().await;
    drop(response);
    probe.assert_cancelled().await;
}

#[tokio::test]
async fn partial_response_drop_cancels_packfile_generator() {
    let (generator, mut probe) = generator_probe();
    let header = Bytes::from_static(b"header");
    let chunks = stream::iter([Ok(header.clone())]).chain(stream::pending());
    let mut response = Box::pin(with_packfile_generator(chunks, generator));
    probe.wait_started().await;
    assert_eq!(response.next().await.unwrap().unwrap(), header);
    assert!(
        probe.allocation.upgrade().is_some(),
        "a live response must keep its producer alive"
    );
    drop(response);
    probe.assert_cancelled().await;
}

#[tokio::test]
async fn response_completion_cancels_unused_packfile_generator() {
    let (generator, mut probe) = generator_probe();
    let mut response = Box::pin(with_packfile_generator(
        stream::empty::<Result<Bytes>>(),
        generator,
    ));
    probe.wait_started().await;
    assert!(response.next().await.is_none());
    probe.assert_cancelled().await;
}

#[tokio::test]
async fn error_eof_cancels_generator_while_response_is_retained() {
    let (generator, mut probe) = generator_probe();
    let chunks = stream::iter([Err(anyhow::anyhow!("injected body failure"))]);
    let mut response = Box::pin(
        ResponseStream::new(with_packfile_generator(chunks, generator))
            .end_on_err::<anyhow::Error>(),
    );
    probe.wait_started().await;
    assert!(response.next().await.is_none());
    probe.assert_cancelled().await;
    assert!(response.next().await.is_none());
}

#[tokio::test]
async fn response_wrapper_fuses_without_polling_terminated_source() {
    for fail in [false, true] {
        let polls = Arc::new(AtomicUsize::new(0));
        let observed = polls.clone();
        let source = stream::poll_fn(move |cx| match observed.fetch_add(1, Ordering::SeqCst) {
            0 => {
                cx.waker().wake_by_ref();
                Poll::Pending
            }
            1 => Poll::Ready(Some(Ok(Bytes::from_static(b"header")))),
            2 if fail => Poll::Ready(Some(Err(anyhow::anyhow!("body failure")))),
            2 => Poll::Ready(None),
            _ => panic!("terminated source must not be polled again"),
        });
        let (generator, mut probe) = generator_probe();
        let mut response = Box::pin(with_packfile_generator(source, generator));
        probe.wait_started().await;
        assert_eq!(
            response.next().await.unwrap().unwrap(),
            Bytes::from_static(b"header")
        );
        assert!(
            probe.allocation.upgrade().is_some(),
            "Pending and successful chunks must retain the producer"
        );
        if fail {
            assert!(response.next().await.unwrap().is_err());
        } else {
            assert!(response.next().await.is_none());
        }
        probe.assert_cancelled().await;
        assert!(response.next().await.is_none());
        assert!(response.next().await.is_none());
        assert_eq!(polls.load(Ordering::SeqCst), 3);
    }
}

#[tokio::test]
async fn successful_response_preserves_chunks_and_cancels_unused_generator() {
    let (generator, mut probe) = generator_probe();
    let chunks = [Bytes::from_static(b"header"), Bytes::from_static(b"pack")];
    let mut response = Box::pin(
        ResponseStream::new(with_packfile_generator(
            stream::iter(chunks.clone().map(Ok)),
            generator,
        ))
        .end_on_err::<anyhow::Error>(),
    );
    probe.wait_started().await;
    for chunk in chunks {
        assert_eq!(response.next().await, Some(chunk));
    }
    assert!(response.next().await.is_none());
    probe.assert_cancelled().await;
}
