/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::hash::Hasher;

pub struct XxHash3_128(twox_hash::xxhash3_128::Hasher);

impl XxHash3_128 {
    pub fn with_seed(seed: u64) -> Self {
        XxHash3_128(twox_hash::xxhash3_128::Hasher::with_seed(seed))
    }

    pub fn finish_128(&self) -> u128 {
        self.0.finish_128()
    }
}

impl Hasher for XxHash3_128 {
    fn write(&mut self, bytes: &[u8]) {
        self.0.write(bytes);
    }

    fn finish(&self) -> u64 {
        panic!("use XxHash3_128::finish_128");
    }
}
