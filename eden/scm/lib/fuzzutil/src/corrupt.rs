/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Turn fuzzer bytes into a bounded set of edits to a valid on-disk structure.
//!
//! Starting from a valid structure is what gets depth: pure random bytes are
//! rejected by the `indexedlog0\0` primary header or the `meta` header and
//! never reach the index radix walk, the checksum verification or the entry
//! decoders that the crash reports point at.

use std::fs;
use std::path::Path;
use std::path::PathBuf;

use tempfile::TempDir;

/// Patches applied per input, bounding per-iteration work for large inputs.
pub const MAX_PATCHES: usize = 256;

/// Bytes a single fill patch may write.
pub const MAX_FILL_LEN: usize = 4096;

/// Bytes one patch record occupies in the fuzzer input.
pub const PATCH_LEN: usize = 8;

/// Most bytes one splice record copies from the input into a buffer.
pub const MAX_SPLICE_LEN: usize = 16;

/// A pristine on-disk structure held in memory, so an iteration can write a
/// fresh copy without paying to rebuild it.
pub struct Seed {
    files: Vec<(PathBuf, Vec<u8>)>,
}

impl Seed {
    /// Snapshot every file under `dir`, keeping paths relative to it.
    pub fn snapshot(dir: &Path) -> Self {
        let mut files = Vec::new();
        collect_files(dir, Path::new(""), &mut files);
        files.sort_by(|a, b| a.0.cmp(&b.0));
        assert!(!files.is_empty(), "the seed structure should have files");
        Self { files }
    }

    /// Write a copy of the seed, corrupted according to `input`, into a fresh
    /// temporary directory. The directory is removed when the returned handle
    /// is dropped.
    pub fn materialize(&self, input: &[u8]) -> TempDir {
        let mut contents: Vec<Vec<u8>> = self.files.iter().map(|(_, data)| data.clone()).collect();
        apply_patches(&mut contents, input, 0);

        let dir = TempDir::new().expect("creating a temp dir should succeed");
        for ((relative, _), data) in self.files.iter().zip(&contents) {
            let path = dir.path().join(relative);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).expect("creating a temp subdir should succeed");
            }
            fs::write(&path, data).expect("writing a temp file should succeed");
        }
        dir
    }
}

fn collect_files(root: &Path, relative: &Path, out: &mut Vec<(PathBuf, Vec<u8>)>) {
    let entries = fs::read_dir(root.join(relative)).expect("reading the seed dir should succeed");
    for entry in entries {
        let entry = entry.expect("reading a seed dir entry should succeed");
        let relative = relative.join(entry.file_name());
        let file_type = entry
            .file_type()
            .expect("reading a seed entry type should succeed");
        if file_type.is_dir() {
            collect_files(root, &relative, out);
        } else if file_type.is_file() {
            let data = fs::read(root.join(&relative)).expect("reading a seed file should succeed");
            out.push((relative, data));
        }
    }
}

