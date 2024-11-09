/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Traits that makes the API more flexible.

/// Support `resize` in-place. Used for appending new nodes.
pub trait Resize<T> {
    fn resize(&mut self, new_len: usize, value: T);
}

impl<T: Copy> Resize<T> for Vec<T> {
    #[inline]
    fn resize(&mut self, new_len: usize, value: T) {
        Vec::resize(self, new_len, value)
    }
}
