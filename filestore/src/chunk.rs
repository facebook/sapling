// Copyright (c) 2019-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use bytes::{Bytes, BytesMut};
use failure_ext::{Error, Result};
use futures::{
    stream::{AndThen, Concat2},
    try_ready, Async, Poll, Stream,
};
use std::convert::TryFrom;
use std::fmt::{self, Debug};

use crate::expected_size::ExpectedSize;

#[derive(Debug)]
#[must_use = "streams do nothing unless polled"]
pub struct ChunkStream<S> {
    stream: S,
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
            chunk_size,
            buff: BytesMut::with_capacity(chunk_size),
            emitted: false,
            had_data: false,
            done: false,
        }
    }
}

impl<S> Stream for ChunkStream<S>
where
    S: Stream<Item = Bytes>,
{
    type Item = Bytes;
    type Error = S::Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        if self.done {
            return Ok(Async::Ready(None));
        }

        loop {
            if self.buff.len() >= self.chunk_size {
                // We've buffered more data than we need. Emit some.
                self.emitted = true;
                let chunk = self.buff.split_to(self.chunk_size).freeze();
                return Ok(Async::Ready(Some(chunk)));
            }

            // We need more data. Poll for some!

            if let Some(bytes) = try_ready!(self.stream.poll()) {
                // We got more data. Extend our buffer, then see if that is enough to return. Note
                // that extend_from slice implicitly extends our BytesMut.
                self.had_data = true;
                self.buff.extend_from_slice(&bytes);
                continue;
            }

            // No more data is coming.

            self.done = true;

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

            let out = if self.buff.len() > 0 || (self.had_data && !self.emitted) {
                // We did have some buffered data. Emit that.
                self.emitted = true;
                let chunk = std::mem::replace(&mut self.buff, BytesMut::new()).freeze();
                Async::Ready(Some(chunk))
            } else {
                // We have no more buffered data. We're done.
                Async::Ready(None)
            };

            return Ok(out);
        }
    }
}

// NOTE: We concretely spell out the types the Chunk variants here. Boxifying would be a little
// nicer, but that causes us to lose the sense of whether the incoming Stream was Send or not.
// Spelling out the concrete types here lets us include the incoming Stream in them, and therefore
// allow callers to call the Filestore with a Stream that's either Send or non-Send, and get back a
// Future that is either Send or non-Send.
type LimitFn = Box<dyn FnMut(Bytes) -> Result<Bytes> + Send>;
type LimitStream<S> = AndThen<S, LimitFn, Result<Bytes>>;

pub type BufferedStream<S> = Concat2<LimitStream<S>>;
pub type ChunkedStream<S> = ChunkStream<LimitStream<S>>;

pub enum Chunks<S>
where
    S: Stream<Item = Bytes, Error = Error>,
{
    Inline(BufferedStream<S>),
    Chunked(ExpectedSize, ChunkedStream<S>),
}

impl<S> Debug for Chunks<S>
where
    S: Stream<Item = Bytes, Error = Error>,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Chunks::Inline(_) => write!(f, "Chunks::Inline(..)"),
            Chunks::Chunked(size, _) => write!(f, "Chunks::Chunked({:?}, ...)", size),
        }
    }
}

