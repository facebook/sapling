/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Decodes stream of data that is "chunked" in the following format:
//!
//! ```text
//! stream := <chunk>
//! chunk := <numbytes> '\n' <byte>{numbytes} <chunk> | '0\n'
//! ```
//!
//! 0-sized chunk is the indication of end of stream, so a proper stream of data should not
//! contain empty chunks inside.

use std::io::Result as IoResult;
use std::pin::Pin;
use std::task::Context;
use std::task::Poll;

use futures::ready;
use pin_project::pin_project;
use tokio::io::AsyncBufRead;
use tokio::io::AsyncRead;
use tokio::io::ReadBuf;

/// Structure that wraps around a `AsyncBufRead` object to provide chunked-based encoding
/// over it. See this module's doc for the description of the format.
///
/// This structure ensures that you don't override the underlying AsyncBufRead, so if you read EOF while
/// reading from `Dechunker` and call `Dechunker::into_inner` on it then you will get the original
/// `AsyncBufRead` object that contains data following the encoded chunks.
#[pin_project]
pub struct Dechunker<R> {
    #[pin]
    bufread: R,
    state: DechunkerState,
}

enum DechunkerState {
    ParsingInt,
    ReadingChunk(usize),
    Done,
}

impl<R> Dechunker<R>
where
    R: AsyncBufRead,
{
    pub fn new(bufread: R) -> Self {
        Self {
            bufread,
            state: DechunkerState::ParsingInt,
        }
    }

    #[cfg(test)]
    pub async fn is_done(&mut self) -> IoResult<bool>
    where
        Self: Unpin,
    {
        use tokio::io::AsyncBufReadExt;
        Ok(self.fill_buf().await?.is_empty())
    }

    pub fn into_inner(self) -> R {
        self.bufread
    }

    /// If the self.state is ParsingInt then we try to parse the following content of the buffer
    /// as:
    ///
    /// ```text
    /// <integer> '\n'
    /// integer := [digit]*
    /// digit := '0' | '1' | ... | '9'
    /// ```
    ///
    /// If the integer parsed was of value `0` then the parsing is done, otherwise we continue by
    /// reading a chunk of the size equal to the provided integer.
    fn advance_parsing_int(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<IoResult<usize>> {
        let mut this = self.project();
        match this.state {
            DechunkerState::ParsingInt => {
                let available = ready!(this.bufread.as_mut().poll_fill_buf(cx))?;
                let mut size = 0usize;
                for (idx, ch) in available.iter().enumerate() {
                    match ch {
                        b'0'..=b'9' => {
                            size = size
                                .checked_mul(10)
                                .and_then(|s| s.checked_add((ch - b'0') as usize))
                                .ok_or_else(|| {
                                    std::io::Error::new(
                                        std::io::ErrorKind::InvalidData,
                                        format!(
                                            "Failed to parse int for Dechunker from '{:?}'",
                                            &available[..idx + 1]
                                        ),
                                    )
                                })?
                        }
                        b'\n' => {
                            this.bufread.as_mut().consume(idx + 1);
                            if size == 0 {
                                *this.state = DechunkerState::Done;
                            } else {
                                *this.state = DechunkerState::ReadingChunk(size);
                            }
                            return Poll::Ready(Ok(size));
                        }
                        _ => {
                            return Poll::Ready(Err(std::io::Error::new(
                                std::io::ErrorKind::InvalidData,
                                format!(
                                    "Failed to parse int for Dechunker from '{:?}'",
                                    &available[..idx + 1]
                                ),
                            )));
                        }
                    }
                }
                Poll::Ready(Ok(0))
            }
            DechunkerState::ReadingChunk(size) => Poll::Ready(Ok(*size)),
            DechunkerState::Done => Poll::Ready(Ok(0)),
        }
    }
}

impl DechunkerState {
    fn consume_chunk(&mut self, amt: usize) {
        if amt > 0 {
            let chunk_size = match self {
                DechunkerState::ReadingChunk(chunk_size) => *chunk_size,
                _ => panic!("Trying to consume bytes while internally not reading chunk yet"),
            };

            if amt == chunk_size {
                *self = DechunkerState::ParsingInt;
            } else {
                assert!(
                    chunk_size > amt,
                    "Trying to consume more bytes than the size of chunk"
                );
                *self = DechunkerState::ReadingChunk(chunk_size - amt);
            }
        }
    }
}

impl<R> AsyncRead for Dechunker<R>
where
    R: AsyncBufRead,
{
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<IoResult<()>> {
        let size = ready!(self.as_mut().advance_parsing_int(cx))?;
        if size > 0 {
            let mut remaining = buf.take(size);
            let this = self.project();
            ready!(this.bufread.poll_read(cx, &mut remaining))?;
            let amt = remaining.filled().len();
            unsafe {
                // SAFETY: initialized by the inner call to `poll_read`.
                buf.assume_init(amt);
            }
            buf.advance(amt);
            this.state.consume_chunk(amt);
        }
        Poll::Ready(Ok(()))
    }
}

impl<R> AsyncBufRead for Dechunker<R>
where
    R: AsyncBufRead,
{
    fn poll_fill_buf(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<IoResult<&[u8]>> {
        let size = ready!(self.as_mut().advance_parsing_int(cx))?;
        if size > 0 {
            let buf = ready!(self.project().bufread.poll_fill_buf(cx))?;
            let max = std::cmp::min(buf.len(), size);
            Poll::Ready(Ok(&buf[0..max]))
        } else {
            Poll::Ready(Ok(&[]))
        }
    }

    fn consume(self: Pin<&mut Self>, amt: usize) {
        let this = self.project();
        this.state.consume_chunk(amt);
        this.bufread.consume(amt);
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use anyhow::ensure;
    use anyhow::Result;
    use quickcheck::quickcheck;
    use quickcheck::Arbitrary;
    use quickcheck::Gen;
    use quickcheck::TestResult;
    use tokio::io::AsyncBufReadExt;
    use tokio::io::AsyncReadExt;
    use tokio::runtime::Runtime;

    use super::*;

    // Vec of non empty Vec<u8> for quickcheck::Arbitrary
    #[derive(Clone, Debug)]
    struct Chunks(Vec<Vec<u8>>);
    impl Arbitrary for Chunks {
        fn arbitrary(g: &mut Gen) -> Self {
            Chunks(
                (0..g.size())
                    .map(|_| Arbitrary::arbitrary(g))
                    .filter_map(|v: Vec<u8>| if v.is_empty() { None } else { Some(v) })
                    .collect(),
            )
        }

        fn shrink(&self) -> Box<dyn Iterator<Item = Self>> {
            Box::new(
                Arbitrary::shrink(&self.0)
                    .map(|v| {
                        v.into_iter()
                            .filter_map(|v| if v.is_empty() { None } else { Some(v) })
                            .collect()
                    })
                    .map(Chunks),
            )
        }
    }

    quickcheck! {
        fn test_bufread_api(chunks: Chunks, remainder: Vec<u8>) -> TestResult {
            let runtime = Runtime::new().unwrap();
            let chunks = &chunks;
            let remainder = remainder.as_slice();
            let concat_chunks = concat_chunks(chunks, remainder);

            let dechunker = Dechunker::new(Cursor::new(&concat_chunks));
            match runtime.block_on(check_bufread_api(
                dechunker,
                chunks,
                remainder,
            )) {
                Ok(()) => TestResult::passed(),
                Err(e) =>TestResult::error(format!("{}", e)),
            }
        }

        fn test_read_api(chunks: Chunks, remainder: Vec<u8>) -> TestResult {
            let runtime = Runtime::new().unwrap();
            let chunks = &chunks;
            let remainder = remainder.as_slice();
            let concat_chunks = concat_chunks(chunks, remainder);

            let dechunker = Dechunker::new(Cursor::new(&concat_chunks));

            match runtime.block_on(check_read_api(
                dechunker,
                chunks,
                remainder,
            )) {
                Ok(()) => TestResult::passed(),
                Err(e) => TestResult::error(format!("{}", e)),
            }
        }
    }

    fn concat_chunks(chunks: &Chunks, remainder: &[u8]) -> Vec<u8> {
        let mut buf = Vec::new();
        for chunk in &chunks.0 {
            buf.extend_from_slice(format!("{}\n", chunk.len()).as_bytes());
            buf.extend_from_slice(chunk.as_slice());
        }
        buf.extend_from_slice(b"0\n");
        buf.extend_from_slice(remainder);
        buf
    }

    async fn check_bufread_api<R: AsyncBufRead + Unpin>(
        mut d: Dechunker<R>,
        chunks: &Chunks,
        remainder: &[u8],
    ) -> Result<()> {
        for chunk in &chunks.0 {
            let buf_len = {
                let buf = d.fill_buf().await?;
                ensure!(
                    buf == chunk.as_slice(),
                    "expected {:?} found {:?} in bufread api check",
                    chunk,
                    buf
                );
                buf.len()
            };
            d.consume(buf_len);
        }

        check_remainder(d, remainder).await
    }

    async fn check_read_api<R: AsyncBufRead + Unpin>(
        mut d: Dechunker<R>,
        chunks: &Chunks,
        remainder: &[u8],
    ) -> Result<()> {
        let all_chunks = {
            let mut buf = Vec::new();
            for chunk in &chunks.0 {
                buf.extend_from_slice(chunk.as_slice());
            }
            buf
        };

        let mut buf = Vec::new();
        let buf_len = d.read_to_end(&mut buf).await?;
        ensure!(
            buf_len == all_chunks.len(),
            "expected read_to_end {:?} bytes, but read {:?}",
            all_chunks.len(),
            buf_len
        );
        ensure!(
            buf == all_chunks,
            "expected read_to_end {:?}, but read {:?}",
            all_chunks,
            buf
        );

        check_remainder(d, remainder).await
    }

    async fn check_remainder<R: AsyncBufRead + Unpin>(
        mut d: Dechunker<R>,
        remainder: &[u8],
    ) -> Result<()> {
        ensure!(d.is_done().await?, "expected the dechunker to be done");

        let mut inner = d.into_inner();
        let mut buf = Vec::new();
        let buf_len = inner.read_to_end(&mut buf).await?;
        ensure!(
            buf_len == remainder.len(),
            "expected read_to_end {:?} bytes from inner reader, but read {:?}",
            remainder.len(),
            buf_len
        );
        ensure!(
            buf.as_slice() == remainder,
            "expected read_to_end {:?} from inner reader, but read {:?}",
            remainder,
            buf
        );
        Ok(())
    }
}
