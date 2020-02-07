/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::ops::{Range, RangeBounds};
use std::sync::Arc;

/// Immutable bytes with zero-copy slicing and cloning.
#[derive(Clone)]
pub struct Bytes {
    ptr: *const u8,
    len: usize,

    // Actual owner of the bytes. None for static buffers.
    owner: Option<Arc<dyn BytesOwner>>,
}

/// The actual storage owning the bytes.
pub trait BytesOwner: AsRef<[u8]> + Send + Sync + 'static {}

// BytesOwner is Send + Sync and Bytes is immutable.
unsafe impl Send for Bytes {}
unsafe impl Sync for Bytes {}

// Core implementation of Bytes.
impl Bytes {
    /// Returns a slice of self for the provided range.
    /// This operation is `O(1)`.
    pub fn slice(&self, range: impl RangeBounds<usize>) -> Self {
        use std::ops::Bound;
        let start = match range.start_bound() {
            Bound::Included(&n) => n,
            Bound::Excluded(&n) => n + 1,
            Bound::Unbounded => 0,
        };
        let end = match range.end_bound() {
            Bound::Included(&n) => n + 1,
            Bound::Excluded(&n) => n,
            Bound::Unbounded => self.len,
        };
        assert!(start <= end, "invalid slice {}..{}", start, end);
        assert!(end <= self.len, "{} exceeds Bytes length {}", end, self.len);
        if start == end {
            Self::new()
        } else {
            Self {
                ptr: unsafe { self.ptr.offset(start as isize) },
                len: end - start,
                owner: self.owner.clone(),
            }
        }
    }

    /// Attempt to convert `slice` to a zero-copy slice of this `Bytes`.
    /// Copy the `slice` if zero-copy cannot be done.
    ///
    /// This is similar to `bytes::Bytes::slice_ref` from `bytes 0.5.4`,
    /// but does not panic.
    pub fn slice_to_bytes(&self, slice: &[u8]) -> Self {
        match self.range_of_slice(slice) {
            Some(range) => self.slice(range),
            None => Self::copy_from_slice(slice),
        }
    }

    /// Return a range `x` so that `self[x]` matches `slice` exactly
    /// (not only content, but also internal pointer addresses).
    ///
    /// Returns `None` if `slice` is outside the memory range of this
    /// `Bytes`.
    ///
    /// This operation is `O(1)`.
    pub fn range_of_slice(&self, slice: &[u8]) -> Option<Range<usize>> {
        let slice_start = slice.as_ptr() as usize;
        let slice_end = slice_start + slice.len();
        let bytes_start = self.ptr as usize;
        let bytes_end = bytes_start + self.len;
        if slice_start >= bytes_start && slice_end <= bytes_end {
            let start = slice_start - bytes_start;
            Some(start..start + slice.len())
        } else {
            None
        }
    }

    /// Creates an empty `Bytes`.
    #[inline]
    pub fn new() -> Self {
        Self::from_static(b"")
    }

    /// Creates `Bytes` from a static slice.
    #[inline]
    pub fn from_static(value: &'static [u8]) -> Self {
        let slice: &[u8] = value.as_ref();
        Self {
            ptr: slice.as_ptr(),
            len: slice.len(),
            owner: None,
        }
    }

    /// Creates `Bytes` from a [`BytesOwner`] (for example, `Vec<u8>`).
    pub fn from_owner(value: impl BytesOwner) -> Self {
        let slice: &[u8] = value.as_ref();
        Self {
            ptr: slice.as_ptr(),
            len: slice.len(),
            owner: Some(Arc::new(value)),
        }
    }

    /// Creates `Bytes` instance from slice, by copying it.
    pub fn copy_from_slice(data: &[u8]) -> Self {
        Self::from_owner(data.to_vec())
    }

    #[inline]
    pub(crate) fn as_slice(&self) -> &[u8] {
        unsafe { std::slice::from_raw_parts(self.ptr, self.len) }
    }
}
