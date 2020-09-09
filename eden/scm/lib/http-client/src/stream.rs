/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::marker::PhantomData;
use std::mem;
use std::pin::Pin;

use futures::{
    prelude::*,
    ready,
    task::{Context, Poll},
};
use pin_project::pin_project;
use serde::de::DeserializeOwned;
use serde_cbor::Deserializer;

const DEFAULT_BUFFER_SIZE: usize = 1024 * 1024;

/// A wrapper around a `TryStream` of bytes that will attempt to deserialize
/// CBOR-encoded values from the data stream as it is received.
///
/// Data from the underlying stream will be buffered until a sufficient amount
/// has accumulated before attempting deserialization. This is important for
/// efficiency since libcurl will typically return a stream of small packets.
#[pin_project]
#[must_use = "streams do nothing unless polled"]
pub struct CborStream<T, S, B, E> {
    #[pin]
    incoming: BufferedStream<S, B, E>,
    buffer: Vec<u8>,
    position: usize,
    terminated: bool,
    _phantom: PhantomData<(T, B, E)>,
}

impl<T, S, B, E> CborStream<T, S, B, E> {
    pub(crate) fn new(body: S) -> Self {
        Self::with_buffer_size(body, DEFAULT_BUFFER_SIZE)
    }

    pub(crate) fn with_buffer_size(body: S, size: usize) -> Self {
        Self {
            incoming: BufferedStream::new(body, size),
            buffer: Vec::new(),
            position: 0,
            terminated: false,
            _phantom: PhantomData,
        }
    }
}

impl<T, S, B, E> Stream for CborStream<T, S, B, E>
where
    T: DeserializeOwned,
    S: Stream<Item = Result<B, E>> + Send + 'static,
    B: AsRef<[u8]>,
    E: From<serde_cbor::Error>,
{
    type Item = Result<T, E>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        let mut this = self.project();

        if *this.terminated {
            return Poll::Ready(None);
        }

        loop {
            // Attempt to deserialize a single item from the buffer.
            let mut de = Deserializer::from_slice(&this.buffer[*this.position..]);
            match T::deserialize(&mut de) {
                Ok(value) => {
                    *this.position += de.byte_offset();
                    return Poll::Ready(Some(Ok(value)));
                }
                Err(e) if !e.is_eof() => {
                    *this.terminated = true;
                    return Poll::Ready(Some(Err(E::from(e))));
                }
                _ => {}
            }

            // At this point we've deserialized most of the data in the buffer.
            // Any remaining trailing data is the prefix of a single, incomplete
            // item. We should move it to the front of the buffer to reclaim the
            // space from the items that have already been deserialized.
            let pos = *this.position;
            let len = this.buffer.len() - pos;
            for i in 0..len {
                this.buffer[i] = this.buffer[pos + i];
            }
            this.buffer.truncate(len);
            *this.position = 0;

            // Poll the underlying stream for more incoming data.
            match ready!(this.incoming.as_mut().poll_next(cx)) {
                Some(Ok(chunk)) => this.buffer.extend_from_slice(chunk.as_ref()),
                Some(Err(e)) => {
                    return Poll::Ready(Some(Err(e)));
                }
                None => {
                    return Poll::Ready(None);
                }
            }
        }
    }
}

/// A wrapper around a byte stream that buffers the data until it exceeds
/// the given size threshold. This can improve the efficiency of processing
/// data from bytes streams that consist largely of small chunks.
#[pin_project]
#[must_use = "streams do nothing unless polled"]
pub struct BufferedStream<S, B, E> {
    #[pin]
    inner: S,
    buffer: Vec<u8>,
    threshold: usize,
    done: bool,
    _phantom: PhantomData<(B, E)>,
}

impl<S, B, E> BufferedStream<S, B, E> {
    pub(crate) fn new(inner: S, n: usize) -> Self {
        Self {
            inner,
            buffer: Vec::with_capacity(n),
            threshold: n,
            done: false,
            _phantom: PhantomData,
        }
    }
}

