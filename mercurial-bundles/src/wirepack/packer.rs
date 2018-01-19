// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! Packing wirepacks to be sent over the wire during e.g. an hg pull.
//! The format is documented at
//! https://bitbucket.org/facebook/hg-experimental/src/@/remotefilelog/wirepack.py.

use std::mem;

use byteorder::BigEndian;
use bytes::BufMut;
use futures::{Async, Poll, Stream};

use mercurial_types::{MPath, RepoPath};

use chunk::Chunk;
use errors::*;

use super::{DataEntry, HistoryEntry, Kind, Part, WIREPACK_END};

pub struct WirePackPacker<S> {
    part_stream: S,
    kind: Kind,
    state: State,
}

impl<S> WirePackPacker<S> {
    pub fn new(part_stream: S, kind: Kind) -> Self {
        Self {
            part_stream,
            kind,
            state: State::HistoryMeta,
        }
    }
}

impl<S> Stream for WirePackPacker<S>
where
    S: Stream<Item = Part, Error = Error>,
{
    type Item = Chunk;
    type Error = Error;

    fn poll(&mut self) -> Poll<Option<Chunk>, Error> {
        use self::Part::*;

        if self.state == State::End {
            // The stream is over.
            return Ok(Async::Ready(None));
        }

        let mut builder = ChunkBuilder::new(self.kind);

        match try_ready!(self.part_stream.poll()) {
            None => {
                self.state.seen_none()?;
                return Ok(Async::Ready(None));
            }
            Some(HistoryMeta { path, entry_count }) => {
                builder.encode_filename(&path)?;
                builder.encode_entry_count(entry_count);
                // seen_history_meta comes afterwards because we have to transfer ownership of path
                self.state.seen_history_meta(path, entry_count)?;
            }

            Some(History(history_entry)) => {
                self.state.seen_history(&history_entry)?;
                builder.encode_history(&history_entry)?;
            }

            Some(DataMeta { path, entry_count }) => {
                self.state.seen_data_meta(path, entry_count)?;
                builder.encode_entry_count(entry_count);
            }

            Some(Data(data_entry)) => {
                self.state.seen_data(&data_entry)?;
                builder.encode_data(&data_entry)?;
            }

            Some(End) => {
                self.state.seen_end()?;
                builder.encode_end();
            }
        }

        Ok(Async::Ready(Some(builder.build()?)))
    }
}

#[derive(Debug, PartialEq)]
enum State {
    HistoryMeta,
    History { path: RepoPath, entry_count: u32 },
    DataMeta { path: RepoPath },
    Data { path: RepoPath, entry_count: u32 },
    End,
    Invalid,
}

impl State {
    fn next_history_state(path: RepoPath, entry_count: u32) -> Self {
        if entry_count == 0 {
            State::DataMeta { path }
        } else {
            State::History { path, entry_count }
        }
    }

    fn next_data_state(path: RepoPath, entry_count: u32) -> Self {
        if entry_count == 0 {
            State::HistoryMeta
        } else {
            State::Data { path, entry_count }
        }
    }

    fn seen_history_meta(&mut self, path: RepoPath, entry_count: u32) -> Result<()> {
        let state = mem::replace(self, State::Invalid);
        *self = match state {
            State::HistoryMeta => Self::next_history_state(path, entry_count),
            other => {
                bail_err!(ErrorKind::WirePackEncode(format!(
                    "invalid encode stream: unexpected history meta entry (state: {:?})",
                    other
                )));
            }
        };
        Ok(())
    }

    fn seen_history(&mut self, entry: &HistoryEntry) -> Result<()> {
        let state = mem::replace(self, State::Invalid);
        *self = match state {
            State::History { path, entry_count } => {
                ensure_err!(
                    entry_count > 0,
                    ErrorKind::WirePackEncode(format!(
                        "invalid encode stream: saw history entry for {} after count dropped to 0",
                        entry.node
                    ))
                );
                Self::next_history_state(path, entry_count - 1)
            }
            other => {
                bail_err!(ErrorKind::WirePackEncode(format!(
                    "invalid encode stream: unexpected history entry for {} (state: {:?})",
                    entry.node, other
                )));
            }
        };
        Ok(())
    }

