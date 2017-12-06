// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! Unpacking wirepacks.

use std::cmp;
use std::mem;

use byteorder::{BigEndian, ByteOrder};
use bytes::BytesMut;
use slog;
use tokio_io::codec::Decoder;

use mercurial_types::{Delta, RepoPath, NULL_HASH};

use super::{DataEntry, HistoryEntry, Kind, Part};
use delta;
use errors::*;
use part_inner::InnerPart;
use utils::BytesExt;

#[derive(Debug)]
pub struct WirePackUnpacker {
    state: State,
    inner: UnpackerInner,
}

impl Decoder for WirePackUnpacker {
    type Item = InnerPart;
    type Error = Error;

    fn decode(&mut self, buf: &mut BytesMut) -> Result<Option<InnerPart>> {
        match self.inner.decode_next(buf, self.state.take()) {
            Err(e) => {
                self.state = State::Invalid;
                Err(e)
            }
            Ok((ret, state)) => {
                self.state = state;
                match ret {
                    None => Ok(None),
                    Some(v) => Ok(Some(InnerPart::WirePack(v))),
                }
            }
        }
    }

    fn decode_eof(&mut self, buf: &mut BytesMut) -> Result<Option<InnerPart>> {
        match self.decode(buf)? {
            None => {
                if !buf.is_empty() {
                    let len = buf.len();
                    let bytes = &buf[..cmp::min(len, 128)];
                    let msg = format!(
                        "incomplete wirepack: {} bytes remaining in \
                         buffer. State: {:?}, First 128 bytes: {:?}",
                        len,
                        self.state,
                        bytes,
                    );
                    Err(ErrorKind::WirePackDecode(msg))?;
                }
                if self.state != State::End {
                    let msg = format!(
                        "incomplete wirepack: expected state End, found {:?}",
                        self.state
                    );
                    Err(ErrorKind::WirePackDecode(msg))?;
                }
                Ok(None)
            }
            Some(v) => Ok(Some(v)),
        }
    }
}

const WIREPACK_END: &[u8] = b"\0\0\0\0\0\0\0\0\0\0";

// See the history header definition below for the breakdown.
const HISTORY_COPY_FROM_OFFSET: usize = 20 + 20 + 20 + 20;
const HISTORY_HEADER_SIZE: usize = HISTORY_COPY_FROM_OFFSET + 2;

// See the data header definition below for the breakdown.
const DATA_DELTA_OFFSET: usize = 20 + 20;
const DATA_HEADER_SIZE: usize = DATA_DELTA_OFFSET + 8;

pub fn new(logger: slog::Logger, kind: Kind) -> WirePackUnpacker {
    WirePackUnpacker {
        state: State::Filename,
        inner: UnpackerInner { logger, kind },
    }
}

#[derive(Debug)]
struct UnpackerInner {
    logger: slog::Logger,
    kind: Kind,
}

impl UnpackerInner {
    fn decode_next(&mut self, buf: &mut BytesMut, state: State) -> Result<(Option<Part>, State)> {
        use self::State::*;
        let mut state = state;

        loop {
            trace!(self.logger, "state: {:?}", state);
            match state {
                Filename => match self.decode_filename(buf)? {
                    DecodeRes::None => return Ok((None, State::Filename)),
                    DecodeRes::Some(f) => {
                        state = HistoryStart(f);
                    }
                    DecodeRes::End => return Ok((Some(Part::End), State::End)),
                },
                HistoryStart(f) => match self.decode_section_start(buf) {
                    Some(entry_count) => {
                        state = self.next_history_state(f, entry_count);
                    }
                    None => return Ok((None, HistoryStart(f))),
                },
                History(f, entry_count) => match self.decode_history(buf)? {
                    Some(entry) => {
                        let entry_count = entry_count - 1;
                        let next_state = self.next_history_state(&f, entry_count);
                        return Ok((Some(Part::History(f, entry)), next_state));
                    }
                    None => return Ok((None, History(f, entry_count))),
                },
                DataStart(f) => match self.decode_section_start(buf) {
                    Some(entry_count) => {
                        state = self.next_data_state(f, entry_count);
                    }
                    None => return Ok((None, DataStart(f))),
                },
                Data(f, entry_count) => match self.decode_data(buf)? {
                    Some(entry) => {
                        let entry_count = entry_count - 1;
                        let next_state = self.next_data_state(&f, entry_count);
                        return Ok((Some(Part::Data(f, entry)), next_state));
                    }
                    None => return Ok((None, Data(f, entry_count))),
                },
                End => return Ok((None, End)),
                Invalid => Err(ErrorKind::WirePackDecode("byte stream corrupt".into()))?,
            }
        }
    }

