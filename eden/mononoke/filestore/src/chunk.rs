/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use anyhow::Result;
use bytes::Bytes;
use bytes::BytesMut;
use futures::future::BoxFuture;
use futures::future::FutureExt;
use futures::future::TryFutureExt;
use futures::stream::BoxStream;
use futures::stream::Stream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use futures::task::Context;
use futures::task::Poll;
use std::fmt;
use std::fmt::Debug;
use std::pin::Pin;

use crate::expected_size::ExpectedSize;

#[must_use = "streams do nothing unless polled"]
#[pin_project::pin_project]
#[derive(Debug)]
pub struct ChunkStream<S> {
    #[pin]
    stream: S,
    state: ChunkStreamState,
}

#[derive(Debug)]
struct ChunkStreamState {
    chunk_size: usize,
    buff: BytesMut,
    emitted: bool,
    had_data: bool,
    done: bool,
}

impl<S> ChunkStream<S> {
    pub fn new(stream: S, chunk_size: usize) -> ChunkStream<S> {
        assert!(chunk_size > 0);

        ChunkStream {
            stream,
            state: ChunkStreamState {
                chunk_size,
                buff: BytesMut::with_capacity(chunk_size),
                emitted: false,
                had_data: false,
                done: false,
            },
        }
    }
}

impl<S, E> Stream for ChunkStream<S>
where
    S: Stream<Item = Result<Bytes, E>>,
{
    type Item = S::Item;

    fn poll_next(self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut proj = self.project();

        if proj.state.done {
            return Poll::Ready(None);
        }

        loop {
            if proj.state.buff.len() >= proj.state.chunk_size {
                // We've buffered more data than we need. Emit some.
                proj.state.emitted = true;
                let chunk = proj.state.buff.split_to(proj.state.chunk_size).freeze();
                return Poll::Ready(Some(Ok(chunk)));
            }

            // We need more data. Poll for some! Note the as_mut() here is used to reborrow the
            // stream and avoid moving it into the loop iteration.

            match futures::ready!(proj.stream.as_mut().poll_next(ctx)) {
                Some(Ok(bytes)) => {
                    // We got more data. Extend our buffer, then see if that is enough to return. Note
                    // that extend_from slice implicitly extends our BytesMut.
                    proj.state.had_data = true;
                    proj.state.buff.extend_from_slice(&bytes);
                    continue;
                }
                Some(Err(e)) => {
                    return Poll::Ready(Some(Err(e)));
                }
                None => {
                    // Fallthrough
                }
            };

            // No more data is coming.

            proj.state.done = true;

            // Return whatever we have left. However, we need to be a little careful to handle
            // empty data here.
            //
            // If our buffer happens to just be empty, but we emitted data, that just means our
            // data was disivible by our chunk size, and we are done.
            //
            // However, if our buffer is empty, but we never emitted, then we have two possible
            // cases to handle:
            //
            // - Our underlying stream was empty Bytes. In this case, we should return empty Bytes
            // too (we're returning a representation of the underlying content, chunked).
            //
            // - Our underlying stream was empty. In this case, we shouldn't return anything.

            let out = if !proj.state.buff.is_empty() || (proj.state.had_data && !proj.state.emitted)
            {
                // We did have some buffered data. Emit that.
                proj.state.emitted = true;
                let chunk = std::mem::replace(&mut proj.state.buff, BytesMut::new()).freeze();
                Poll::Ready(Some(Ok(chunk)))
            } else {
                // We have no more buffered data. We're done.
                Poll::Ready(None)
            };

            return out;
        }
    }
}

pub enum Chunks<'a> {
    Inline(BoxFuture<'a, Result<Bytes, Error>>),
    Chunked(ExpectedSize, BoxStream<'a, Result<Bytes, Error>>),
}

impl Debug for Chunks<'_> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Chunks::Inline(_) => write!(f, "Chunks::Inline(..)"),
            Chunks::Chunked(size, _) => write!(f, "Chunks::Chunked({:?}, ...)", size),
        }
    }
}

