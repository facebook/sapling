/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

//! Unpacking changegroups.
//! See https://www.mercurial-scm.org/repo/hg/file/@/mercurial/help/internals/changegroups.txt.

use std::cmp;
use std::mem;

use anyhow::{bail, format_err, Context, Error, Result};
use bytes::BytesMut;
use slog::Logger;
use std::str::FromStr;
use tokio_io::codec::Decoder;

use mercurial_types::{MPath, RevFlags};

use crate::delta;
use crate::errors::ErrorKind;
use crate::utils::BytesExt;

use super::{CgDeltaChunk, Part, Section};

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum CgVersion {
    Cg2Version,
    Cg3Version,
}

impl CgVersion {
    pub fn to_str(&self) -> &str {
        match self {
            CgVersion::Cg2Version => "02",
            CgVersion::Cg3Version => "03",
        }
    }
}

impl FromStr for CgVersion {
    type Err = Error;

    fn from_str(s: &str) -> Result<CgVersion> {
        match s {
            "02" => Ok(CgVersion::Cg2Version),
            "03" => Ok(CgVersion::Cg3Version),
            bad => Err(ErrorKind::CgDecode(format!(
                "Non supported Cg version in Part Header {}",
                bad
            ))
            .into()),
        }
    }
}

// See the chunk header definition below for the first 100 bytes. The last 4 is
// for the length field itself. _CHANGEGROUPV2_DELTA_HEADER = "20s20s20s20s20s"
const CHUNK_HEADER2_LEN: usize = 20 + 20 + 20 + 20 + 20 + 4;

// See the chunk header definition below for the first 102 bytes. The last 4 is
// for the length field itself. _CHANGEGROUPV3_DELTA_HEADER = ">20s20s20s20s20sH"
const CHUNK_HEADER3_LEN: usize = 20 + 20 + 20 + 20 + 20 + 2 + 4;

#[derive(Debug)]
pub struct CgUnpacker {
    logger: Logger,
    state: State,
    version: CgVersion,
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

impl Decoder for CgUnpacker {
    type Item = Part;
    type Error = Error;

    fn decode(&mut self, buf: &mut BytesMut) -> Result<Option<Self::Item>> {
        match Self::decode_next(buf, self.state.take(), &self.version) {
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
                        "incomplete changegroup: {} bytes remaining in \
                         buffer. State: {:?}, First 128 bytes: {:?}",
                        len, self.state, bytes,
                    );
                    bail!(ErrorKind::CgDecode(msg));
                }
                if self.state != State::End {
                    let msg = format!(
                        "incomplete changegroup: expected state End, found {:?}",
                        self.state
                    );
                    bail!(ErrorKind::CgDecode(msg));
                }
                Ok(None)
            }
            Some(v) => Ok(Some(v)),
        }
    }
}

impl CgUnpacker {
    pub fn new(logger: Logger, version: CgVersion) -> Self {
        CgUnpacker {
            logger,
            state: State::Changeset,
            version,
        }
    }

    fn chunk_header_len(version: &CgVersion) -> usize {
        match version {
            CgVersion::Cg2Version => CHUNK_HEADER2_LEN,
            CgVersion::Cg3Version => CHUNK_HEADER3_LEN,
        }
    }

