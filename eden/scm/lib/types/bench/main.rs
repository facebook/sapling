/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use criterion::Criterion;
use sha1::Digest;
use sha1::Sha1;
use types::HgId;

fn hgid_from_hex(hash: &[u8]) -> [u8; HgId::len()] {
    HgId::from_hex(hash).unwrap().into_byte_array()
}

fn from_hex_faster_hex(hash: &[u8]) -> [u8; HgId::len()] {
    let mut bytes = [0u8; HgId::len()];
    faster_hex::hex_decode(hash, &mut bytes).unwrap();
    bytes
}

fn from_hex_bitwise(hash: &[u8]) -> [u8; HgId::len()] {
    let mut bytes = [0u8; HgId::len()];
    let hexify = |nibble| 9u8 * (nibble >> 6) + (nibble & 0o17);

    for (i, chunk) in hash.chunks_exact(2).enumerate() {
        let high = hexify(chunk[0]);
        let low = hexify(chunk[1]);
        bytes[i] = (high << 4) | low;
    }
    bytes
}

static HEX_LOOKUP: [u8; 256] = [
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 0, 0, 0, 0, 0, 0,
    0, 10, 11, 12, 13, 14, 15, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 10, 11, 12, 13, 14, 15, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0,
];

fn from_hex_lookup_table(hash: &[u8]) -> [u8; HgId::len()] {
    let mut bytes = [0u8; HgId::len()];

    for (i, chunk) in hash.chunks_exact(2).enumerate() {
        let high = HEX_LOOKUP[chunk[0] as usize];
        let low = HEX_LOOKUP[chunk[1] as usize];
        bytes[i] = (high << 4) | low;
    }
    bytes
}

fn make_hashes() -> Vec<String> {
    let mut hashes = vec![];
    for i in 0..1000 {
        let mut hasher = Sha1::new();
        hasher.update(format!("{}", i));
        hashes.push(hex::encode(hasher.finalize()));
    }
    hashes
}

fn main() {
    let mut criterion = Criterion::default();

    criterion.bench_function("HgId::from_hex", |b| {
        let hashes = make_hashes();
        b.iter(|| {
            for hash in hashes.iter() {
                criterion::black_box(hgid_from_hex(hash.as_bytes()));
            }
        })
    });

    criterion.bench_function("faster-hex::from_hex", |b| {
        let hashes = make_hashes();
        b.iter(|| {
            for hash in hashes.iter() {
                criterion::black_box(from_hex_faster_hex(hash.as_bytes()));
            }
        })
    });

    criterion.bench_function("lookup table::from_hex", |b| {
        let hashes = make_hashes();
        b.iter(|| {
            for hash in hashes.iter() {
                criterion::black_box(from_hex_lookup_table(hash.as_bytes()));
            }
        })
    });

    criterion.bench_function("bitwise::from_hex", |b| {
        let hashes = make_hashes();
        b.iter(|| {
            for hash in hashes.iter() {
                criterion::black_box(from_hex_bitwise(hash.as_bytes()));
            }
        })
    });
}
