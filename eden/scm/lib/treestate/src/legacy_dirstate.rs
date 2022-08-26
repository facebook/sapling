/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;
use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::io::Write;
use std::path::Path;
use std::str::FromStr;

use anyhow::anyhow;
use anyhow::Result;
use byteorder::BigEndian;
use byteorder::ReadBytesExt;
use byteorder::WriteBytesExt;
use sha2::Digest;
use sha2::Sha256;
use types::hgid::WriteHgIdExt;
use types::HgId;
use util::file::atomic_write;

use crate::filestate::FileStateV2;
use crate::filestate::StateFlags;
use crate::metadata::Metadata;

/// Reader and writer functions to a legacy Eden dirstate. Note carefully that
/// a legacy Eden dirstate has a different binary format to a legacy dirstate
/// in a non-Eden repo.

const CURRENT_VERSION: u32 = 1;

const DIRSTATE_HEADER: u8 = 0x01;
const COPYMAP_HEADER: u8 = 0x02;
const CHECKSUM_HEADER: u8 = 0xFF;

const MERGE_NOT_APPLICABLE: i8 = 0;
const MERGE_BOTH_PARENTS: i8 = -1;
const MERGE_OTHER_PARENT: i8 = -2;

const DIRSTATE_PATH: &str = ".hg/dirstate";

pub fn read_dirstate(repo_root: &Path) -> Result<(Metadata, HashMap<Box<[u8]>, FileStateV2>)> {
    let mut dirstate = File::open(repo_root.join(DIRSTATE_PATH))?;
    deserialize_dirstate(&mut dirstate)
}

pub fn write_dirstate(
    repo_root: &Path,
    metadata: Metadata,
    entries: HashMap<Box<[u8]>, FileStateV2>,
) -> Result<()> {
    let mut dirstate_bytes = Vec::new();
    serialize_dirstate(&mut dirstate_bytes, &metadata, &entries)?;
    atomic_write(&repo_root.join(DIRSTATE_PATH), |f| {
        f.write_all(&dirstate_bytes)
    })
    .map(|_| ())
    .map_err(|e| anyhow!(e))
}

struct Reader<T> {
    hasher: Sha256,
    inner: T,
}

impl<T: Read> Reader<T> {
    fn new(inner: T) -> Self {
        let hasher = Sha256::new();
        Self { hasher, inner }
    }

    fn verify_checksum(mut self) -> Result<bool> {
        let actual: [u8; 32] = self.hasher.finalize().into();
        let mut expected = [0; 32];
        self.inner.read_exact(&mut expected)?;
        Ok(actual == expected)
    }
}

impl<T: Read> Read for Reader<T> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let result = self.inner.read(buf)?;
        self.hasher.update(buf);
        Ok(result)
    }
}

struct Writer<T> {
    hasher: Sha256,
    inner: T,
}

impl<T: Write> Writer<T> {
    fn new(inner: T) -> Self {
        let hasher = Sha256::new();
        Self { hasher, inner }
    }

    fn write_checksum(mut self) -> std::io::Result<()> {
        let checksum = self.hasher.finalize();
        self.inner.write_all(&checksum)
    }
}

impl<T: Write> Write for Writer<T> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.hasher.update(buf);
        self.inner.write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

// The serialization format of the dirstate is as follows:
// - The first 40 bytes are the hashes of the two parent pointers.
// - The next 4 bytes are the version number of the format.
// - The next section is the dirstate tuples. Each dirstate tuple is
//   represented as follows:
//   - The first byte is '\x01'.
//   - The second byte represents the status. It is the ASCII value of
//     'n', 'm', 'r', 'a', '?', as appropriate.
//   - The next four bytes are an unsigned integer representing mode_t.
//   - The seventh byte (which is signed) represents the merge state:
//     - 0 is NotApplicable
//     - -1 is BothParents
//     - -2 is OtherParent
//   - The next two bytes are an unsigned short representing the length of
//     the path, in bytes.
//   - The bytes of the path itself. Note that a path cannot contain \0.
// - The next section is the copymap. Each entry in the copymap is
//   represented as follows.
//   - The first byte is '\x02'.
//   - An unsigned short (two bytes) representing the length, followed by
//     that number of bytes, which constitutes the relative path name of the
//     *destination* of the copy.
//   - An unsigned short (two bytes) representing the length, followed by
//     that number of bytes, which constitutes the relative path name of the
//     *source* of the copy.
// - The last section is the checksum. Although the other tuples can be
//   interleaved or reordered without issue, the checksum must come last.
//   The checksum is a function of all of the bytes written up to this point
//   plus the \xFF header for the checksum section.
//   - The first byte is '\xFF' to distinguish it from the other fields.
//   - Because we use SHA-256 as the hash algorithm for the checksum, the
//     remaining 32 bytes are used for the hash.
fn serialize_dirstate(
    dirstate: impl Write,
    metadata: &Metadata,
    treestate: &HashMap<Box<[u8]>, FileStateV2>,
) -> Result<()> {
    let mut writer = Writer::new(dirstate);

    let p1 = metadata
        .0
        .get("p1")
        .ok_or_else(|| anyhow!("Treestate is missing necessary P1 node"))?;
    let p1 = HgId::from_str(p1)?;
    writer.write_hgid(&p1)?;

    if let Some(p2) = metadata.0.get("p2") {
        let p2 = HgId::from_str(p2)?;
        writer.write_hgid(&p2)?;
    } else {
        writer.write_hgid(HgId::null_id())?;
    }

    writer.write_u32::<BigEndian>(CURRENT_VERSION)?;

    for (path, state) in treestate {
        serialize_entry(&mut writer, path, state)?;
    }

    writer.write_u8(CHECKSUM_HEADER)?;
    writer.write_checksum()?;

    Ok(())
}

