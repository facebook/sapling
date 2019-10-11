/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

//! Decodes stream of data that is "chunked" in the following format:
//!
//! ```
//! stream := <chunk>
//! chunk := <numbytes> '\n' <byte>{numbytes} <chunk> | '0\n'
//! ```
//!
//! 0-sized chunk is the indication of end of stream, so a proper stream of data should not
//! contain empty chunks inside.

use bytes::Bytes;
use std::io::{self, BufRead, Read};
use std::sync::{Arc, Mutex};

use failure::format_err;
use futures::future::poll_fn;
use futures::{Async, Future};
use tokio_io::{try_nb, AsyncRead};

/// Structure that wraps around a `AsyncRead + BufRead` object to provide chunked-based encoding
/// over it. See this module's doc for the description of the format.
///
/// This structure ensures that you don't override the underlying BufRead, so if you read EOF while
/// reading from `Dechunker` and call `Dechunker::into_inner` on it then you will get the original
/// `BufRead` object that contains data following the encoded chunks.
pub struct Dechunker<R> {
    bufread: R,
    state: DechunkerState,
    maybe_full_content: Option<Arc<Mutex<Bytes>>>,
}

enum DechunkerState {
    ParsingInt(Vec<u8>),
    ReadingChunk(usize),
    Done,
}

use self::DechunkerState::*;

impl<R> Dechunker<R>
where
    R: AsyncRead + BufRead,
{
    pub fn new(bufread: R) -> Self {
        Self {
            bufread,
            state: ParsingInt(Vec::new()),
            maybe_full_content: None,
        }
    }

    #[allow(dead_code)]
    pub fn with_full_content(mut self, full_bundle2_content: Arc<Mutex<Bytes>>) -> Self {
        // TODO(ikostia): make this used in commands.rs and remove the attribute above
        self.maybe_full_content = Some(full_bundle2_content);
        self
    }

    pub fn check_is_done(self) -> impl Future<Item = (bool, Self), Error = io::Error> {
        let mut this = Some(self);
        poll_fn(move || {
            let is_done = match this {
                None => panic!("called poll after completed"),
                Some(ref mut this) => try_nb!(this.fill_buf()).is_empty(),
            };

            let this = this.take().expect("This was Some few lines above");
            Ok(Async::Ready((is_done, this)))
        })
    }

    pub fn into_inner(self) -> R {
        self.bufread
    }

    /// If the self.state is ParsingInt then we try to parse the following content of the buffer
    /// as:
    ///
    /// ```
    /// <integer> '\n'
    /// integer := [digit]*
    /// digit := '0' | '1' | ... | '9'
    /// ```
    ///
    /// If the integer parsed was of value `0` then the parsing is done, otherwise we continue by
    /// reading a chunk of the size equal to the provided integer.
    fn advance_parsing_int(&mut self) -> io::Result<()> {
        let chunk_size = match &mut self.state {
            &mut ParsingInt(ref mut buf) => {
                self.bufread.read_until(b'\n', buf)?;

                let mut size = 0;
                for inp in &*buf {
                    match *inp {
                        digit @ b'0'..=b'9' => size = size * 10 + ((digit - b'0') as usize),
                        b'\n' => break,
                        _ => {
                            return Err(io::Error::new(
                                io::ErrorKind::InvalidData,
                                format_err!("Failed to parse int for Dechunker from '{:?}'", buf)
                                    .compat(),
                            ));
                        }
                    }
                }

                size
            }
            _ => return Ok(()),
        };

        if chunk_size == 0 {
            self.state = Done;
        } else {
            self.state = ReadingChunk(chunk_size);
        }
        Ok(())
    }

    fn consume_chunk(&mut self, amt: usize) {
        if amt > 0 {
            let chunk_size = match &self.state {
                &ReadingChunk(ref chunk_size) => *chunk_size,
                _ => panic!("Trying to consume bytes while internally not reading chunk yet"),
            };

            if amt == chunk_size {
                self.state = ParsingInt(Vec::new());
            } else {
                assert!(
                    chunk_size > amt,
                    "Trying to consume more bytes than the size of chunk"
                );
                self.state = ReadingChunk(chunk_size - amt);
            }
        }
    }
}

