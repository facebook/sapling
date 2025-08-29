/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::marker::PhantomData;
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

const MEDIUM_BUFFER_SIZE: usize = 1024 * 1024;
const SMALL_BUFFER_SIZE: usize = 1024 * 8;

/// A wrapper around a `TryStream` of bytes that will attempt to deserialize
/// CBOR-encoded values from the data stream as it is received.
///
/// Data from the underlying stream will be buffered until a sufficient amount
/// has accumulated before attempting deserialization. This is important for
/// efficiency since libcurl will typically return a stream of small packets.
#[pin_project]
#[must_use = "streams do nothing unless polled"]
pub struct CborStream<T, S, E> {
    #[pin]
    incoming: S,
    incoming_done: bool,
    buffer: Vec<u8>,
    deserializing: minibytes::Bytes,
    threshold: usize,
    position: usize,
    terminated: bool,
    _phantom: PhantomData<(T, E)>,
}

impl<T, S, E> CborStream<T, S, E> {
    pub(crate) fn new(body: S) -> Self {
        Self {
            incoming: body,
            incoming_done: false,
            buffer: Vec::new(),
            deserializing: minibytes::Bytes::new(),
            threshold: SMALL_BUFFER_SIZE,
            position: 0,
            terminated: false,
            _phantom: PhantomData,
        }
    }
}

impl<T, S, E> Stream for CborStream<T, S, E>
where
    T: DeserializeOwned,
    S: Stream<Item = Result<Vec<u8>, E>> + Send + 'static,
    E: From<CborStreamError>,
{
    type Item = Result<T, E>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        let mut this = self.project();

        while !(*this.terminated
            || *this.incoming_done
                && *this.position == this.buffer.len().max(this.deserializing.len()))
        {
            if this.deserializing.is_empty() {
                // Before we try deserializing, shuffle this.buffer to this.deserializing.
                // This lets us use as_deserialize_hint() for zero copy Bytes deserializing.
                *this.deserializing = std::mem::take(this.buffer).into();
            }

            let deserialized = this.deserializing.as_deserialize_hint(|| {
                let mut de = Deserializer::from_slice(&this.deserializing[*this.position..]);
                T::deserialize(&mut de).map(|t| (t, de.byte_offset()))
            });

            // Attempt to deserialize a single item from the buffer.
            match deserialized {
                Ok((value, byte_offset)) => {
                    *this.position += byte_offset;

                    // Reset the buffer threshold in case we had expanded it to fit a very large
                    // value. Do not shrink it to smaller size than MEDIUM_BUFFER_SIZE, lest
                    // there is a traffic pattern rather than a single large item.
                    *this.threshold = (*this.threshold).min(MEDIUM_BUFFER_SIZE);

                    return Poll::Ready(Some(Ok(value)));
                }
                Err(e) if !e.is_eof() => {
                    // Got an error other than ran-out-of-bytes - end stream.
                    *this.terminated = true;
                    let e = CborStreamError::CborError(e);
                    return Poll::Ready(Some(Err(E::from(e))));
                }
                _ => {
                    // We got EOF - we don't have enough data to deserialize the entire item.

                    // Check if underlying stream is done.
                    if *this.incoming_done {
                        let e = CborStreamError::TrailingData(
                            this.deserializing.len() - *this.position,
                        );
                        return Poll::Ready(Some(Err(E::from(e))));
                    }

                    match std::mem::take(this.deserializing).take_vec() {
                        Ok(vec) => {
                            // We recovered the buffer zero-copy - reuse the entire thing.
                            *this.buffer = vec;
                        }
                        Err(bytes) => {
                            // Couldn't recover the buffer (because T::deserialize retained a slice).
                            // Copy unused part into a new buffer.
                            *this.buffer = Vec::with_capacity(*this.threshold);
                            this.buffer.extend_from_slice(&bytes[*this.position..]);
                            *this.position = 0;
                        }
                    }
                }
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
            } else if this.buffer.len() >= *this.threshold {
                // If we get here, that means that we're attempting to deserialize a single value
                // that is larger than the current buffer size. If the value is significantly larger
                // than the current buffer size, this will result in accidentally quadratic behavior
                // as we repeatedly attempt to deserialize the partial value whenever a new chunk
                // comes in.
                //
                // To prevent this situation, whenever we encounter an item that exceeds the current
                // buffer size, we simply double it. This means that we'll only need to do O(log(n))
                // deserialization attempts for very large values.
                //
                // We will skip doubling small buffers, and go strait to 1MB buffer size.
                *this.threshold = MEDIUM_BUFFER_SIZE.max(2 * *this.threshold);
            }

            // Poll the underlying stream for more incoming data, reading until we have self.threshold data.
            while this.buffer.len() - *this.position < *this.threshold {
                match ready!(this.incoming.as_mut().poll_next(cx)) {
                    Some(Ok(chunk)) => {
                        if this.buffer.is_empty() && this.buffer.capacity() < chunk.len() {
                            // Optimize the fetching-single-item case to not copy the data. For
                            // example, fetching a single tree/file will often come in via a single
                            // data chunk from curl.
                            *this.buffer = chunk;
                        } else {
                            this.buffer.extend_from_slice(&chunk);
                        }
                    }
                    Some(Err(e)) => return Poll::Ready(Some(Err(e))),
                    None => {
                        // Underlying stream is done.
                        *this.incoming_done = true;
                        break;
                    }
                }
            }
        }

        Poll::Ready(None)
    }
}

#[cfg(test)]
mod tests {
    use anyhow::Result;
    use anyhow::anyhow;
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
        let items = [TestItem::new("foo"), TestItem::new("bar")];

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
}
