/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Serialize a fuzzer-derived value, decode it again and compare.
//!
//! Unlike `decode`, the bytes handed to the deserializer here are ones mincode
//! wrote itself, so failing to decode them, decoding them to a different
//! value, or re-encoding to different bytes are all defects rather than the
//! expected handling of corrupt input. This is the target that catches
//! asymmetry between `ser.rs` and `de.rs` - a length written in bytes and read
//! as a count, or a read that forgets to advance the cursor - which the
//! decode-only target cannot see.
//!
//! No floats appear in the round-tripped shapes: a generated NaN would fail
//! the comparison without any encoding being wrong.

#![no_main]

use std::collections::BTreeMap;
use std::collections::HashMap;
use std::fmt::Debug;

use libfuzzer_sys::fuzz_target;
use serde::Serialize;
use serde::de::DeserializeOwned;

mod shapes;

use crate::shapes::ContentHash;
use crate::shapes::ContentHashType;
use crate::shapes::LfsPointer;
use crate::shapes::RepoPathBuf;
use crate::shapes::Root;
use crate::shapes::SerId20;
use crate::shapes::TupleKey;

/// Entries in a generated map.
const MAX_ENTRIES: usize = 8;

/// Characters in a generated string.
const MAX_CHARS: usize = 32;

/// Deterministic value source.
struct Gen<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Gen<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, pos: 0 }
    }

    /// Reads wrap around the input, so even a one byte input yields a complete
    /// value. Every length drawn below is capped, so wrapping cannot make
    /// generation diverge.
    fn byte(&mut self) -> u8 {
        if self.bytes.is_empty() {
            return 0;
        }
        let byte = self.bytes[self.pos % self.bytes.len()];
        self.pos = self.pos.wrapping_add(1);
        byte
    }

    fn len(&mut self, max: usize) -> usize {
        self.byte() as usize % (max + 1)
    }

    fn u64(&mut self) -> u64 {
        (0..8).fold(0u64, |acc, _| (acc << 8) | u64::from(self.byte()))
    }

    fn array<const N: usize>(&mut self) -> [u8; N] {
        std::array::from_fn(|_| self.byte())
    }

    /// Bytes are read as Latin-1 code points, so anything above 0x7f encodes
    /// to more than one UTF-8 byte and the encoded length stops matching the
    /// character count.
    fn string(&mut self, max: usize) -> String {
        let len = self.len(max);
        (0..len).map(|_| char::from(self.byte())).collect()
    }
}

fn gen_root(source: &mut Gen<'_>) -> Root {
    let entries = source.len(MAX_ENTRIES);
    Root {
        map: (0..entries)
            .map(|_| (source.string(MAX_CHARS), SerId20(source.array())))
            .collect(),
        timestamp: source.u64(),
        message: source.string(MAX_CHARS),
    }
}

fn gen_pointer(source: &mut Gen<'_>) -> LfsPointer {
    let copy_from = source.byte().is_multiple_of(2).then(|| TupleKey {
        path: RepoPathBuf(source.string(MAX_CHARS)),
        hgid: source.array(),
    });
    // The one variant of `ContentHashType` bounds this map at a single entry,
    // which is what keeps `HashMap` iteration order out of the comparison.
    let content_hashes =
        HashMap::from([(ContentHashType::Sha256, ContentHash::Sha256(source.array()))]);
    LfsPointer {
        hgid: source.array(),
        size: source.u64(),
        is_binary: source.byte().is_multiple_of(2),
        copy_from,
        content_hashes,
    }
}

fn check<T>(value: &T)
where
    T: Serialize + DeserializeOwned + PartialEq + Debug,
{
    let bytes = mincode::serialize(value).expect("serializing a generated value should succeed");
    let decoded: T =
        mincode::deserialize(&bytes).expect("mincode should decode the bytes it just wrote");
    assert_eq!(
        *value, decoded,
        "value did not survive a mincode round trip"
    );
    let reencoded =
        mincode::serialize(&decoded).expect("re-serializing a decoded value should succeed");
    assert_eq!(
        bytes, reencoded,
        "re-encoding a decoded value moved the bytes"
    );
}

/// Round trip a shape whose encoding is nothing but a length prefixed map, to
/// separate a length handling defect from a field ordering one.
fn check_map(source: &mut Gen<'_>) {
    let entries = source.len(MAX_ENTRIES);
    let map: BTreeMap<String, Vec<u8>> = (0..entries)
        .map(|_| {
            let key = source.string(MAX_CHARS);
            let len = source.len(MAX_CHARS);
            (key, (0..len).map(|_| source.byte()).collect())
        })
        .collect();
    check(&map);
}

fuzz_target!(|input: &[u8]| {
    let mut source = Gen::new(input);
    check(&gen_root(&mut source));
    check(&gen_pointer(&mut source));
    check_map(&mut source);
});
