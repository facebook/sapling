// Copyright 2018 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use lz4_pyframe::{compress, decompress};
use minibench::{bench, elapsed};
use rand_core::{RngCore, SeedableRng};

fn main() {
    let mut rng = rand_chacha::ChaChaRng::seed_from_u64(0);
    let mut buf = vec![0u8; 100_000000];
    rng.fill_bytes(&mut buf);
    let compressed = compress(&buf).unwrap();

    bench("compress (100M)", || {
        elapsed(|| {
            compress(&buf).unwrap();
        })
    });

    bench("decompress (~100M)", || {
        elapsed(|| {
            decompress(&compressed).unwrap();
        })
    });
}
