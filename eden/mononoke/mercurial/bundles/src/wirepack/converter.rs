/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Given a stream of wirepack entries, convert it to to other stream, using WirePackPartProcessor

use std::mem;

use anyhow::bail;
use anyhow::ensure;
use anyhow::Result;
use async_stream::try_stream;
use futures::pin_mut;
use futures::Stream;
use futures::TryStreamExt;
use mercurial_types::RepoPath;

use super::DataEntry;
use super::HistoryEntry;
use super::Part;
use crate::errors::ErrorKind;

pub trait WirePackPartProcessor {
    type Data;

    fn history_meta(&mut self, path: &RepoPath, entry_count: u32) -> Result<Option<Self::Data>>;
    fn history(&mut self, entry: &HistoryEntry) -> Result<Option<Self::Data>>;
    fn data_meta(&mut self, path: &RepoPath, entry_count: u32) -> Result<Option<Self::Data>>;
    fn data(&mut self, data_entry: &DataEntry) -> Result<Option<Self::Data>>;
    fn end(&mut self) -> Result<Option<Self::Data>>;
}

pub fn convert_wirepack<S, P>(
    part_stream: S,
    mut processor: P,
) -> impl Stream<Item = Result<<P as WirePackPartProcessor>::Data>>
where
    S: Stream<Item = Result<Part>>,
    P: WirePackPartProcessor,
{
    try_stream! {
        let mut state = State::HistoryMeta;
        pin_mut!(part_stream);

        while let Some(part) = part_stream.try_next().await? {
            match part {
                Part::HistoryMeta { path, entry_count } => {
                    // seen_history_meta comes afterwards because we have to transfer
                    // ownership of path
                    if let Some(history_meta) = processor.history_meta(&path, entry_count)? {
                        state.seen_history_meta(path, entry_count)?;
                        yield history_meta;
                    } else {
                        state.seen_history_meta(path, entry_count)?;
                    }
                }

                Part::History(history_entry) => {
                    state.seen_history(&history_entry)?;
                    if let Some(encoded_history) = processor.history(&history_entry)? {
                        yield encoded_history;
                    }
                }

                Part::DataMeta { path, entry_count } => {
                    if let Some(data_meta) = processor.data_meta(&path, entry_count)? {
                        state.seen_data_meta(path, entry_count)?;
                        yield data_meta;
                    } else {
                        state.seen_data_meta(path, entry_count)?;
                    }
                }

                Part::Data(data_entry) => {
                    state.seen_data(&data_entry)?;
                    if let Some(data_entry) = processor.data(&data_entry)? {
                        yield data_entry;
                    }
                }

                Part::End => {
                    state.seen_end()?;
                    if let Some(end) = processor.end()? {
                        yield end;
                    }
                }
            }
        }

        if state != State::End {
            Err(ErrorKind::WirePackEncode(format!(
                "invalid encode stream: unexpected None (state: {:?})",
                state
            )))?;
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
                ensure!(
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
                ensure!(
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
                ensure!(
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
}
