// Copyright (c) 2017-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! Wire packs. The format is currently undocumented.

use std::fmt;
use std::io::Cursor;

use byteorder::{BigEndian, ByteOrder};
use bytes::{BufMut, BytesMut};

use mercurial_types::{Delta, HgNodeHash, RepoPath, NULL_HASH};
use revisionstore::Metadata;

use crate::delta;
use crate::errors::*;
use crate::utils::BytesExt;

pub mod converter;
pub mod packer;
#[cfg(test)]
mod quickcheck_types;
pub mod unpacker;

/// What sort of wirepack this is.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Kind {
    /// A wire pack representing tree manifests.
    Tree,
    /// A wire pack representing file contents.
    File,
}

impl fmt::Display for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Kind::Tree => write!(f, "tree"),
            Kind::File => write!(f, "file"),
        }
    }
}

/// An atomic part returned from the wirepack.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Part {
    HistoryMeta { path: RepoPath, entry_count: u32 },
    History(HistoryEntry),
    DataMeta { path: RepoPath, entry_count: u32 },
    Data(DataEntry),
    End,
}

#[cfg(test)]
impl Part {
    pub(crate) fn unwrap_history_meta(self) -> (RepoPath, u32) {
        match self {
            Part::HistoryMeta { path, entry_count } => (path, entry_count),
            other => panic!("expected wirepack part to be HistoryMeta, was {:?}", other),
        }
    }

    pub(crate) fn unwrap_history(self) -> HistoryEntry {
        match self {
            Part::History(entry) => entry,
            other => panic!("expected wirepack part to be History, was {:?}", other),
        }
    }

    pub(crate) fn unwrap_data_meta(self) -> (RepoPath, u32) {
        match self {
            Part::DataMeta { path, entry_count } => (path, entry_count),
            other => panic!("expected wirepack part to be HistoryMeta, was {:?}", other),
        }
    }

    pub(crate) fn unwrap_data(self) -> DataEntry {
        match self {
            Part::Data(entry) => entry,
            other => panic!("expected wirepack part to be Data, was {:?}", other),
        }
    }
}

const WIREPACK_END: &[u8] = b"\0\0\0\0\0\0\0\0\0\0";

// See the history header definition in this file for the breakdown.
const HISTORY_COPY_FROM_OFFSET: usize = 20 + 20 + 20 + 20;
const HISTORY_HEADER_SIZE: usize = HISTORY_COPY_FROM_OFFSET + 2;

// See the data header definition in this file for the breakdown.
const DATA_DELTA_OFFSET: usize = 20 + 20;
const DATA_HEADER_SIZE: usize = DATA_DELTA_OFFSET + 8;

// TODO: move to mercurial_types
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HistoryEntry {
    pub node: HgNodeHash,
    // TODO: replace with HgParents?
    pub p1: HgNodeHash,
    pub p2: HgNodeHash,
    pub linknode: HgNodeHash,
    pub copy_from: Option<RepoPath>,
}