/// Apply the fuzzer input as a sequence of 8-byte patch records:
///
/// ```text
/// byte 0      buffer selector
/// byte 1      opcode
/// bytes 2..6  u32 little endian offset
/// bytes 6..8  u16 little endian argument
/// ```
///
/// Opcodes, taken modulo 5: set one byte to the argument, xor one byte with
/// the argument, fill a run with the argument's high byte, truncate at the
/// offset, and splice. A splice record is followed by `1 + argument % 16`
/// raw input bytes that are copied verbatim over the buffer at the offset,
/// clipped to the buffer. That is the opcode a dictionary token needs:
/// without it a multi-byte token in the input is only ever read as patch
/// records, never written contiguously into the data under test.
///
/// Selectors and offsets are reduced modulo the available range, so every
/// record mutates something and there are no magic values the fuzzer has to
/// guess before it makes any progress. A trailing partial record is ignored.
///
/// `min_len` floors the truncate opcode. A harness that patches a serialized
/// entry before appending it needs the leading key to survive so the store's
/// index function still has bytes to reference; shorter data trips an
/// assertion inside `indexedlog`, which is not a decoder finding.
pub fn apply_patches(buffers: &mut [Vec<u8>], input: &[u8], min_len: usize) {
    let mut rest = input;
    for _ in 0..MAX_PATCHES {
        let Some(record) = rest.get(..PATCH_LEN) else {
            break;
        };
        rest = &rest[PATCH_LEN..];
        let buffer = &mut buffers[record[0] as usize % buffers.len()];
        let offset = u32::from_le_bytes([record[2], record[3], record[4], record[5]]) as usize;
        let offset = offset % (buffer.len() + 1);
        let arg = u16::from_le_bytes([record[6], record[7]]);
        match record[1] % 5 {
            0 if offset < buffer.len() => buffer[offset] = arg as u8,
            1 if offset < buffer.len() => buffer[offset] ^= arg as u8,
            2 => {
                let len = (arg as usize % MAX_FILL_LEN).min(buffer.len() - offset);
                buffer[offset..offset + len].fill((arg >> 8) as u8);
            }
            3 => buffer.truncate(offset.max(min_len)),
            4 => {
                let want = 1 + arg as usize % MAX_SPLICE_LEN;
                let bytes = &rest[..want.min(rest.len())];
                rest = &rest[bytes.len()..];
                let len = bytes.len().min(buffer.len() - offset);
                buffer[offset..offset + len].copy_from_slice(&bytes[..len]);
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(selector: u8, opcode: u8, offset: u32, arg: u16) -> Vec<u8> {
        let mut out = vec![selector, opcode];
        out.extend_from_slice(&offset.to_le_bytes());
        out.extend_from_slice(&arg.to_le_bytes());
        out
    }

    #[test]
    fn set_xor_fill_and_truncate_stay_in_bounds() {
        let mut buffers = vec![vec![0u8; 16], vec![1u8; 4]];
        let mut input = Vec::new();
        input.extend(record(0, 0, 3, 0xAB));
        input.extend(record(1, 1, 0, 0x0F));
        // Fill with 0xCD; the length, 0xCD00 % MAX_FILL_LEN, runs past the
        // end and is clipped to the buffer.
        input.extend(record(0, 2, 12, 0xCD00));
        // Truncate below min_len keeps min_len bytes.
        input.extend(record(1, 3, 0, 0));
        apply_patches(&mut buffers, &input, 2);
        assert_eq!(buffers[0][3], 0xAB);
        assert_eq!(&buffers[0][12..], &[0xCD, 0xCD, 0xCD, 0xCD]);
        assert_eq!(buffers[1], vec![0x0E, 1]);
    }

    #[test]
    fn splice_copies_following_input_bytes_and_clips() {
        let mut buffers = vec![vec![0u8; 8]];
        let mut input = record(0, 4, 5, 3); // 1 + 3 % 16 = 4 bytes follow
        input.extend_from_slice(&[0xAA, 0xBB, 0xCC, 0xDD]);
        // The next record starts after the spliced bytes.
        input.extend(record(0, 0, 0, 0x11));
        apply_patches(&mut buffers, &input, 0);
        // Only three bytes fit before the end of the buffer.
        assert_eq!(buffers[0], vec![0x11, 0, 0, 0, 0, 0xAA, 0xBB, 0xCC]);
    }

    #[test]
    fn splice_at_end_of_input_takes_what_is_left() {
        let mut buffers = vec![vec![0u8; 4]];
        let mut input = record(0, 4, 0, 15); // asks for 16 bytes
        input.extend_from_slice(&[1, 2]);
        apply_patches(&mut buffers, &input, 0);
        assert_eq!(buffers[0], vec![1, 2, 0, 0]);
    }

    #[test]
    fn empty_and_partial_records_are_ignored() {
        let mut buffers = vec![vec![7u8; 3]];
        apply_patches(&mut buffers, &[], 0);
        apply_patches(&mut buffers, &[0, 0, 0], 0);
        assert_eq!(buffers[0], vec![7, 7, 7]);
    }

    #[test]
    fn materialize_writes_a_patched_copy() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join("sub")).unwrap();
        fs::write(dir.path().join("a"), b"hello").unwrap();
        fs::write(dir.path().join("sub/b"), b"world").unwrap();
        let seed = Seed::snapshot(dir.path());

        let copy = seed.materialize(&record(0, 0, 0, b'j' as u16));
        assert_eq!(fs::read(copy.path().join("a")).unwrap(), b"jello");
        assert_eq!(fs::read(copy.path().join("sub/b")).unwrap(), b"world");
        assert_eq!(fs::read(dir.path().join("a")).unwrap(), b"hello");
    }
}