fn serialize_entry(mut dirstate: impl Write, path: &[u8], state: &FileStateV2) -> Result<()> {
    if state.state.contains(StateFlags::COPIED) {
        dirstate.write_u8(COPYMAP_HEADER)?;
        dirstate.write_u16::<BigEndian>(path.len().try_into()?)?;
        dirstate.write_all(path)?;

        let source_path = state.copied.as_ref().ok_or_else(|| {
            anyhow!(
                "Missing source path on copied entry ({})",
                String::from_utf8_lossy(path)
            )
        })?;
        dirstate.write_u16::<BigEndian>(source_path.len().try_into()?)?;
        dirstate.write_all(source_path)?;
    }

    dirstate.write_u8(DIRSTATE_HEADER)?;

    let (state_char, merge_state) = {
        let s =
            state.state & (StateFlags::EXIST_NEXT | StateFlags::EXIST_P1 | StateFlags::EXIST_P2);

        if s == StateFlags::EXIST_NEXT | StateFlags::EXIST_P1 | StateFlags::EXIST_P2 {
            (b'm', MERGE_BOTH_PARENTS)
        } else if s == StateFlags::EXIST_NEXT | StateFlags::EXIST_P1 {
            (b'n', MERGE_NOT_APPLICABLE)
        } else if s == StateFlags::EXIST_NEXT | StateFlags::EXIST_P2 {
            (b'n', MERGE_OTHER_PARENT)
        } else if s == StateFlags::EXIST_NEXT {
            (b'a', MERGE_BOTH_PARENTS)
        } else if s == StateFlags::EXIST_P1 | StateFlags::EXIST_P2 {
            (b'r', MERGE_BOTH_PARENTS)
        } else if s == StateFlags::EXIST_P2 {
            (b'r', MERGE_OTHER_PARENT)
        } else if s == StateFlags::EXIST_P1 {
            (b'r', MERGE_NOT_APPLICABLE)
        } else if s.is_empty() {
            (b'?', MERGE_NOT_APPLICABLE)
        } else {
            return Err(anyhow!(
                "Unhandled treestate -> dirstate conversion ({:?})",
                s
            ));
        }
    };

    dirstate.write_u8(state_char)?;
    dirstate.write_u32::<BigEndian>(0)?;
    dirstate.write_i8(merge_state)?;
    dirstate.write_u16::<BigEndian>(path.len().try_into()?)?;
    dirstate.write_all(path)?;
    Ok(())
}

fn deserialize_dirstate(
    dirstate: impl Read,
) -> Result<(Metadata, HashMap<Box<[u8]>, FileStateV2>)> {
    let mut reader = Reader::new(dirstate);
    let mut p1 = [0; 20];
    reader.read_exact(&mut p1)?;
    let p1 = HgId::from(&p1);

    let mut p2_bytes = [0; 20];
    reader.read_exact(&mut p2_bytes)?;
    let p2 = HgId::from(&p2_bytes);

    let mut metadata = BTreeMap::new();
    metadata.insert("p1".to_string(), p1.to_string());
    if !p2.is_null() {
        metadata.insert("p2".to_string(), p2.to_string());
    }
    let metadata = Metadata(metadata);

    let _version = reader.read_u32::<BigEndian>()?;

    let mut entries: HashMap<Box<[u8]>, FileStateV2> = HashMap::new();
    while let Some((path, mut state)) = deserialize_entry(&mut reader)? {
        if let Some(existing_state) = entries.get_mut(&path) {
            state.state |= existing_state.state;
            if existing_state.copied.is_some() {
                state.copied = Some(existing_state.copied.take().unwrap());
            }
        }
        entries.insert(path, state);
    }

    if !reader.verify_checksum()? {
        return Err(anyhow!(
            "Dirstate contained incorrect checksum: likely a corrupt dirstate"
        ));
    }

    Ok((metadata, entries))
}