/// Chunk a stream of incoming data for storage. We use the incoming size hint to decide whether
/// to chunk.
pub fn make_chunks<S>(data: S, expected_size: ExpectedSize, chunk_size: Option<u64>) -> Chunks<S>
where
    S: Stream<Item = Bytes, Error = Error>,
{
    // NOTE: We stop reading if the stream we are provided exceeds the expected_size we were given.
    // While we do check later that the stream matches *exactly* the size we were given, doing this
    // check lets us bail early (and e.g. ensures that if we are told something is 1 byte but it
    // actually is 1TB, we don't try to buffer the whole 1TB).
    let limit = {
        let mut observed_size: u64 = 0; // This moves into the closure below and serves as its state.
        move |chunk: Bytes| {
            // NOTE: unwrap() will fail if we have a Bytes whose length is too large to fit in a u64.
            // We presumably don't have such Bytes in memory!
            observed_size += u64::try_from(chunk.len()).unwrap();
            expected_size.check_less(observed_size)?;
            Ok(chunk)
        }
    };

    let data = data.and_then(Box::new(limit) as LimitFn);

    match chunk_size {
        Some(chunk_size) if expected_size.should_chunk(chunk_size) => {
            let stream = ChunkStream::new(data, chunk_size as usize);
            Chunks::Chunked(expected_size, stream)
        }
        _ => {
            let fut = data.concat2();
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
        let in_stream = stream::iter_ok::<_, Error>(vec![]);

        match make_chunks(in_stream, ExpectedSize::new(10), None) {
            Chunks::Inline(_) => {}
            c => panic!("Did not expect {:?}", c),
        };
    }

    #[test]
    fn test_make_chunks_no_chunking() {
        let in_stream = stream::iter_ok::<_, Error>(vec![]);

        match make_chunks(in_stream, ExpectedSize::new(10), Some(100)) {
            Chunks::Inline(_) => {}
            c => panic!("Did not expect {:?}", c),
        };
    }

    #[test]
    fn test_make_chunks_no_chunking_limit() {
        let in_stream = stream::iter_ok::<_, Error>(vec![]);

        match make_chunks(in_stream, ExpectedSize::new(100), Some(100)) {
            Chunks::Inline(_) => {}
            c => panic!("Did not expect {:?}", c),
        };
    }

    #[test]
    fn test_make_chunks_chunking() {
        let in_stream = stream::iter_ok::<_, Error>(vec![]);

        match make_chunks(in_stream, ExpectedSize::new(1000), Some(100)) {
            Chunks::Chunked(h, _) if h.check_equals(1000).is_ok() => {}
            c => panic!("Did not expect {:?}", c),
        };
    }

    #[test]
    fn test_make_chunks_overflow_inline() {
        // Make chunks buffers if we expect content that is small enough to fit the chunk size.
        // However, if we get more content than that, we should stop.
        let mut rt = Runtime::new().unwrap();

        let chunks = vec![
            Bytes::from(vec![1; 5]),
            Bytes::from(vec![1; 5]),
            Bytes::from(vec![1; 5]),
            Bytes::from(vec![1; 5]),
        ];
        let in_stream = stream::iter_ok::<_, Error>(chunks);

        let fut = match make_chunks(in_stream, ExpectedSize::new(10), Some(100)) {
            c @ Chunks::Chunked(..) => panic!("Did not expect {:?}", c),
            Chunks::Inline(fut) => fut,
        };

        rt.block_on(fut)
            .expect_err("make_chunks should abort if the content does not end as advertised");
    }

    #[test]
    fn test_make_chunks_overflow_chunked() {
        // If we get more content than advertises, abort.
        let mut rt = Runtime::new().unwrap();

        let chunks = vec![
            Bytes::from(vec![1; 5]),
            Bytes::from(vec![1; 5]),
            Bytes::from(vec![1; 5]),
            Bytes::from(vec![1; 5]),
        ];
        let in_stream = stream::iter_ok::<_, Error>(chunks);

        let fut = match make_chunks(in_stream, ExpectedSize::new(10), Some(1)) {
            Chunks::Chunked(_, stream) => stream.collect(),
            c @ Chunks::Inline(..) => panic!("Did not expect {:?}", c),
        };

        rt.block_on(fut)
            .expect_err("make_chunks should abort if the content does not end as advertised");
    }

    #[test]
    fn test_stream_of_empty_bytes() {
        // If we give ChunkStream a stream that contains empty bytes, then we should return one
        // chunk of empty bytes.
        let mut rt = Runtime::new().unwrap();

        let chunks = vec![Bytes::new()];
        let in_stream = stream::iter_ok::<_, Error>(chunks);
        let stream = ChunkStream::new(in_stream, 1);

        let (ret, stream) = rt.block_on(stream.into_future()).unwrap();
        assert_eq!(ret, Some(Bytes::new()));

        let (ret, _) = rt.block_on(stream.into_future()).unwrap();
        assert_eq!(ret, None);
    }

    #[test]
    fn test_stream_of_repeated_empty_bytes() {
        // If we give ChunkStream a stream that contains however many empty bytes, then we should
        // return a single chunk of empty bytes.
        let mut rt = Runtime::new().unwrap();

        let chunks = vec![Bytes::new(), Bytes::new()];
        let in_stream = stream::iter_ok::<_, Error>(chunks);
        let stream = ChunkStream::new(in_stream, 1);

        let (ret, stream) = rt.block_on(stream.into_future()).unwrap();
        assert_eq!(ret, Some(Bytes::new()));

        let (ret, _) = rt.block_on(stream.into_future()).unwrap();
        assert_eq!(ret, None);
    }

    #[test]
    fn test_empty_stream() {
        // If we give ChunkStream an empty stream, it should retun an empty stream.
        let mut rt = Runtime::new().unwrap();

        let in_stream = stream::iter_ok::<_, Error>(vec![]);
        let stream = ChunkStream::new(in_stream, 1);

        let (ret, _) = rt.block_on(stream.into_future()).unwrap();
        assert_eq!(ret, None);
    }

    #[test]
    fn test_bigger_incoming_chunks() {
        // Explicitly test that ChunkStream handles splitting chunks.
        let chunks = vec![vec![1; 10], vec![1; 10]];
        assert!(do_check_chunk_stream(chunks, 5))
    }

    #[test]
    fn test_smaller_incoming_chunks() {
        // Explicitly test that ChunkStream handles putting chunks together.
        let chunks = vec![vec![1; 10], vec![1; 10]];
        assert!(do_check_chunk_stream(chunks, 15))
    }

    #[test]
    fn test_stream_exhaustion() {
        struct StrictStream {
            chunks: Vec<Bytes>,
            done: bool,
        };

        impl Stream for StrictStream {
            type Item = Bytes;
            type Error = ();

            fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
                if self.done {
                    panic!("StrictStream was done");
                }

                match self.chunks.pop() {
                    Some(b) => Ok(Async::Ready(Some(b))),
                    None => {
                        self.done = true;
                        Ok(Async::Ready(None))
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

        assert_matches!(stream.poll(), Ok(Async::Ready(Some(_))));
        assert_matches!(stream.poll(), Ok(Async::Ready(Some(_))));
        assert_matches!(stream.poll(), Ok(Async::Ready(None)));
        assert_matches!(stream.poll(), Ok(Async::Ready(None)));
    }

    fn do_check_chunk_stream(in_chunks: Vec<Vec<u8>>, size: usize) -> bool {
        let mut rt = Runtime::new().unwrap();

        let in_chunks: Vec<Bytes> = in_chunks.into_iter().map(Bytes::from).collect();
        let chunk_stream = ChunkStream::new(stream::iter_ok::<_, ()>(in_chunks.clone()), size);
        let out_chunks = rt.block_on(chunk_stream.collect()).unwrap();

        let expected_bytes = in_chunks.iter().fold(Bytes::new(), |mut bytes, chunk| {
            bytes.extend_from_slice(&chunk);
            bytes
        });

        let got_bytes = out_chunks.iter().fold(Bytes::new(), |mut bytes, chunk| {
            bytes.extend_from_slice(&chunk);
            bytes
        });

        // The contents should be the same
        if expected_bytes != got_bytes {
            return false;
        }

        // If there were no contents, then just return that.
        if expected_bytes.len() == 0 {
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
        fn check_chunk_stream(in_chunks: Vec<Vec<u8>>, size: usize) -> bool {
            let size = size + 1; // Don't allow 0 as the size.
            do_check_chunk_stream(in_chunks, size)
        }

        fn check_make_chunks_fut_joins(in_chunks: Vec<Vec<u8>>) -> bool {
            let mut rt = Runtime::new().unwrap();

            let in_chunks: Vec<Bytes> = in_chunks.into_iter().map(Bytes::from).collect();
            let in_stream = stream::iter_ok::<_, Error>(in_chunks.clone());

            let expected_bytes = in_chunks.iter().fold(Bytes::new(), |mut bytes, chunk| {
                bytes.extend_from_slice(&chunk);
                bytes
            });

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
