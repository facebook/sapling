/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;
use std::collections::HashMap;
use std::io::Read;
use std::path::Path;

use anyhow::anyhow;
use anyhow::Result;
use byteorder::BigEndian;
use byteorder::ReadBytesExt;
use types::HgId;

use crate::filestate::FileStateV2;
use crate::filestate::StateFlags;
use crate::metadata::Metadata;

const DIRSTATE_HEADER: u8 = 0x01;
const COPYMAP_HEADER: u8 = 0x02;
const CHECKSUM_HEADER: u8 = 0xFF;

const MERGE_BOTH_PARENTS: i8 = -1;
const MERGE_OTHER_PARENT: i8 = -2;

pub fn read_dirstate(_dirstate_path: &Path) -> Result<(Metadata, HashMap<Box<[u8]>, FileStateV2>)> {
    todo!();
}

pub fn write_dirstate(
    _dirstate_path: &Path,
    _metadata: Metadata,
    _entries: HashMap<Box<[u8]>, FileStateV2>,
) -> Result<()> {
    todo!();
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
fn deserialize_dirstate(
    dirstate: &mut &[u8],
) -> Result<(Metadata, HashMap<Box<[u8]>, FileStateV2>)> {
    let mut p1 = [0; 20];
    dirstate.read_exact(&mut p1)?;
    let p1 = HgId::from(&p1);

    let mut p2_bytes = [0; 20];
    dirstate.read_exact(&mut p2_bytes)?;
    let p2 = HgId::from(&p2_bytes);

    let mut metadata = BTreeMap::new();
    metadata.insert("p1".to_string(), p1.to_string());
    if !p2.is_null() {
        metadata.insert("p2".to_string(), p2.to_string());
    }
    let metadata = Metadata(metadata);

    let _version = dirstate.read_u32::<BigEndian>()?;

    let mut entries: HashMap<Box<[u8]>, FileStateV2> = HashMap::new();
    while let Some((path, mut state)) = deserialize_entry(dirstate)? {
        if let Some(existing_state) = entries.get_mut(&path) {
            state.state |= existing_state.state;
            if existing_state.copied.is_some() {
                state.copied = Some(existing_state.copied.take().unwrap());
            }
        }
        entries.insert(path, state);
    }

    // TODO: Verify checksum

    Ok((metadata, entries))
}

fn deserialize_entry(dirstate: &mut &[u8]) -> Result<Option<(Box<[u8]>, FileStateV2)>> {
    let header = dirstate.read_u8()?;
    if header == CHECKSUM_HEADER {
        // Reached checksum, no more entries in dirstate
        return Ok(None);
    }

    if header == COPYMAP_HEADER {
        let dest_size = dirstate.read_u16::<BigEndian>()?;
        let dest_path = read_path(dirstate, dest_size)?;

        let source_size = dirstate.read_u16::<BigEndian>()?;
        let source_path = read_path(dirstate, source_size)?;

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

    let path = read_path(dirstate, size)?;
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

fn read_path(dirstate: &mut &[u8], size: u16) -> Result<Box<[u8]>> {
    let mut path = Vec::with_capacity(size.into());
    let mut reader = Read::take(dirstate, size.into());
    reader.read_to_end(&mut path)?;
    Ok(path.into_boxed_slice())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use pretty_assertions::assert_eq;

    use super::deserialize_dirstate;
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
}