fn deserialize_entry(mut dirstate: impl Read) -> Result<Option<(Box<[u8]>, FileStateV2)>> {
    let header = dirstate.read_u8()?;
    if header == CHECKSUM_HEADER {
        // Reached checksum, no more entries in dirstate
        return Ok(None);
    }

    if header == COPYMAP_HEADER {
        let dest_size = dirstate.read_u16::<BigEndian>()?;
        let dest_path = read_path(&mut dirstate, dest_size)?;

        let source_size = dirstate.read_u16::<BigEndian>()?;
        let source_path = read_path(&mut dirstate, source_size)?;

        return Ok(Some((
            dest_path,
            FileStateV2 {
                mode: 0,
                size: 0,
                mtime: 0,
                state: StateFlags::COPIED,
                copied: Some(source_path),
            },
        )));
    }

    if header != DIRSTATE_HEADER {
        return Err(anyhow!("Unexpected header value: {}", header));
    }

    let state_char = dirstate.read_u8()?;
    let mode = dirstate.read_u32::<BigEndian>()?;
    let merge_state = dirstate.read_i8()?;
    let size = dirstate.read_u16::<BigEndian>()?;

    let state = match state_char {
        b'n' if merge_state == MERGE_OTHER_PARENT => StateFlags::EXIST_NEXT | StateFlags::EXIST_P2,
        b'n' => StateFlags::EXIST_NEXT | StateFlags::EXIST_P1,
        b'm' => StateFlags::EXIST_NEXT | StateFlags::EXIST_P1 | StateFlags::EXIST_P2,
        b'r' if merge_state == MERGE_BOTH_PARENTS => StateFlags::EXIST_P1 | StateFlags::EXIST_P2,
        b'r' if merge_state == MERGE_OTHER_PARENT => StateFlags::EXIST_P2,
        b'r' => StateFlags::EXIST_P1,
        b'a' => StateFlags::EXIST_NEXT,
        b'?' => StateFlags::empty(),
        s => return Err(anyhow!("Unexpected state: {}", s)),
    };

    let path = read_path(&mut dirstate, size)?;
    Ok(Some((
        path,
        FileStateV2 {
            mode,
            size: 0,
            mtime: 0,
            state,
            copied: None,
        },
    )))
}

