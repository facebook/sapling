/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

//! Given a stream of wirepack entries, convert it to to other stream, using WirePackPartProcessor

use std::mem;

use failure_ext::{bail, ensure_err};
use futures::{try_ready, Async, Poll, Stream};

use mercurial_types::RepoPath;

use super::{DataEntry, HistoryEntry, Part};

use crate::errors::*;

pub trait WirePackPartProcessor {
    type Data;

    fn history_meta(&mut self, path: &RepoPath, entry_count: u32) -> Result<Option<Self::Data>>;
    fn history(&mut self, entry: &HistoryEntry) -> Result<Option<Self::Data>>;
    fn data_meta(&mut self, path: &RepoPath, entry_count: u32) -> Result<Option<Self::Data>>;
    fn data(&mut self, data_entry: &DataEntry) -> Result<Option<Self::Data>>;
    fn end(&mut self) -> Result<Option<Self::Data>>;
}

pub struct WirePackConverter<S, P> {
    part_stream: S,
    state: State,
    processor: P,
}

impl<S, P> WirePackConverter<S, P> {
    pub fn new(part_stream: S, processor: P) -> Self {
        Self {
            part_stream,
            state: State::HistoryMeta,
            processor,
        }
    }
}

impl<S, P> Stream for WirePackConverter<S, P>
where
    S: Stream<Item = Part, Error = Error>,
    P: WirePackPartProcessor,
{
    type Item = <P as WirePackPartProcessor>::Data;
    type Error = Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Error> {
        use self::Part::*;

        if self.state == State::End {
            // The stream is over.
            return Ok(Async::Ready(None));
        }

        loop {
            match try_ready!(self.part_stream.poll()) {
                None => {
                    self.state.seen_none()?;
                    return Ok(Async::Ready(None));
                }
                Some(HistoryMeta { path, entry_count }) => {
                    if let Some(history_meta) = self.processor.history_meta(&path, entry_count)? {
                        self.state.seen_history_meta(path, entry_count)?;
                        return Ok(Async::Ready(Some(history_meta)));
                    }
                    // seen_history_meta comes afterwards because we have to transfer
                    // ownership of path
                    self.state.seen_history_meta(path, entry_count)?;
                }

                Some(History(history_entry)) => {
                    self.state.seen_history(&history_entry)?;
                    if let Some(encoded_history) = self.processor.history(&history_entry)? {
                        return Ok(Async::Ready(Some(encoded_history)));
                    }
                }

                Some(DataMeta { path, entry_count }) => {
                    if let Some(data_meta) = self.processor.data_meta(&path, entry_count)? {
                        self.state.seen_data_meta(path, entry_count)?;
                        return Ok(Async::Ready(Some(data_meta)));
                    }
                    self.state.seen_data_meta(path, entry_count)?;
                }

                Some(Data(data_entry)) => {
                    self.state.seen_data(&data_entry)?;
                    if let Some(data_entry) = self.processor.data(&data_entry)? {
                        return Ok(Async::Ready(Some(data_entry)));
                    }
                }

                Some(End) => {
                    self.state.seen_end()?;
                    if let Some(end) = self.processor.end()? {
                        return Ok(Async::Ready(Some(end)));
                    }
                }
            }
        }
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
                bail!(ErrorKind::WirePackEncode(format!(
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
                bail!(ErrorKind::WirePackEncode(format!(
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
                bail!(ErrorKind::WirePackEncode(format!(
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
                bail!(ErrorKind::WirePackEncode(format!(
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
                bail!(ErrorKind::WirePackEncode(format!(
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
                bail!(ErrorKind::WirePackEncode(format!(
                    "invalid encode stream: unexpected None (state: {:?})",
                    other
                )));
            }
        };
        Ok(())
    }
}