/// Chunk a stream of incoming data for storage. We use the incoming size hint to decide whether
/// to chunk.
pub fn make_chunks<'a, S>(
    data: S,
    expected_size: ExpectedSize,
    chunk_size: Option<u64>,
) -> Chunks<'a>
where
    S: Stream<Item = Result<Bytes, Error>> + Send + 'a,
{
    // NOTE: We stop reading if the stream we are provided exceeds the expected_size we were given.
    // While we do check later that the stream matches *exactly* the size we were given, doing this
    // check lets us bail early (and e.g. ensures that if we are told something is 1 byte but it
    // actually is 1TB, we don't try to buffer the whole 1TB).
    let limit = {
        let mut observed_size: u64 = 0; // This moves into the closure below and serves as its state.
        move |chunk: Result<Bytes, Error>| {
            // NOTE: unwrap() will fail if we have a Bytes whose length is too large to fit in a u64.
            // We presumably don't have such Bytes in memory!
            let chunk = chunk?;
            observed_size += u64::try_from(chunk.len()).unwrap();
            expected_size.check_less(observed_size)?;
            Result::<_, Error>::Ok(chunk)
        }
    };

    let data = data.map(limit);

    match chunk_size {
        Some(chunk_size) if expected_size.should_chunk(chunk_size) => {
            let stream = ChunkStream::new(data, chunk_size as usize);
            Chunks::Chunked(expected_size, stream.boxed())
        }
        _ => {
            let fut = data
                .try_fold(
                    expected_size.new_buffer(),
                    |mut bytes, incoming| async move {
                        bytes.extend_from_slice(incoming.as_ref());
                        Result::<_, Error>::Ok(bytes)
                    },
                )
                .map_ok(BytesMut::freeze)
                .boxed();
            Chunks::Inline(fut)
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    use assert_matches::assert_matches;
    use futures::stream;
    use quickcheck::quickcheck;
    use tokio::runtime::Runtime;

    #[test]
    fn test_make_chunks_no_chunk_size() {
        let in_stream = stream::empty();

        match make_chunks(in_stream, ExpectedSize::new(10), None) {
            Chunks::Inline(_) => {}
            c => panic!("Did not expect {:?}", c),
        };
    }

    #[test]
    fn test_make_chunks_no_chunking() {
        let in_stream = stream::empty();

        match make_chunks(in_stream, ExpectedSize::new(10), Some(100)) {
            Chunks::Inline(_) => {}
            c => panic!("Did not expect {:?}", c),
        };
    }

    #[test]
    fn test_make_chunks_no_chunking_limit() {
        let in_stream = stream::empty();

        match make_chunks(in_stream, ExpectedSize::new(100), Some(100)) {
            Chunks::Inline(_) => {}
            c => panic!("Did not expect {:?}", c),
        };
    }

    #[test]
    fn test_make_chunks_chunking() {
        let in_stream = stream::empty();

        match make_chunks(in_stream, ExpectedSize::new(1000), Some(100)) {
            Chunks::Chunked(h, _) if h.check_equals(1000).is_ok() => {}
            c => panic!("Did not expect {:?}", c),
        };
    }

    #[tokio::test]
    async fn test_make_chunks_overflow_inline() {
        // Make chunks buffers if we expect content that is small enough to fit the chunk size.
        // However, if we get more content than that, we should stop.

        let chunks = vec![
            Bytes::from(vec![1; 5]),
            Bytes::from(vec![1; 5]),
            Bytes::from(vec![1; 5]),
            Bytes::from(vec![1; 5]),
        ];
        let in_stream = stream::iter(chunks).map(Ok);

        let fut = match make_chunks(in_stream, ExpectedSize::new(10), Some(100)) {
            c @ Chunks::Chunked(..) => panic!("Did not expect {:?}", c),
            Chunks::Inline(fut) => fut,
        };

        fut.await
            .expect_err("make_chunks should abort if the content does not end as advertised");
    }

    #[tokio::test]
    async fn test_make_chunks_overflow_chunked() {
        // If we get more content than advertises, abort.

        let chunks = vec![
            Bytes::from(vec![1; 5]),
            Bytes::from(vec![1; 5]),
            Bytes::from(vec![1; 5]),
            Bytes::from(vec![1; 5]),
        ];
        let in_stream = stream::iter(chunks).map(Ok);

        let fut = match make_chunks(in_stream, ExpectedSize::new(10), Some(1)) {
            Chunks::Chunked(_, stream) => stream.try_collect::<Vec<_>>(),
            c @ Chunks::Inline(..) => panic!("Did not expect {:?}", c),
        };

        fut.await
            .expect_err("make_chunks should abort if the content does not end as advertised");
    }

    #[tokio::test]
    async fn test_stream_of_empty_bytes() {
        // If we give ChunkStream a stream that contains empty bytes, then we should return one
        // chunk of empty bytes.
        let chunks = vec![Bytes::new()];
        let in_stream = stream::iter(chunks).map(Result::<_, ()>::Ok);
        let mut stream = ChunkStream::new(in_stream, 1);

        assert_eq!(stream.try_next().await, Ok(Some(Bytes::new())));
        assert_eq!(stream.try_next().await, Ok(None));
    }

    #[tokio::test]
    async fn test_stream_of_repeated_empty_bytes() {
        // If we give ChunkStream a stream that contains however many empty bytes, then we should
        // return a single chunk of empty bytes.

        let chunks = vec![Bytes::new(), Bytes::new()];
        let in_stream = stream::iter(chunks).map(Result::<_, ()>::Ok);
        let mut stream = ChunkStream::new(in_stream, 1);

        assert_eq!(stream.try_next().await, Ok(Some(Bytes::new())));
        assert_eq!(stream.try_next().await, Ok(None));
    }

    #[tokio::test]
    async fn test_empty_stream() {
        // If we give ChunkStream an empty stream, it should retun an empty stream.

        let in_stream = stream::iter(vec![]).map(Result::<_, ()>::Ok);
        let mut stream = ChunkStream::new(in_stream, 1);

        assert_eq!(stream.next().await, None);
    }

    #[tokio::test]
    async fn test_bigger_incoming_chunks() {
        // Explicitly test that ChunkStream handles splitting chunks.
        let chunks = vec![vec![1; 10], vec![1; 10]];
        assert!(do_check_chunk_stream(chunks, 5).await)
    }

    #[tokio::test]
    async fn test_smaller_incoming_chunks() {
        // Explicitly test that ChunkStream handles putting chunks together.
        let chunks = vec![vec![1; 10], vec![1; 10]];
        assert!(do_check_chunk_stream(chunks, 15).await)
    }

    #[tokio::test]
    async fn test_stream_exhaustion() {
        #[pin_project::pin_project]
        struct StrictStream {
            chunks: Vec<Bytes>,
            done: bool,
        }

        impl Stream for StrictStream {
            type Item = Result<Bytes, ()>;

            fn poll_next(
                mut self: Pin<&mut Self>,
                _: &mut Context<'_>,
            ) -> Poll<Option<Self::Item>> {
                if self.done {
                    panic!("StrictStream was done");
                }

                match self.chunks.pop() {
                    Some(b) => Poll::Ready(Some(Ok(b))),
                    None => {
                        self.done = true;
                        Poll::Ready(None)
                    }
                }
            }
        }

        let chunks = vec![
            Bytes::from(vec![1; 5]),
            Bytes::from(vec![1; 5]),
            Bytes::from(vec![1; 5]),
            Bytes::from(vec![1; 5]),
        ];

        let mut stream = ChunkStream::new(
            StrictStream {
                chunks,
                done: false,
            },
            10,
        );

        assert_matches!(stream.try_next().await, Ok(Some(_)));
        assert_matches!(stream.try_next().await, Ok(Some(_)));
        assert_matches!(stream.try_next().await, Ok(None));
        assert_matches!(stream.try_next().await, Ok(None));
    }

    async fn do_check_chunk_stream(in_chunks: Vec<Vec<u8>>, size: usize) -> bool {
        let in_chunks: Vec<Bytes> = in_chunks.into_iter().map(Bytes::from).collect();
        let chunk_stream = ChunkStream::new(
            stream::iter(in_chunks.clone()).map(Result::<_, ()>::Ok),
            size,
        );
        let out_chunks = chunk_stream.try_collect::<Vec<_>>().await.unwrap();

        let expected_bytes = in_chunks
            .iter()
            .fold(BytesMut::new(), |mut bytes, chunk| {
                bytes.extend_from_slice(chunk);
                bytes
            })
            .freeze();

        let got_bytes = out_chunks
            .iter()
            .fold(BytesMut::new(), |mut bytes, chunk| {
                bytes.extend_from_slice(chunk);
                bytes
            })
            .freeze();

        // The contents should be the same
        if expected_bytes != got_bytes {
            return false;
        }

        // If there were no contents, then just return that.
        if expected_bytes.is_empty() {
            return true;
        }

        // All chunks except for the last one must equal chunk size
        for chunk in out_chunks[0..out_chunks.len() - 1].iter() {
            if chunk.len() != size {
                return false;
            }
        }

        // The last chunk must smaller than the chunk size
        if out_chunks[out_chunks.len() - 1].len() > size {
            return false;
        }

        true
    }

    quickcheck! {
        fn check_chunk_stream(in_chunks: Vec<Vec<u8>>, size: u8) -> bool {
            let size = (size as usize) + 1; // Don't allow 0 as the size.
            let rt = Runtime::new().unwrap();
            rt.block_on(do_check_chunk_stream(in_chunks, size))
        }

        fn check_make_chunks_fut_joins(in_chunks: Vec<Vec<u8>>) -> bool {
            let rt = Runtime::new().unwrap();

            let in_chunks: Vec<Bytes> = in_chunks.into_iter().map(Bytes::from).collect();
            let in_stream = stream::iter(in_chunks.clone()).map(Ok);

            let expected_bytes = in_chunks.iter().fold(BytesMut::new(), |mut bytes, chunk| {
                bytes.extend_from_slice(chunk);
                bytes
            }).freeze();

            let len = expected_bytes.len() as u64;

            let fut = match make_chunks(in_stream, ExpectedSize::new(len), Some(len)) {
                Chunks::Inline(fut) => fut,
                c => panic!("Did not expect {:?}", c),
            };

            let out_bytes = rt.block_on(fut).unwrap();
            out_bytes == expected_bytes
        }
    }
}
