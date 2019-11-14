/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
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