impl HistoryEntry {
    pub(crate) fn decode(buf: &mut BytesMut, kind: Kind) -> Result<Option<Self>> {
        if buf.len() < HISTORY_HEADER_SIZE {
            return Ok(None);
        }

        // A history revision has:
        // ---
        // node: HgNodeHash (20 bytes)
        // p1: HgNodeHash (20 bytes)
        // p2: HgNodeHash (20 bytes)
        // link node: HgNodeHash (20 bytes)
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
                    node, path
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

    // This would ideally be generic over any BufMut, but that won't be very useful until
    // https://github.com/carllerche/bytes/issues/170 is fixed.
    pub(crate) fn encode(&self, kind: Kind, buf: &mut Vec<u8>) -> Result<()> {
        self.verify(kind).with_context(|_| {
            ErrorKind::WirePackEncode("attempted to encode an invalid history entry".into())
        })?;
        buf.put_slice(self.node.as_ref());
        buf.put_slice(self.p1.as_ref());
        buf.put_slice(self.p2.as_ref());
        buf.put_slice(self.linknode.as_ref());
        let path_vec = if let Some(ref path) = self.copy_from {
            path.mpath()
                .expect("verify ensures that path is always a RepoPath::FilePath")
                .to_vec()
        } else {
            vec![]
        };
        buf.put_u16_be(path_vec.len() as u16);
        buf.put_slice(&path_vec);
        Ok(())
    }

    pub fn verify(&self, kind: Kind) -> Result<()> {
        if let Some(ref path) = self.copy_from {
            match *path {
                RepoPath::RootPath => bail_err!(ErrorKind::InvalidWirePackEntry(format!(
                    "history entry for {} is copied from the root path, which isn't allowed",
                    self.node
                ))),
                RepoPath::DirectoryPath(ref path) => {
                    bail_err!(ErrorKind::InvalidWirePackEntry(format!(
                        "history entry for {} is copied from directory {}, which isn't allowed",
                        self.node, path
                    )))
                }
                RepoPath::FilePath(ref path) => {
                    ensure_err!(
                        kind == Kind::File,
                        ErrorKind::InvalidWirePackEntry(format!(
                            "history entry for {} is copied from file {}, but the pack is of \
                             kind {}",
                            self.node, path, kind
                        ))
                    );
                    ensure_err!(
                        path.len() <= (u16::max_value() as usize),
                        ErrorKind::InvalidWirePackEntry(format!(
                            "history entry for {} is copied from a path of length {} -- maximum \
                             length supported is {}",
                            self.node,
                            path.len(),
                            u16::max_value(),
                        ),)
                    );
                }
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DataEntry {
    pub node: HgNodeHash,
    pub delta_base: HgNodeHash,
    pub delta: Delta,
    /// Metadata presence implies a getpack protocol version that supports it (i.e. version 2)
    pub metadata: Option<Metadata>,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum DataEntryVersion {
    V1,
    V2,
}

impl DataEntry {
    pub(crate) fn decode(buf: &mut BytesMut, version: DataEntryVersion) -> Result<Option<Self>> {
        if buf.len() < DATA_HEADER_SIZE {
            return Ok(None);
        }

        // A data revision has:
        // ---
        // node: HgNodeHash (20 bytes)
        // delta base: HgNodeHash (20 bytes) -- NULL_HASH if full text
        // delta len: u64 (8 bytes)
        // delta: Delta (<delta len> bytes)
        // On v2:
        // metadata len: u32 (4 bytes)
        // metadata: [metadata item, ...] (<metadata len> bytes)
        // metadata item:
        //  metadata key: u8 (1 byte)
        //  metadata value len: u16 (2 bytes)
        //  metadata value: Bytes (<metadata value len> bytes)
        // ---
        // There's a bit of a wart in the current format: if delta base is NULL_HASH, instead of
        // storing a delta with start = 0 and end = 0, we store the full text directly. This
        // should be fixed in a future wire protocol revision.

        // First, check that we have enough data to proceed.
        let delta_len = BigEndian::read_u64(&buf[DATA_DELTA_OFFSET..DATA_HEADER_SIZE]) as usize;
        match version {
            DataEntryVersion::V1 => {
                if buf.len() < DATA_HEADER_SIZE + delta_len {
                    return Ok(None);
                }
            }
            DataEntryVersion::V2 => {
                let meta_offset = DATA_HEADER_SIZE + delta_len;
                let meta_header_size = 4; // Metadata header is a u32.
                if buf.len() < meta_offset + meta_header_size {
                    return Ok(None);
                }

                let meta_header_slice = &buf[meta_offset..meta_offset + meta_header_size];
                let meta_size = BigEndian::read_u32(meta_header_slice) as usize;
                if buf.len() < meta_offset + meta_header_size + meta_size {
                    return Ok(None);
                }
            }
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

        let metadata = match version {
            DataEntryVersion::V1 => None,
            DataEntryVersion::V2 => {
                let mut cursor = Cursor::new(buf.as_ref());
                let metadata = Metadata::read(&mut cursor)?;

                // Metadata::read doesn't consume bytes (it just reads them), but our
                // implementation here wants to consume bytes we read. We work around this here.
                let pos = cursor.position();
                std::mem::drop(cursor);
                let _ = buf.split_to(pos as usize);

                Some(metadata)
            }
        };

        Ok(Some(Self {
            node,
            delta_base,
            delta,
            metadata,
        }))
    }

    // This would ideally be generic over any BufMut, but that won't be very useful until
    // https://github.com/carllerche/bytes/issues/170 is fixed.
    pub(crate) fn encode(&self, buf: &mut Vec<u8>) -> Result<()> {
        self.verify()?;
        buf.put_slice(self.node.as_ref());
        buf.put_slice(self.delta_base.as_ref());
        if self.delta_base == NULL_HASH {
            // This is a fulltext -- the spec requires that instead of storing a delta with
            // start = 0 and end = 0, the fulltext be stored directly.
            let fulltext = self
                .delta
                .maybe_fulltext()
                .expect("verify will have already checked that the delta is a fulltext");
            buf.put_u64_be(fulltext.len() as u64);
            buf.put_slice(fulltext);
        } else {
            buf.put_u64_be(delta::encoded_len(&self.delta) as u64);
            delta::encode_delta(&self.delta, buf);
        }

        if let Some(ref metadata) = self.metadata {
            metadata.write(buf)?
        }

        Ok(())
    }

    pub fn verify(&self) -> Result<()> {
        // The only limitation is that the delta base being null means that the revision is a
        // fulltext.
        ensure_err!(
            self.delta_base != NULL_HASH || self.delta.maybe_fulltext().is_some(),
            ErrorKind::InvalidWirePackEntry(format!(
                "data entry for {} has a null base but is not a fulltext",
                self.node
            ))
        );
        Ok(())
    }
}

#[cfg(test)]
mod test {
    use std::cmp;

    use quickcheck::rand::{self, Rng};
    use quickcheck::{Gen, StdGen};

    use mercurial_types::delta::Fragment;
    use mercurial_types_mocks::nodehash::{AS_HASH, BS_HASH};

    use super::*;

    #[test]
    fn test_history_verify_basic() {
        let foo_dir = RepoPath::dir("foo").unwrap();
        let bar_file = RepoPath::file("bar").unwrap();
        let root = RepoPath::root();

        let valid = hashset! {
            (Kind::Tree, None),
            (Kind::File, None),
            (Kind::File, Some(bar_file.clone())),
        };
        // Can't use arrays here because IntoIterator isn't supported for them:
        // https://github.com/rust-lang/rust/issues/25725
        let kinds = vec![Kind::Tree, Kind::File].into_iter();
        let copy_froms = vec![None, Some(foo_dir), Some(bar_file), Some(root)].into_iter();

        for pair in iproduct!(kinds, copy_froms) {
            let is_valid = valid.contains(&pair);
            let (kind, copy_from) = pair;
            let entry = make_history_entry(copy_from);
            let result = entry.verify(kind);
            if is_valid {
                result.expect("expected history entry to be valid");
            } else {
                result.expect_err("expected history entry to be invalid");
            }
        }
    }

    fn make_history_entry(copy_from: Option<RepoPath>) -> HistoryEntry {
        HistoryEntry {
            node: NULL_HASH,
            p1: NULL_HASH,
            p2: NULL_HASH,
            linknode: NULL_HASH,
            copy_from,
        }
    }

    #[test]
    fn test_history_verify_arbitrary() {
        let mut rng = StdGen::new(rand::thread_rng(), 100);
        for _n in 0..100 {
            HistoryEntry::arbitrary_kind(&mut rng, Kind::Tree)
                .verify(Kind::Tree)
                .expect("generated history entry should be valid");
            HistoryEntry::arbitrary_kind(&mut rng, Kind::File)
                .verify(Kind::File)
                .expect("generated history entry should be valid");
        }
    }

    #[test]
    fn test_history_roundtrip() {
        let mut rng = StdGen::new(rand::thread_rng(), 100);
        for _n in 0..100 {
            history_roundtrip(&mut rng, Kind::Tree);
            history_roundtrip(&mut rng, Kind::File);
        }
    }

    fn history_roundtrip<G: Gen>(g: &mut G, kind: Kind) {
        let entry = HistoryEntry::arbitrary_kind(g, kind);
        let mut encoded = vec![];
        entry
            .encode(kind, &mut encoded)
            .expect("encoding this history entry should succeed");

        let mut encoded_bytes = BytesMut::from(encoded);

        // Ensure that a partial entry results in None.
        let bytes_len = encoded_bytes.len();
        let reduced_len = unsafe {
            let reduced_len = cmp::max(bytes_len - g.gen_range(1, bytes_len), 0);
            encoded_bytes.set_len(reduced_len);
            reduced_len
        };
        let decoded = HistoryEntry::decode(&mut encoded_bytes, kind)
            .expect("decoding this history entry should succeed");
        assert_eq!(decoded, None);
        // Ensure that no bytes in encoded actually got read
        assert_eq!(encoded_bytes.len(), reduced_len);

        // Restore the original length for the next test.
        unsafe {
            encoded_bytes.set_len(bytes_len);
        }

        let decoded = HistoryEntry::decode(&mut encoded_bytes, kind)
            .expect("decoding this history entry should succeed");
        assert_eq!(Some(entry), decoded);
        assert_eq!(encoded_bytes.len(), 0);
    }

    #[test]
    fn test_data_verify_basic() {
        #[cfg_attr(rustfmt, rustfmt_skip)]
        let tests = vec![
            (NULL_HASH, vec![Fragment { start: 0, end: 0, content: vec![b'a'] }], true),
            (NULL_HASH, vec![Fragment { start: 0, end: 5, content: vec![b'b'] }], false),
            (AS_HASH, vec![Fragment { start: 0, end: 0, content: vec![b'c'] }], true),
            (AS_HASH, vec![Fragment { start: 0, end: 5, content: vec![b'd'] }], true),
        ];

        for (delta_base, frags, is_valid) in tests.into_iter() {
            let delta = Delta::new(frags).expect("test deltas should all be valid");
            let entry = DataEntry {
                node: BS_HASH,
                delta_base,
                delta,
                metadata: None,
            };
            let result = entry.verify();
            if is_valid {
                result.expect("expected data entry to be valid");
            } else {
                result.expect_err("expected data entry to be invalid");
            }
        }
    }

    quickcheck! {
        fn test_data_verify_arbitrary(entry: DataEntry) -> bool {
            entry.verify().is_ok()
        }

        fn test_data_roundtrip(entry: DataEntry) -> bool {
            let version = if entry.metadata.is_none() {
                DataEntryVersion::V1
            } else {
                DataEntryVersion::V2
            };

            let mut rng = StdGen::new(rand::thread_rng(), 100);

            let mut encoded = vec![];
            entry.encode(&mut encoded).expect("encoding this data entry should succeed");

            let mut encoded_bytes = BytesMut::from(encoded);

            // Ensure that a partial entry results in None.
            let bytes_len = encoded_bytes.len();
            let reduced_len = unsafe {
                let reduced_len = cmp::max(bytes_len - rng.gen_range(1, bytes_len), 0);
                encoded_bytes.set_len(reduced_len);
                reduced_len
            };
            let decoded = DataEntry::decode(&mut encoded_bytes, version)
                .expect("decoding this data entry should succeed");
            assert_eq!(decoded, None);
            // Ensure that no bytes in encoded actually got read.
            assert_eq!(encoded_bytes.len(), reduced_len);

            // Restore the original length for the next test.
            unsafe {
                encoded_bytes.set_len(bytes_len);
            }

            let decoded = DataEntry::decode(&mut encoded_bytes, version)
                .expect("decoding this history entry should succeed");
            assert_eq!(Some(entry), decoded);
            assert_eq!(encoded_bytes.len(), 0);
            true
        }
    }
}
