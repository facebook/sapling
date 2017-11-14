// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! Unpacking changegroups.
//! See https://www.mercurial-scm.org/repo/hg/file/@/mercurial/help/internals/changegroups.txt.

use std::cmp;
use std::mem;

use bytes::BytesMut;
use slog;
use tokio_io::codec::Decoder;

use mercurial_types::MPath;

use InnerPart;
use delta;
use errors::*;
use utils::BytesExt;

use super::{CgDeltaChunk, Part, Section};

#[derive(Debug)]
pub struct Cg2Unpacker {
    logger: slog::Logger,
    state: State,
}

impl Part {
    /// Gets the section this part is from.
    ///
    /// # Panics
    ///
    /// When self does not contain a Section.
    pub fn section(&self) -> &Section {
        match self {
            &Part::CgChunk(ref section, _) => section,
            &Part::SectionEnd(ref section) => section,
            _ => panic!("this Part does not contain a Section"),
        }
    }

    /// Gets the chunk inside this part.
    ///
    /// # Panics
    ///
    /// When self does not contain a CgDeltaChunk (it is a SectionEnd or End).
    pub fn chunk(&self) -> &CgDeltaChunk {
        match self {
            &Part::CgChunk(_, ref chunk) => chunk,
            _ => panic!("this Part does not contain a CgDeltaChunk"),
        }
    }
}

// See the chunk header definition below for the first 100 bytes. The last 4 is
// for the length field itself.
const CHUNK_HEADER_LEN: usize = 20 + 20 + 20 + 20 + 20 + 4;

impl Decoder for Cg2Unpacker {
    type Item = InnerPart;
    type Error = Error;

    fn decode(&mut self, buf: &mut BytesMut) -> Result<Option<InnerPart>> {
        match Self::decode_next(buf, self.state.take()) {
            Err(e) => {
                self.state = State::Invalid;
                Err(e)
            }
            Ok((ret, state)) => {
                self.state = state;
                match ret {
                    None => Ok(None),
                    Some(v) => Ok(Some(InnerPart::Cg2(v))),
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
                        "incomplete changegroup: {} bytes remaining in \
                         buffer. State: {:?}, First 128 bytes: {:?}",
                        len,
                        self.state,
                        bytes,
                    );
                    bail!(ErrorKind::Cg2Decode(msg));
                }
                if self.state != State::End {
                    let msg = format!(
                        "incomplete changegroup: expected state End, found {:?}",
                        self.state
                    );
                    bail!(ErrorKind::Cg2Decode(msg))
                }
                Ok(None)
            }
            Some(v) => Ok(Some(v)),
        }
    }
}

impl Cg2Unpacker {
    pub fn new(logger: slog::Logger) -> Self {
        Cg2Unpacker {
            logger: logger,
            state: State::Changeset,
        }
    }

    fn decode_next(buf: &mut BytesMut, state: State) -> Result<(Option<Part>, State)> {
        match state {
            State::Changeset => match Self::decode_chunk(buf)? {
                None => Ok((None, State::Changeset)),
                Some(CgChunk::Empty) => Ok((
                    Some(Part::SectionEnd(Section::Changeset)),
                    State::Manifest,
                )),
                Some(CgChunk::Delta(chunk)) => Ok((
                    Some(Part::CgChunk(Section::Changeset, chunk)),
                    State::Changeset,
                )),
            },
            State::Manifest => match Self::decode_chunk(buf)? {
                None => Ok((None, State::Manifest)),
                Some(CgChunk::Empty) => {
                    Ok((Some(Part::SectionEnd(Section::Manifest)), State::Filename))
                }
                Some(CgChunk::Delta(chunk)) => Ok((
                    Some(Part::CgChunk(Section::Manifest, chunk)),
                    State::Manifest,
                )),
            },
            State::Filename => {
                let filename = Self::decode_filename(buf)?;
                match filename {
                    DecodeRes::None => Ok((None, State::Filename)),
                    DecodeRes::Some(f) => Self::decode_filelog_chunk(buf, f),
                    DecodeRes::End => Ok((Some(Part::End), State::End)),
                }
            }
            State::Filelog(filename) => Self::decode_filelog_chunk(buf, filename),
            State::End => Ok((None, State::End)),
            State::Invalid => Err(ErrorKind::Cg2Decode("byte stream corrupt".into()).into()),
        }
    }

