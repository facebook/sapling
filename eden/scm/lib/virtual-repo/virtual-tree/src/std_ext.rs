/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Extension to stdlib

use std::num::NonZeroU64;

pub trait LosslessShl {
    /// Ensure `<<` does not overflow.
    /// Note stdlib `checked_shl`, and `strict_shl` check against `Self::BITS`,
    /// not against `self`.
    fn lossless_shl(self, bits: u8) -> Self;
}

impl LosslessShl for u64 {
    fn lossless_shl(self, bits: u8) -> Self {
        let result = self << bits;
        assert_eq!(result >> bits, self, "shl is not lossless");
        result
    }
}

impl LosslessShl for usize {
    fn lossless_shl(self, bits: u8) -> Self {
        let result = self << bits;
        assert_eq!(result >> bits, self, "shl is not lossless");
        result
    }
}

impl LosslessShl for NonZeroU64 {
    fn lossless_shl(self, bits: u8) -> Self {
        let result = self.get().lossless_shl(bits);
        Self::new(result).unwrap()
    }
}