impl<S, B, E> Stream for BufferedStream<S, B, E>
where
    S: Stream<Item = Result<B, E>> + Send + 'static,
    B: AsRef<[u8]>,
{
    type Item = Result<Vec<u8>, E>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        let mut this = self.project();

        if *this.done {
            return Poll::Ready(None);
        }

        while this.buffer.len() < *this.threshold {
            match ready!(this.inner.as_mut().poll_next(cx)) {
                Some(Ok(bytes)) => this.buffer.extend_from_slice(bytes.as_ref()),
                Some(Err(e)) => return Poll::Ready(Some(Err(e))),
                None => {
                    *this.done = true;
                    break;
                }
            }
        }

        let buf = mem::replace(this.buffer, Vec::with_capacity(*this.threshold));
        Poll::Ready(Some(Ok(buf)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use anyhow::{anyhow, Result};
    use serde::{Deserialize, Serialize};

    #[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
    struct TestItem(String);

    impl TestItem {
        fn new(s: &str) -> Self {
            TestItem(s.into())
        }
    }

    #[tokio::test]
    async fn test_single_item() -> Result<()> {
        let item = TestItem::new("hello");
        let bytes = serde_cbor::to_vec(&item)?;

        let byte_stream = stream::once(async {
            let res: Result<_> = Ok(bytes); // Need to assign for type hint.
            res
        });
        let mut cbor_stream = Box::pin(CborStream::new(byte_stream));

        let res = cbor_stream.try_next().await?;
        assert_eq!(res, Some(item));

        Ok(())
    }

    #[tokio::test]
    async fn test_single_error() -> Result<()> {
        let byte_stream = stream::once(async {
            let res: Result<Vec<u8>> = Err(anyhow!("error")); // Need to assign for type hint.
            res
        });
        let mut cbor_stream = Box::pin(CborStream::new(byte_stream));

        let res: Result<Option<TestItem>> = cbor_stream.try_next().await;
        assert!(res.is_err());

        Ok(())
    }

    #[tokio::test]
    async fn test_complete_items() -> Result<()> {
        let items = vec![
            TestItem::new("foo"),
            TestItem::new("bar"),
            TestItem::new("baz"),
        ];

        let incoming = items
            .clone()
            .into_iter()
            .map(|v| -> Result<Vec<u8>> { Ok(serde_cbor::to_vec(&v)?) });
        let cbor_stream = Box::pin(CborStream::new(stream::iter(incoming)));

        let res: Vec<TestItem> = cbor_stream.try_collect().await?;
        assert_eq!(res, items);

        Ok(())
    }

    #[tokio::test]
    async fn test_mid_stream_error() -> Result<()> {
        let items = vec![TestItem::new("foo"), TestItem::new("bar")];

        let bytes_and_errors = vec![
            Ok(serde_cbor::to_vec(&items[0])?),
            Err(anyhow!("mid-stream error")),
            Ok(serde_cbor::to_vec(&items[1])?),
        ];
        let mut cbor_stream = Box::pin(CborStream::new(stream::iter(bytes_and_errors)));

        let res1: Result<Option<TestItem>> = cbor_stream.try_next().await;
        let res2: Result<Option<TestItem>> = cbor_stream.try_next().await;
        let res3: Result<Option<TestItem>> = cbor_stream.try_next().await;

        // Due to internal buffering, the error will be returned first
        // even though it was second in the input stream.
        assert!(res1.is_err());
        assert_eq!(res2?, Some(items[0].clone()));
        assert_eq!(res3?, Some(items[1].clone()));

        Ok(())
    }

    #[tokio::test]
    async fn test_partial_item() -> Result<()> {
        let items = vec![TestItem::new("hello"), TestItem::new("world")];

        let mut incoming: Vec<Result<Vec<u8>>> = Vec::new();
        for i in &items {
            let bytes = serde_cbor::to_vec(&i)?;

            let mid = bytes.len() / 2;
            let prefix = bytes[..mid].to_vec();
            let suffix = bytes[mid..].to_vec();

            incoming.push(Ok(prefix));
            incoming.push(Ok(suffix));
        }
        let cbor_stream = Box::pin(CborStream::new(stream::iter(incoming)));

        let res: Vec<TestItem> = cbor_stream.try_collect().await?;
        assert_eq!(res, items);

        Ok(())
    }

    #[tokio::test]
    async fn test_concatenated_items() -> Result<()> {
        let items = vec![
            TestItem::new("test1"),
            TestItem::new("test2"),
            TestItem::new("test3"),
            TestItem::new("test4"),
            TestItem::new("test5"),
        ];

        let mut concat = Vec::new();
        for i in &items {
            concat.extend(serde_cbor::to_vec(&i)?);
        }

        let mid = concat.len() / 2;
        let chunk1 = concat[..mid].to_vec();
        let chunk2 = concat[mid..].to_vec();
        let chunks: Vec<Result<Vec<u8>>> = vec![Ok(chunk1), Ok(chunk2)];
        let cbor_stream = Box::pin(CborStream::new(stream::iter(chunks)));

        let res: Vec<TestItem> = cbor_stream.try_collect().await?;
        assert_eq!(res, items);

        Ok(())
    }

    #[tokio::test]
    async fn test_buffered_stream() -> Result<()> {
        let words: Vec<Result<Vec<u8>>> = vec![
            Ok(b"The ".to_vec()),
            Ok(b"quick ".to_vec()),
            Ok(b"brown ".to_vec()),
            Ok(b"fox ".to_vec()),
            Ok(b"jumped ".to_vec()),
            Ok(b"over ".to_vec()),
            Ok(b"the ".to_vec()),
            Ok(b"lazy ".to_vec()),
            Ok(b"dog".to_vec()),
        ];

        let input = stream::iter(words);
        let buf_size = 20;
        let buffered = Box::pin(BufferedStream::new(input, buf_size));
        let output = buffered.try_collect::<Vec<_>>().await?;

        assert_eq!(output.len(), 3);

        // Exactly 20 bytes long.
        assert_eq!(&*output[0], &b"The quick brown fox "[..]);

        // 21 bytes long because last chunk pushed buffer over limit.
        assert_eq!(&*output[1], &b"jumped over the lazy "[..]);

        // Short last item.
        assert_eq!(&*output[2], &b"dog"[..]);

        Ok(())
    }
}
