// Copyright (c) 2017-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! Wire packs. The format is currently undocumented.

use byteorder::{BigEndian, ByteOrder};
use bytes::BytesMut;

use mercurial_types::{Delta, NodeHash, RepoPath, NULL_HASH};

use delta;
use errors::*;
use utils::BytesExt;

pub mod unpacker;

/// What sort of wirepack this is.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Kind {
    /// A wire pack representing tree manifests.
    Tree,
    /// A wire pack representing file contents.
    File,
}

/// An atomic part returned from the wirepack.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Part {
    History(RepoPath, HistoryEntry),
    Data(RepoPath, DataEntry),
    End,
}

impl Part {
    #[cfg(test)]
    pub(crate) fn unwrap_history(self) -> (RepoPath, HistoryEntry) {
        match self {
            Part::History(path, entry) => (path, entry),
            other => panic!("expected wirepack part to be History, was {:?}", other),
        }
    }

    #[cfg(test)]
    pub(crate) fn unwrap_data(self) -> (RepoPath, DataEntry) {
        match self {
            Part::Data(path, entry) => (path, entry),
            other => panic!("expected wirepack part to be Data, was {:?}", other),
        }
    }
}

// See the history header definition in this file for the breakdown.
const HISTORY_COPY_FROM_OFFSET: usize = 20 + 20 + 20 + 20;
const HISTORY_HEADER_SIZE: usize = HISTORY_COPY_FROM_OFFSET + 2;

// See the data header definition in this file for the breakdown.
const DATA_DELTA_OFFSET: usize = 20 + 20;
const DATA_HEADER_SIZE: usize = DATA_DELTA_OFFSET + 8;

// TODO: move to mercurial-types
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HistoryEntry {
    pub node: NodeHash,
    // TODO: replace with Parents?
    pub p1: NodeHash,
    pub p2: NodeHash,
    pub linknode: NodeHash,
    pub copy_from: Option<RepoPath>,
}

impl HistoryEntry {
    pub(crate) fn decode(buf: &mut BytesMut, kind: Kind) -> Result<Option<Self>> {
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
            match kind {
                Kind::Tree => bail_err!(ErrorKind::WirePackDecode(format!(
                    "tree entry {} is marked as copied from path {}, but they cannot be copied",
                    node,
                    path
                ))),
                Kind::File => Some(RepoPath::file(path).with_context(|_| {
                    ErrorKind::WirePackDecode("invalid copy from path".into())
                })?),
            }
        } else {
            None
        };
        Ok(Some(Self {
            node,
            p1,
            p2,
            linknode,
            copy_from,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DataEntry {
    pub node: NodeHash,
    pub delta_base: NodeHash,
    pub delta: Delta,
}

impl DataEntry {
    pub(crate) fn decode(buf: &mut BytesMut) -> Result<Option<Self>> {
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

        Ok(Some(Self {
            node,
            delta_base,
            delta,
        }))
    }
}
