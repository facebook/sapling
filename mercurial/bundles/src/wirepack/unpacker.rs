// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! Unpacking wirepacks.

use std::cmp;
use std::mem;

use bytes::BytesMut;
use failure_ext::bail_err;
use slog::trace;
use tokio_codec::Decoder;

use context::CoreContext;
use mercurial_types::RepoPath;

use super::{DataEntry, DataEntryVersion, HistoryEntry, Kind, Part, WIREPACK_END};
use crate::errors::*;
use crate::utils::BytesExt;

#[derive(Debug)]
pub struct WirePackUnpacker {
    state: State,
    inner: UnpackerInner,
}

impl Decoder for WirePackUnpacker {
    type Item = Part;
    type Error = Error;

    fn decode(&mut self, buf: &mut BytesMut) -> Result<Option<Self::Item>> {
        match self.inner.decode_next(buf, self.state.take()) {
            Err(e) => {
                self.state = State::Invalid;
                Err(e)
            }
            Ok((ret, state)) => {
                self.state = state;
                match ret {
                    None => Ok(None),
                    Some(v) => Ok(Some(v)),
                }
            }
        }
    }

    fn decode_eof(&mut self, buf: &mut BytesMut) -> Result<Option<Self::Item>> {
        match self.decode(buf)? {
            None => {
                if !buf.is_empty() {
                    let len = buf.len();
                    let bytes = &buf[..cmp::min(len, 128)];
                    let msg = format!(
                        "incomplete wirepack: {} bytes remaining in \
                         buffer. State: {:?}, First 128 bytes: {:?}",
                        len, self.state, bytes,
                    );
                    bail_err!(ErrorKind::WirePackDecode(msg));
                }
                if self.state != State::End {
                    let msg = format!(
                        "incomplete wirepack: expected state End, found {:?}",
                        self.state
                    );
                    bail_err!(ErrorKind::WirePackDecode(msg));
                }
                Ok(None)
            }
            Some(v) => Ok(Some(v)),
        }
    }
}

pub fn new(ctx: CoreContext, kind: Kind) -> WirePackUnpacker {
    WirePackUnpacker {
        state: State::Filename,
        inner: UnpackerInner { ctx, kind },
    }
}

#[derive(Debug)]
struct UnpackerInner {
    ctx: CoreContext,
    kind: Kind,
}

impl UnpackerInner {
    fn decode_next(&mut self, buf: &mut BytesMut, state: State) -> Result<(Option<Part>, State)> {
        use self::State::*;
        let mut state = state;

        loop {
            trace!(self.ctx.logger(), "state: {:?}", state);
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
                        let next_state = self.next_history_state(&f, entry_count);
                        return Ok((
                            Some(Part::HistoryMeta {
                                path: f,
                                entry_count,
                            }),
                            next_state,
                        ));
                    }
                    None => return Ok((None, HistoryStart(f))),
                },
                History(f, entry_count) => match self.decode_history(buf)? {
                    Some(entry) => {
                        let entry_count = entry_count - 1;
                        let next_state = self.next_history_state(&f, entry_count);
                        return Ok((Some(Part::History(entry)), next_state));
                    }
                    None => return Ok((None, History(f, entry_count))),
                },
                DataStart(f) => match self.decode_section_start(buf) {
                    Some(entry_count) => {
                        let next_state = self.next_data_state(&f, entry_count);
                        return Ok((
                            Some(Part::DataMeta {
                                path: f,
                                entry_count,
                            }),
                            next_state,
                        ));
                    }
                    None => return Ok((None, DataStart(f))),
                },
                Data(f, entry_count) => match self.decode_data(buf)? {
                    Some(entry) => {
                        let entry_count = entry_count - 1;
                        let next_state = self.next_data_state(&f, entry_count);
                        return Ok((Some(Part::Data(entry)), next_state));
                    }
                    None => return Ok((None, Data(f, entry_count))),
                },
                End => return Ok((None, End)),
                Invalid => bail_err!(ErrorKind::WirePackDecode("byte stream corrupt".into())),
            }
        }
    }

    #[inline]
    fn next_history_state<P: Into<RepoPath>>(&mut self, filename: P, entry_count: u32) -> State {
        if entry_count == 0 {
            State::DataStart(filename.into())
        } else {
            State::History(filename.into(), entry_count)
        }
    }

    #[inline]
    fn next_data_state<P: Into<RepoPath>>(&mut self, filename: P, entry_count: u32) -> State {
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
                Kind::File => bail_err!(ErrorKind::WirePackDecode(
                    "file packs cannot contain zero-length filenames".into(),
                )),
            }
        } else {
            let mpath = buf.drain_path(filename_len).with_context(|_| {
                let msg = format!("invalid filename of length {}", filename_len);
                ErrorKind::WirePackDecode(msg)
            })?;

            match self.kind {
                Kind::Tree => RepoPath::dir(mpath),
                Kind::File => RepoPath::file(mpath),
            }
            .with_context(|_| ErrorKind::WirePackDecode("invalid filename".into()))?
        };

        trace!(
            self.ctx.logger(),
            "decoding entries for filename: {}",
            filename
        );

        Ok(DecodeRes::Some(filename))
    }

    fn decode_section_start(&mut self, buf: &mut BytesMut) -> Option<u32> {
        if buf.len() < 4 {
            None
        } else {
            Some(buf.drain_u32())
        }
    }

    #[inline]
    fn decode_history(&mut self, buf: &mut BytesMut) -> Result<Option<HistoryEntry>> {
        HistoryEntry::decode(buf, self.kind)
    }

    #[inline]
    fn decode_data(&mut self, buf: &mut BytesMut) -> Result<Option<DataEntry>> {
        DataEntry::decode(buf, DataEntryVersion::V1)
    }
}

#[derive(Debug, Eq, PartialEq)]
enum State {
    Filename,
    HistoryStart(RepoPath),
    History(RepoPath, u32),
    DataStart(RepoPath),
    Data(RepoPath, u32),
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
    use std::io::Cursor;

    use fbinit::FacebookInit;
    use futures::{Future, Stream};
    use tokio_codec::FramedRead;

    use super::*;

    #[fbinit::test]
    fn test_empty(fb: FacebookInit) {
        let ctx = CoreContext::test_mock(fb);

        // Absolutely nothing in here.
        let empty_1 = Cursor::new(WIREPACK_END);
        let unpacker = new(ctx.clone(), Kind::Tree);
        let stream = FramedRead::new(empty_1, unpacker);
        let collect_fut = stream.collect();

        let fut = collect_fut
            .and_then(move |parts| {
                assert_eq!(parts, vec![Part::End]);

                // A file with no entries:
                // - filename b"\0\x03foo"
                // - history count: b"\0\0\0\0"
                // - data count: b"\0\0\0\0"
                // - next filename, end of stream: b"\0\0\0\0\0\0\0\0\0\0"
                let empty_2 = Cursor::new(b"\0\x03foo\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0");
                let unpacker = new(ctx.clone(), Kind::File);
                let stream = FramedRead::new(empty_2, unpacker);
                stream.collect()
            })
            .map(|parts| {
                let foo_dir = RepoPath::file("foo").unwrap();
                assert_eq!(
                    parts,
                    vec![
                        Part::HistoryMeta {
                            path: foo_dir.clone(),
                            entry_count: 0,
                        },
                        Part::DataMeta {
                            path: foo_dir,
                            entry_count: 0,
                        },
                        Part::End,
                    ]
                );
            })
            .map_err(|err| panic!("{:#?}", err));

        tokio::run(fut);
    }
}