    fn seen_data_meta(&mut self, path: RepoPath, entry_count: u32) -> Result<()> {
        let state = mem::replace(self, State::Invalid);
        *self = match state {
            State::DataMeta {
                path: expected_path,
            } => {
                ensure_err!(
                    path == expected_path,
                    ErrorKind::WirePackEncode(format!(
                        "invalid encode stream: saw data meta for path '{}', expected path '{}'\
                         (entry_count: {})",
                        path, expected_path, entry_count
                    ))
                );
                Self::next_data_state(path, entry_count)
            }
            other => {
                bail_err!(ErrorKind::WirePackEncode(format!(
                    "invalid encode stream: saw unexpected data meta for {} (entry count: {}, \
                     state: {:?}",
                    path, entry_count, other
                ),));
            }
        };
        Ok(())
    }

    fn seen_data(&mut self, entry: &DataEntry) -> Result<()> {
        let state = mem::replace(self, State::Invalid);
        *self = match state {
            State::Data { path, entry_count } => {
                ensure_err!(
                    entry_count > 0,
                    ErrorKind::WirePackEncode(format!(
                        "invalid encode stream: saw history entry for {} after count dropped to 0",
                        entry.node
                    ))
                );
                Self::next_data_state(path, entry_count - 1)
            }
            other => {
                bail_err!(ErrorKind::WirePackEncode(format!(
                    "invalid encode stream: unexpected data entry for {} (state: {:?})",
                    entry.node, other
                )));
            }
        };
        Ok(())
    }

    fn seen_end(&mut self) -> Result<()> {
        let state = mem::replace(self, State::Invalid);
        *self = match state {
            State::HistoryMeta => State::End,
            other => {
                bail_err!(ErrorKind::WirePackEncode(format!(
                    "invalid encode stream: unexpected end (state: {:?})",
                    other
                )));
            }
        };
        Ok(())
    }

    fn seen_none(&mut self) -> Result<()> {
        let state = mem::replace(self, State::Invalid);
        *self = match state {
            State::End => State::End,
            other => {
                bail_err!(ErrorKind::WirePackEncode(format!(
                    "invalid encode stream: unexpected None (state: {:?})",
                    other
                )));
            }
        };
        Ok(())
    }
}

#[derive(Debug)]
struct ChunkBuilder {
    kind: Kind,
    inner: Vec<u8>,
}

impl ChunkBuilder {
    pub fn new(kind: Kind) -> Self {
        Self {
            kind,
            inner: Vec::with_capacity(256),
        }
    }

    /// Encode a filename -- this should always happen before any history or data entries are
    /// encoded.
    fn encode_filename(&mut self, filename: &RepoPath) -> Result<&mut Self> {
        let empty = MPath::empty();

        let mpath = match (self.kind, filename) {
            (Kind::Tree, &RepoPath::RootPath) => &empty,
            (Kind::File, &RepoPath::RootPath) => bail_err!(ErrorKind::WirePackEncode(
                "attempted to encode a zero-length filename into a file wirepack".into()
            )),
            (Kind::Tree, &RepoPath::DirectoryPath(ref dir_path)) => verify_path(dir_path)?,
            (Kind::File, &RepoPath::FilePath(ref file_path)) => verify_path(file_path)?,
            (kind, path) => bail_err!(ErrorKind::WirePackEncode(format!(
                "attempted to encode incompatible path into wirepack (kind: {}, path: {:?})",
                kind, path
            ))),
        };

        self.inner.put_u16::<BigEndian>(mpath.len() as u16);
        mpath.generate(&mut self.inner)?;
        Ok(self)
    }

    #[inline]
    fn encode_entry_count(&mut self, entry_count: u32) -> &mut Self {
        self.inner.put_u32::<BigEndian>(entry_count);
        self
    }

    #[inline]
    fn encode_history(&mut self, history_entry: &HistoryEntry) -> Result<&mut Self> {
        history_entry.encode(self.kind, &mut self.inner)?;
        Ok(self)
    }

    #[inline]
    fn encode_data(&mut self, data_entry: &DataEntry) -> Result<&mut Self> {
        data_entry.encode(&mut self.inner)?;
        Ok(self)
    }

    #[inline]
    fn encode_end(&mut self) -> &mut Self {
        self.inner.put_slice(WIREPACK_END);
        self
    }

    #[inline]
    fn build(self) -> Result<Chunk> {
        Chunk::new(self.inner)
    }
}

fn verify_path<'a>(mpath: &'a MPath) -> Result<&'a MPath> {
    let len = mpath.len();
    if len == 0 {
        bail_err!(ErrorKind::WirePackEncode(format!(
            "attempted to encode a zero-length filename"
        )));
    }
    if len > (u16::max_value() as usize) {
        bail_err!(ErrorKind::WirePackEncode(format!(
            "attempted to encode a filename of length {} -- maximum length supported is {}",
            len,
            u16::max_value()
        )));
    }
    Ok(mpath)
}