fn read_path(dirstate: impl Read, size: u16) -> Result<Box<[u8]>> {
    let mut path = Vec::with_capacity(size.into());
    let mut reader = Read::take(dirstate, size.into());
    reader.read_to_end(&mut path)?;
    Ok(path.into_boxed_slice())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use pretty_assertions::assert_eq;
    use sha2::Digest;
    use sha2::Sha256;

    use super::deserialize_dirstate;
    use super::serialize_dirstate;
    use crate::filestate::FileStateV2;
    use crate::filestate::StateFlags;
    use crate::metadata::Metadata;
    use crate::serialization::Serializable;

    // DIRSTATE corresponds to the following working copy state:
    // $ hg st -C
    // M modified
    // A added
    // A copy_dest
    //   copy_source
    // A move_after
    //   move_before
    // R move_before
    // R removed
    // ! missing
    // ? untracked
    static DIRSTATE: [u8; 215] = [
        0xc9, 0x4c, 0x85, 0xea, 0x63, 0xba, 0x78, 0x41, 0x64, 0xf8, 0x5d, 0x3c, 0x8c, 0x89, 0xb2,
        0x57, 0x06, 0xec, 0x7a, 0x7f, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x01,
        0x72, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x07, 0x72, 0x65, 0x6d, 0x6f, 0x76, 0x65, 0x64,
        0x01, 0x61, 0x00, 0x00, 0x00, 0x00, 0xff, 0x00, 0x05, 0x61, 0x64, 0x64, 0x65, 0x64, 0x01,
        0x61, 0x00, 0x00, 0x00, 0x00, 0xff, 0x00, 0x0a, 0x6d, 0x6f, 0x76, 0x65, 0x5f, 0x61, 0x66,
        0x74, 0x65, 0x72, 0x01, 0x72, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x0b, 0x6d, 0x6f, 0x76,
        0x65, 0x5f, 0x62, 0x65, 0x66, 0x6f, 0x72, 0x65, 0x01, 0x61, 0x00, 0x00, 0x00, 0x00, 0xff,
        0x00, 0x09, 0x63, 0x6f, 0x70, 0x79, 0x5f, 0x64, 0x65, 0x73, 0x74, 0x02, 0x00, 0x0a, 0x6d,
        0x6f, 0x76, 0x65, 0x5f, 0x61, 0x66, 0x74, 0x65, 0x72, 0x00, 0x0b, 0x6d, 0x6f, 0x76, 0x65,
        0x5f, 0x62, 0x65, 0x66, 0x6f, 0x72, 0x65, 0x02, 0x00, 0x09, 0x63, 0x6f, 0x70, 0x79, 0x5f,
        0x64, 0x65, 0x73, 0x74, 0x00, 0x0b, 0x63, 0x6f, 0x70, 0x79, 0x5f, 0x73, 0x6f, 0x75, 0x72,
        0x63, 0x65, 0xff, 0x39, 0x39, 0x1a, 0x7d, 0x3b, 0x0b, 0xa4, 0xe7, 0xe2, 0xc3, 0xfd, 0x1b,
        0xae, 0xd3, 0x75, 0x11, 0x00, 0x14, 0x06, 0x84, 0x1b, 0xe9, 0x68, 0xd9, 0x1e, 0x65, 0xf3,
        0xf6, 0x43, 0x2a, 0xbe, 0xbf,
    ];

    fn treestate() -> (Metadata, HashMap<Box<[u8]>, FileStateV2>) {
        let mut metadata_bytes = b"p1=c94c85ea63ba784164f85d3c8c89b25706ec7a7f".as_slice();
        let metadata = Metadata::deserialize(&mut metadata_bytes).unwrap();
        let entries = vec![
            (
                "added",
                FileStateV2 {
                    mode: 0,
                    size: 0,
                    mtime: 0,
                    state: StateFlags::EXIST_NEXT,
                    copied: None,
                },
            ),
            (
                "copy_dest",
                FileStateV2 {
                    mode: 0,
                    size: 0,
                    mtime: 0,
                    state: StateFlags::EXIST_NEXT | StateFlags::COPIED,
                    copied: Some(b"copy_source".to_vec().into_boxed_slice()),
                },
            ),
            (
                "move_after",
                FileStateV2 {
                    mode: 0,
                    size: 0,
                    mtime: 0,
                    state: StateFlags::EXIST_NEXT | StateFlags::COPIED,
                    copied: Some(b"move_before".to_vec().into_boxed_slice()),
                },
            ),
            (
                "move_before",
                FileStateV2 {
                    mode: 0,
                    size: 0,
                    mtime: 0,
                    state: StateFlags::EXIST_P1,
                    copied: None,
                },
            ),
            (
                "removed",
                FileStateV2 {
                    mode: 0,
                    size: 0,
                    mtime: 0,
                    state: StateFlags::EXIST_P1,
                    copied: None,
                },
            ),
        ]
        .iter()
        .map(|(key, value)| (key.as_bytes().to_vec().into_boxed_slice(), value.clone()))
        .collect::<HashMap<_, _>>();

        (metadata, entries)
    }

    #[test]
    fn deserialize_test() {
        let (metadata, entries) = deserialize_dirstate(&mut DIRSTATE.as_slice()).unwrap();
        let (expected_metadata, expected_entries) = treestate();
        assert_eq!(metadata, expected_metadata);

        let mut entries = entries
            .into_iter()
            .map(|(path, state)| (String::from_utf8_lossy(&path).into_owned(), state))
            .collect::<Vec<_>>();
        entries.sort_unstable_by_key(|(path, _)| path.clone());

        let mut expected_entries = expected_entries
            .into_iter()
            .map(|(path, state)| (String::from_utf8_lossy(&path).into_owned(), state))
            .collect::<Vec<_>>();
        expected_entries.sort_unstable_by_key(|(path, _)| path.clone());

        assert_eq!(entries, expected_entries);
    }

    #[test]
    fn serialize_test() {
        let (in_metadata, in_entries) = treestate();
        let mut dirstate = Vec::new();
        serialize_dirstate(&mut dirstate, &in_metadata, &in_entries).unwrap();

        let (out_metadata, out_entries) = deserialize_dirstate(&mut dirstate.as_slice()).unwrap();

        assert_eq!(in_metadata, out_metadata);

        let mut in_entries = in_entries
            .into_iter()
            .map(|(path, state)| (String::from_utf8_lossy(&path).into_owned(), state))
            .collect::<Vec<_>>();
        in_entries.sort_unstable_by_key(|(path, _)| path.clone());

        let mut out_entries = out_entries
            .into_iter()
            .map(|(path, state)| (String::from_utf8_lossy(&path).into_owned(), state))
            .collect::<Vec<_>>();
        out_entries.sort_unstable_by_key(|(path, _)| path.clone());

        assert_eq!(in_entries, out_entries);
    }

    #[test]
    fn write_checksum_test() {
        let (metadata, entries) = treestate();
        let mut dirstate = Vec::new();
        serialize_dirstate(&mut dirstate, &metadata, &entries).unwrap();
        assert_eq!(dirstate.len(), 215);

        let mut hasher = Sha256::new();
        hasher.update(&dirstate[0..183]);
        let expected_checksum: [u8; 32] = hasher.finalize().into();

        assert_eq!(expected_checksum, dirstate[183..215]);
    }
}
