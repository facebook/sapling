/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::marker::PhantomData;
use std::mem;
use std::pin::Pin;

use futures::prelude::*;
use futures::ready;
use futures::task::Context;
use futures::task::Poll;
use pin_project::pin_project;
use serde::de::DeserializeOwned;
use serde_cbor::Deserializer;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum CborStreamError {
    #[error(transparent)]
    CborError(serde_cbor::Error),

    #[error("Unexpected trailing data found at end of CBOR stream ({0} bytes)")]
    TrailingData(usize),
}

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
    threshold: usize,
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
            threshold: size,
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
    E: From<CborStreamError>,
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
                    // Reset the buffer threshold in case we had expanded it to fit a large value.
                    this.incoming.as_mut().set_threshold(*this.threshold);
                    return Poll::Ready(Some(Ok(value)));
                }
                Err(e) if !e.is_eof() => {
                    *this.terminated = true;
                    let e = CborStreamError::CborError(e);
                    return Poll::Ready(Some(Err(E::from(e))));
                }
                _ => {}
            }


            let pos = *this.position;
            if pos > 0 {
                // At this point we've deserialized most of the data in the buffer. Any remaining
                // trailing data is the prefix of a single, incomplete item. We should move it to
                // the front of the buffer to reclaim the space from the items that have already
                // been deserialized.
                let len = this.buffer.len() - pos;
                for i in 0..len {
                    this.buffer[i] = this.buffer[pos + i];
                }
                this.buffer.truncate(len);
                *this.position = 0;
            } else if this.buffer.len() >= this.incoming.threshold() {
                // If we get here, that means that we're attempting to deserialize a single value
                // that is larger than the current buffer size. If the value is significantly larger
                // than the current buffer size, this will result in accidentally quadratic behavior
                // as we repeatedly attempt to deserialize the partial value whenever a new chunk
                // comes in.
                //
                // To prevent this situation, whenever we encounter an item that exceeds the current
                // buffer size, we simply double it. This means that we'll only need to do O(log(n))
                // deserialization attempts for very large values.
                let new_threshold = 2 * this.incoming.threshold();
                this.incoming.as_mut().set_threshold(new_threshold);
            }

            // Poll the underlying stream for more incoming data.
            match ready!(this.incoming.as_mut().poll_next(cx)) {
                Some(Ok(chunk)) => this.buffer.extend_from_slice(chunk.as_ref()),
                Some(Err(e)) => {
                    return Poll::Ready(Some(Err(e)));
                }
                None => {
                    // At this point the stream is complete, so we expect to have read everything.
                    // If we haven't, then something went wrong during the transfer and the data
                    // we're handling seems corrupted: raise an error in that case.
                    if !this.buffer.is_empty() {
                        let e = CborStreamError::TrailingData(this.buffer.len());
                        return Poll::Ready(Some(Err(E::from(e))));
                    }
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

    fn threshold(&self) -> usize {
        self.threshold
    }

    /// Update the buffer size in the middle of polling the stream.
    fn set_threshold(self: Pin<&mut Self>, threshold: usize) {
        *self.project().threshold = threshold;
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
    use anyhow::anyhow;
    use anyhow::Result;
    use serde::Deserialize;
    use serde::Serialize;

    use super::*;

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
    async fn test_truncated_data() -> Result<()> {
        // Add a valid chunk, but truncate it, though in practice, the truncated data could be
        // stuff that looks like it could be CBOR but doesn't decode properly (like an error
        // string).
        let mut chunk = serde_cbor::to_vec(&TestItem::new("test1"))?;
        chunk.pop();
        let len = chunk.len();

        let chunks: Vec<Result<Vec<u8>, CborStreamError>> = vec![Ok(chunk)];
        let mut cbor_stream = Box::pin(CborStream::new(stream::iter(chunks)));

        let res1: Result<Option<TestItem>, _> = cbor_stream.try_next().await;

        match res1 {
            Err(CborStreamError::TrailingData(l)) if l == len => {}
            other => panic!("Unexpected result on trailing data: {:?}", other),
        };

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