    fn decode_filelog_chunk(buf: &mut BytesMut, f: MPath) -> Result<(Option<Part>, State)> {
        match Self::decode_chunk(buf)? {
            None => Ok((None, State::Filelog(f))),
            Some(CgChunk::Empty) => Ok((
                Some(Part::SectionEnd(Section::Filelog(f))),
                State::Filename,
            )),
            Some(CgChunk::Delta(chunk)) => Ok((
                Some(Part::CgChunk(Section::Filelog(f.clone()), chunk)),
                State::Filelog(f),
            )),
        }
    }

    fn decode_chunk(buf: &mut BytesMut) -> Result<Option<CgChunk>> {
        if buf.len() < 4 {
            return Ok(None);
        }

        let chunk_len = buf.peek_i32();
        // Note that chunk_len includes the 4 bytes consumed by itself
        // TODO: chunk_len < 0 = error
        let chunk_len = chunk_len as usize;
        if chunk_len == 0 {
            let _ = buf.drain_i32();
            return Ok(Some(CgChunk::Empty));
        }
        if chunk_len < CHUNK_HEADER_LEN {
            let msg = format!(
                "invalid chunk: length >= {} required, found {}",
                CHUNK_HEADER_LEN,
                chunk_len
            );
            bail!(ErrorKind::Cg2Decode(msg));
        }

        if buf.len() < chunk_len {
            return Ok(None);
        }
        let _ = buf.drain_i32();

        // A chunk header has:
        // ---
        // node: NodeHash (20 bytes)
        // p1: NodeHash (20 bytes)
        // p2: NodeHash (20 bytes) -- NULL_HASH if only 1 parent
        // base node: NodeHash (20 bytes) (new in changegroup2)
        // link node: NodeHash (20 bytes)
        // ---

        let node = buf.drain_node();
        let p1 = buf.drain_node();
        let p2 = buf.drain_node();
        let base = buf.drain_node();
        let linknode = buf.drain_node();

        let delta = delta::decode_delta(buf.split_to(chunk_len - CHUNK_HEADER_LEN))?;
        return Ok(Some(CgChunk::Delta(CgDeltaChunk {
            node: node,
            p1: p1,
            p2: p2,
            base: base,
            linknode: linknode,
            delta: delta,
        })));
    }

    fn decode_filename(buf: &mut BytesMut) -> Result<DecodeRes<MPath>> {
        if buf.len() < 4 {
            return Ok(DecodeRes::None);
        }
        let filename_len = buf.peek_i32();
        // TODO: filename_len < 0 == error
        if filename_len == 0 {
            let _ = buf.split_to(4);
            return Ok(DecodeRes::End);
        }
        let filename_len = filename_len as usize;
        // filename_len includes the 4 bytes for the length field.
        if buf.len() < filename_len {
            return Ok(DecodeRes::None);
        }
        let _ = buf.split_to(4);
        let filename = buf.drain_path(filename_len - 4).chain_err(|| {
            let msg = format!("invalid filename of length {}", filename_len);
            ErrorKind::Cg2Decode(msg)
        })?;
        Ok(DecodeRes::Some(filename))
    }
}

enum DecodeRes<T> {
    None,
    Some(T),
    End,
}

enum CgChunk {
    Delta(CgDeltaChunk),
    Empty,
}

#[derive(Debug, Eq, PartialEq)]
enum State {
    Changeset,
    Manifest,
    Filename,
    Filelog(MPath),
    End,
    Invalid,
}

impl State {
    pub fn take(&mut self) -> Self {
        mem::replace(self, State::Invalid)
    }
}
