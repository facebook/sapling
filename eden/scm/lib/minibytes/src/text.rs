/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::any::Any;
use std::borrow::Cow;

use super::bytes::AbstractBytes;
use super::bytes::AbstractOwner;
use super::bytes::SliceLike;
use crate::Bytes;

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

    /// Creates `Text` from utf-8 encoded `Bytes`.
    /// Zero-copy if possible.
    pub fn from_utf8_lossy(bytes: Bytes) -> Self {
        match String::from_utf8_lossy(bytes.as_slice()) {
            Cow::Borrowed(..) => {
                // safety: utf-8 is checked by `from_utf8_lossy`.
                unsafe { Self::from_utf8_unchecked(bytes) }
            }
            Cow::Owned(s) => Self::from_owner(s),
        }
    }

    /// Creates `Text` from utf-8 `Bytes` in a zero-copy way.
    /// Safety: the `bytes` must be valid UTF-8.
    pub unsafe fn from_utf8_unchecked(bytes: Bytes) -> Self {
        struct Utf8Bytes(Bytes);
        impl AsRef<str> for Utf8Bytes {
            fn as_ref(&self) -> &str {
                // safety: `Utf8Bytes` is only constructed by
                // `from_utf8_unchecked`, which is marked `unsafe`.
                unsafe { std::str::from_utf8_unchecked(self.0.as_slice()) }
            }
        }
        impl TextOwner for Utf8Bytes {}
        Self::from_owner(Utf8Bytes(bytes))
    }

    #[inline]
    pub(crate) fn as_slice(&self) -> &str {
        let bytes = self.as_bytes();
        // bytes was validated as utf-8.
        unsafe { std::str::from_utf8_unchecked(bytes) }
    }
}

impl Bytes {
    /// Same as `Text::from_utf8_lossy`.
    pub fn into_text_lossy(self) -> Text {
        Text::from_utf8_lossy(self)
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