impl<R> Read for Dechunker<R>
where
    R: AsyncRead + BufRead,
{
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.advance_parsing_int()?;
        let chunk_size = match &self.state {
            &ParsingInt(_) => panic!("expected to get pass parsing int state"),
            &ReadingChunk(ref chunk_size) => *chunk_size,
            &Done => return Ok(0),
        };

        let buf_size = if buf.len() > chunk_size {
            chunk_size
        } else {
            buf.len()
        };

        let buf_size = self.bufread.read(&mut buf[0..buf_size])?;
        self.consume_chunk(buf_size);
        if let Some(ref mut full_bundle2_content) = self.maybe_full_content {
            full_bundle2_content
                .lock()
                .unwrap()
                .extend_from_slice(&buf[0..buf_size]);
        }
        Ok(buf_size)
    }
}

impl<R> AsyncRead for Dechunker<R> where R: AsyncRead + BufRead {}

impl<R> BufRead for Dechunker<R>
where
    R: AsyncRead + BufRead,
{
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        self.advance_parsing_int()?;
        let chunk_size = match &self.state {
            &ParsingInt(_) => panic!("expected to get pass parsing int state"),
            &ReadingChunk(ref chunk_size) => *chunk_size,
            &Done => return Ok(&[]),
        };

        let buf = self.bufread.fill_buf()?;
        let buf_size = if buf.len() > chunk_size {
            chunk_size
        } else {
            buf.len()
        };
        Ok(&buf[0..buf_size])
    }

    fn consume(&mut self, amt: usize) {
        self.consume_chunk(amt);
        self.bufread.consume(amt);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::errors::*;
    use failure_ext::ensure_msg;
    use quickcheck::{quickcheck, Arbitrary, Gen, TestResult};
    use std::io::Cursor;

    // Vec of non empty Vec<u8> for quickcheck::Arbitrary
    #[derive(Clone, Debug)]
    struct Chunks(Vec<Vec<u8>>);
    impl Arbitrary for Chunks {
        fn arbitrary<G: Gen>(g: &mut G) -> Self {
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
            let chunks = &chunks;
            let remainder = remainder.as_slice();
            let concat_chunks = concat_chunks(chunks, remainder);

            match check_bufread_api(
                Dechunker::new(Cursor::new(&concat_chunks)),
                chunks,
                remainder,
            ) {
                Ok(()) => TestResult::passed(),
                Err(e) =>TestResult::error(format!("{}", e)),
            }
        }

        fn test_read_api(chunks: Chunks, remainder: Vec<u8>) -> TestResult {
            let chunks = &chunks;
            let remainder = remainder.as_slice();
            let concat_chunks = concat_chunks(chunks, remainder);

            match check_read_api(
                Dechunker::new(Cursor::new(&concat_chunks)),
                chunks,
                remainder,
            ) {
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

    fn check_bufread_api<R: AsyncRead + BufRead>(
        mut d: Dechunker<R>,
        chunks: &Chunks,
        remainder: &[u8],
    ) -> Result<()> {
        for chunk in &chunks.0 {
            let buf_len = {
                let buf = d.fill_buf()?;
                ensure_msg!(
                    buf == chunk.as_slice(),
                    "expected {:?} found {:?} in bufread api check",
                    chunk,
                    buf
                );
                buf.len()
            };
            d.consume(buf_len);
        }

        check_remainder(d, remainder)
    }

    fn check_read_api<R: AsyncRead + BufRead>(
        mut d: Dechunker<R>,
        chunks: &Chunks,
        remainder: &[u8],
    ) -> Result<()> {
        let concat_chunks = {
            let mut buf = Vec::new();
            for chunk in &chunks.0 {
                buf.extend_from_slice(chunk.as_slice());
            }
            buf
        };

        let mut buf = Vec::new();
        let buf_len = d.read_to_end(&mut buf)?;
        ensure_msg!(
            buf_len == concat_chunks.len(),
            "expected read_to_end {:?} bytes, but read {:?}",
            concat_chunks.len(),
            buf_len
        );
        ensure_msg!(
            buf == concat_chunks,
            "expected read_to_end {:?}, but read {:?}",
            concat_chunks,
            buf
        );
        check_remainder(d, remainder)
    }

    fn check_remainder<R: AsyncRead + BufRead>(d: Dechunker<R>, remainder: &[u8]) -> Result<()> {
        let (is_done, d) = d.check_is_done().wait()?;
        ensure_msg!(is_done, "expected the dechunker to be done");

        let mut inner = d.into_inner();
        let mut buf = Vec::new();
        let buf_len = inner.read_to_end(&mut buf)?;
        ensure_msg!(
            buf_len == remainder.len(),
            "expected read_to_end {:?} bytes from inner reader, but read {:?}",
            remainder.len(),
            buf_len
        );
        ensure_msg!(
            buf.as_slice() == remainder,
            "expected read_to_end {:?} from inner reader, but read {:?}",
            remainder,
            buf
        );
        Ok(())
    }
}