    #[inline]
    fn next_history_state<P: Into<RepoPath>>(&mut self, filename: P, entry_count: usize) -> State {
        if entry_count == 0 {
            State::DataStart(filename.into())
        } else {
            State::History(filename.into(), entry_count)
        }
    }

    #[inline]
    fn next_data_state<P: Into<RepoPath>>(&mut self, filename: P, entry_count: usize) -> State {
        if entry_count == 0 {
            State::Filename
        } else {
            State::Data(filename.into(), entry_count)
        }
    }

    fn decode_filename(&mut self, buf: &mut BytesMut) -> Result<DecodeRes<RepoPath>> {
        // Notes:
        // - A zero-length filename indicates the root manifest for tree packs.
        //   (It is not allowed for file packs.)
        // - The end of the stream is marked with 10 null bytes (2 for the filename + 4 for
        //   history entry count + 4 for data entry count).
        // - This means that the buffer has to have at least 10 bytes in it at this stage.
        if buf.len() < 10 {
            return Ok(DecodeRes::None);
        }
        if &buf[..10] == WIREPACK_END {
            let _ = buf.split_to(10);
            return Ok(DecodeRes::End);
        }

        let filename_len = buf.peek_u16() as usize;
        if buf.len() < filename_len + 2 {
            return Ok(DecodeRes::None);
        }
        let _ = buf.split_to(2);
        let filename = if filename_len == 0 {
            match self.kind {
                Kind::Tree => RepoPath::root(),
                Kind::File => Err(ErrorKind::WirePackDecode(
                    "file packs cannot contain zero-length filenames".into(),
                ))?,
            }
        } else {
            let mpath = buf.drain_path(filename_len).with_context(|_| {
                let msg = format!("invalid filename of length {}", filename_len);
                ErrorKind::WirePackDecode(msg)
            })?;

            match self.kind {
                Kind::Tree => RepoPath::dir(mpath),
                Kind::File => RepoPath::file(mpath),
            }.with_context(|_| ErrorKind::WirePackDecode("invalid filename".into()))?
        };

        trace!(self.logger, "decoding entries for filename: {}", filename);

        Ok(DecodeRes::Some(filename))
    }

    fn decode_section_start(&mut self, buf: &mut BytesMut) -> Option<usize> {
        if buf.len() < 4 {
            None
        } else {
            Some(buf.drain_u32() as usize)
        }
    }

    fn decode_history(&mut self, buf: &mut BytesMut) -> Result<Option<HistoryEntry>> {
        if buf.len() < HISTORY_HEADER_SIZE {
            return Ok(None);
        }

        // A history revision has:
        // ---
        // node: NodeHash (20 bytes)
        // p1: NodeHash (20 bytes)
        // p2: NodeHash (20 bytes)
        // link node: NodeHash (20 bytes)
        // copy from len: u16 (2 bytes) -- 0 if this revision is not a copy
        // copy from: RepoPath (<copy from len> bytes)
        // ---
        // Tree revisions are never copied, so <copy from len> is always 0.

        let copy_from_len =
            BigEndian::read_u16(&buf[HISTORY_COPY_FROM_OFFSET..HISTORY_HEADER_SIZE]) as usize;
        if buf.len() < HISTORY_HEADER_SIZE + copy_from_len {
            return Ok(None);
        }

        let node = buf.drain_node();
        let p1 = buf.drain_node();
        let p2 = buf.drain_node();
        let linknode = buf.drain_node();
        let _ = buf.drain_u16();
        let copy_from = if copy_from_len > 0 {
            let path = buf.drain_path(copy_from_len)?;
            match self.kind {
                Kind::Tree => Err(ErrorKind::WirePackDecode(format!(
                    "tree entry {} is marked as copied from path {}, but they cannot be copied",
                    node,
                    path
                )))?,
                Kind::File => Some(RepoPath::file(path).with_context(|_| {
                    ErrorKind::WirePackDecode("invalid copy from path".into())
                })?),
            }
        } else {
            None
        };
        Ok(Some(HistoryEntry {
            node,
            p1,
            p2,
            linknode,
            copy_from,
        }))
    }

