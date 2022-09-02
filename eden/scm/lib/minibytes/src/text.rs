/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::any::Any;

use super::bytes::AbstractBytes;
use super::bytes::AbstractOwner;
use super::bytes::SliceLike;

pub type Text = AbstractBytes<str>;
pub trait TextOwner: AsRef<str> + Send + Sync + 'static {}

impl<T: TextOwner> AbstractOwner<str> for T {
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

impl Text {
    /// Creates `Text` from a static str.
    pub const fn from_static(slice: &'static str) -> Self {
        Self {
            ptr: slice.as_ptr(),
            len: slice.len(),
            owner: None,
        }
    }

    #[inline]
    pub(crate) fn as_slice(&self) -> &str {
        let bytes = self.as_bytes();
        // bytes was validated as utf-8.
        unsafe { std::str::from_utf8_unchecked(bytes) }
    }
}

impl SliceLike for str {
    type Owned = String;
    const EMPTY: &'static Self = "";

    #[inline]
    fn check_slice_bytes(bytes: &[u8], start: usize, end: usize) {
        // called by AbstractBytes::slice, bytes was validated as utf-8.
        let s = unsafe { std::str::from_utf8_unchecked(bytes) };
        // check whether the slicing is valid.
        let _ = s[start..end];
    }
    #[inline]
    fn as_bytes(&self) -> &[u8] {
        self.as_bytes()
    }
    #[inline]
    fn to_owned(&self) -> Self::Owned {
        self.to_string()
    }
}