    fn decode_next(
        buf: &mut BytesMut,
        state: State,
        version: &CgVersion,
    ) -> Result<(Option<Part>, State)> {
        match state {
            State::Changeset => match Self::decode_chunk(buf, version)? {
                None => Ok((None, State::Changeset)),
                Some(CgChunk::Empty) => {
                    Ok((Some(Part::SectionEnd(Section::Changeset)), State::Manifest))
                }
                Some(CgChunk::Delta(chunk)) => Ok((
                    Some(Part::CgChunk(Section::Changeset, chunk)),
                    State::Changeset,
                )),
            },
            State::Manifest => match Self::decode_chunk(buf, version)? {
                None => Ok((None, State::Manifest)),
                Some(CgChunk::Empty) => {
                    let next_state = match version {
                        CgVersion::Cg2Version => State::Filename,
                        CgVersion::Cg3Version => State::Treemanifest,
                    };
                    Ok((Some(Part::SectionEnd(Section::Manifest)), next_state))
                }
                Some(CgChunk::Delta(chunk)) => Ok((
                    Some(Part::CgChunk(Section::Manifest, chunk)),
                    State::Manifest,
                )),
            },
            State::Treemanifest => match Self::decode_chunk(buf, version)? {
                None => Ok((None, State::Treemanifest)),
                Some(CgChunk::Empty) => Ok((
                    Some(Part::SectionEnd(Section::Treemanifest)),
                    State::Filename,
                )),
                Some(CgChunk::Delta(_)) => {
                    Err(ErrorKind::CgDecode("Empty TreeManifest has expected".into()).into())
                }
            },
            State::Filename => {
                let filename = Self::decode_filename(buf)?;
                match filename {
                    DecodeRes::None => Ok((None, State::Filename)),
                    DecodeRes::Some(f) => Self::decode_filelog_chunk(buf, f, version),
                    DecodeRes::End => Ok((Some(Part::End), State::End)),
                }
            }
            State::Filelog(filename) => Self::decode_filelog_chunk(buf, filename, version),
            State::End => Ok((None, State::End)),
            State::Invalid => Err(ErrorKind::CgDecode("byte stream corrupt".into()).into()),
        }
    }

    fn decode_filelog_chunk(
        buf: &mut BytesMut,
        f: MPath,
        version: &CgVersion,
    ) -> Result<(Option<Part>, State)> {
        match Self::decode_chunk(buf, version)? {
            None => Ok((None, State::Filelog(f))),
            Some(CgChunk::Empty) => {
                Ok((Some(Part::SectionEnd(Section::Filelog(f))), State::Filename))
            }
            Some(CgChunk::Delta(chunk)) => Ok((
                Some(Part::CgChunk(Section::Filelog(f.clone()), chunk)),
                State::Filelog(f),
            )),
        }
    }

    fn decode_chunk(buf: &mut BytesMut, version: &CgVersion) -> Result<Option<CgChunk>> {
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
        if chunk_len < Self::chunk_header_len(version) {
            let msg = format!(
                "invalid chunk for version {:?}: length >= {} required, found {}",
                version,
                Self::chunk_header_len(version),
                chunk_len,
            );
            bail!(ErrorKind::CgDecode(msg));
        }

        if buf.len() < chunk_len {
            return Ok(None);
        }
        Self::decode_delta(buf, chunk_len, version)
    }

    fn decode_delta(
        buf: &mut BytesMut,
        chunk_len: usize,
        version: &CgVersion,
    ) -> Result<Option<CgChunk>> {
        let _ = buf.drain_i32();

        // A chunk header has:
        // ---
        // node: HgNodeHash (20 bytes)
        // p1: HgNodeHash (20 bytes)
        // p2: HgNodeHash (20 bytes) -- NULL_HASH if only 1 parent
        // base node: HgNodeHash (20 bytes) (new in changegroup2)
        // link node: HgNodeHash (20 bytes)
        // flags: unsigned short (2 bytes) -- (version 3 only)
        // ---

        let node = buf.drain_node();
        let p1 = buf.drain_node();
        let p2 = buf.drain_node();
        let base = buf.drain_node();
        let linknode = buf.drain_node();
        let flags = match version {
            CgVersion::Cg2Version => None,
            CgVersion::Cg3Version => {
                let bits = buf.drain_u16();
                let flags = RevFlags::from_bits(bits)
                    .ok_or(format_err!("unknown revlog flags: {}", bits))?;
                Some(flags)
            }
        };

        let delta = delta::decode_delta(buf.split_to(chunk_len - Self::chunk_header_len(version)))?;

        return Ok(Some(CgChunk::Delta(CgDeltaChunk {
            node,
            p1,
            p2,
            base,
            linknode,
            delta,
            flags,
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
        let filename = buf.drain_path(filename_len - 4).with_context(|| {
            let msg = format!("invalid filename of length {}", filename_len);
            ErrorKind::CgDecode(msg)
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
    Treemanifest,
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
