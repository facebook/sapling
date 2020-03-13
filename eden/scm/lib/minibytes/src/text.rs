/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use super::bytes::{AbstractBytes, AbstractOwner, SliceLike};

pub type Text = AbstractBytes<str>;
pub trait TextOwner: AsRef<str> + Send + Sync + 'static {}

impl<T: TextOwner> AbstractOwner<str> for T {}

impl Text {
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
