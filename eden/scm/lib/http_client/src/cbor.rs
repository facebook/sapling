/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::marker::PhantomData;
use std::pin::Pin;

use futures::{
    prelude::*,
    ready,
    task::{Context, Poll},
};
use pin_project::pin_project;
use serde::de::DeserializeOwned;
use serde_cbor::Deserializer;

/// A wrapper around a `TryStream` of bytes that will attempt to deserialize
/// CBOR-encoded values from the data stream as it is received.
#[pin_project]
#[must_use = "streams do nothing unless polled"]
pub struct CborStream<T, S, B, E> {
    #[pin]
    incoming: S,
    buffer: Vec<u8>,
    position: usize,
    terminated: bool,
    _phantom: PhantomData<(T, B, E)>,
}

impl<T, S, B, E> CborStream<T, S, B, E> {
    pub(crate) fn new(body: S) -> Self {
        Self {
            incoming: body,
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

            // Copy any remaining data to the front of the buffer.
            // (This data is the prefix of an incomplete serialized item.)
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

        assert_eq!(res1?, Some(items[0].clone()));
        assert!(res2.is_err());
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
}