    fn decode_data(&mut self, buf: &mut BytesMut) -> Result<Option<DataEntry>> {
        if buf.len() < DATA_HEADER_SIZE {
            return Ok(None);
        }

        // A data revision has:
        // ---
        // node: NodeHash (20 bytes)
        // delta base: NodeHash (20 bytes) -- NULL_HASH if full text
        // delta len: u64 (8 bytes)
        // delta: Delta (<delta len> bytes)
        // ---
        // There's a bit of a wart in the current format: if delta base is NULL_HASH, instead of
        // storing a delta with start = 0 and end = 0, we store the full text directly. This
        // should be fixed in a future wire protocol revision.
        let delta_len = BigEndian::read_u64(&buf[DATA_DELTA_OFFSET..DATA_HEADER_SIZE]) as usize;
        if buf.len() < DATA_HEADER_SIZE + delta_len {
            return Ok(None);
        }

        let node = buf.drain_node();
        let delta_base = buf.drain_node();
        let _ = buf.drain_u64();
        let delta = buf.split_to(delta_len);

        let delta = if delta_base == NULL_HASH {
            Delta::new_fulltext(delta.to_vec())
        } else {
            delta::decode_delta(delta)?
        };

        Ok(Some(DataEntry {
            node,
            delta_base,
            delta,
        }))
    }
}

#[derive(Debug, Eq, PartialEq)]
enum State {
    Filename,
    HistoryStart(RepoPath),
    History(RepoPath, usize),
    DataStart(RepoPath),
    Data(RepoPath, usize),
    End,
    Invalid,
}

impl State {
    pub fn take(&mut self) -> Self {
        mem::replace(self, State::Invalid)
    }
}

enum DecodeRes<T> {
    None,
    Some(T),
    End,
}

#[cfg(test)]
mod test {
    use std::io::{self, Cursor};

    use futures::Stream;
    use slog::Drain;
    use slog_term;
    use tokio_core::reactor::Core;
    use tokio_io::codec::FramedRead;

    use super::*;

    #[test]
    fn test_empty() {
        let logger = make_root_logger();
        let mut core = Core::new().unwrap();

        // Absolutely nothing in here.
        let empty_1 = Cursor::new(WIREPACK_END);
        let unpacker = new(logger.clone(), Kind::Tree);
        let stream = FramedRead::new(empty_1, unpacker);
        let collect_fut = stream.collect();

        let parts = core.run(collect_fut).unwrap();
        assert_eq!(parts, vec![InnerPart::WirePack(Part::End)]);

        // A file with no entries:
        // - filename b"\0\x03foo"
        // - history count: b"\0\0\0\0"
        // - data count: b"\0\0\0\0"
        // - next filename, end of stream: b"\0\0\0\0\0\0\0\0\0\0"
        let empty_2 = Cursor::new(b"\0\x03foo\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0");
        let unpacker = new(logger.clone(), Kind::File);
        let stream = FramedRead::new(empty_2, unpacker);
        let collect_fut = stream.collect();

        let parts = core.run(collect_fut).unwrap();
        assert_eq!(parts, vec![InnerPart::WirePack(Part::End)]);
    }

    fn make_root_logger() -> slog::Logger {
        let plain = slog_term::PlainSyncDecorator::new(io::stdout());
        slog::Logger::root(slog_term::FullFormat::new(plain).build().fuse(), o!())
    }
}
