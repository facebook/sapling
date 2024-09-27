/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::any::Any;
use std::ops::Range;
use std::ops::RangeBounds;
use std::sync::Arc;
use std::sync::Weak;

pub type Bytes = AbstractBytes<[u8]>;
pub trait BytesOwner: AsRef<[u8]> + Send + Sync + 'static {}

pub type WeakBytes = Weak<dyn AbstractOwner<[u8]>>;

/// Immutable bytes with zero-copy slicing and cloning.
pub struct AbstractBytes<T: ?Sized> {
    pub(crate) ptr: *const u8,
    pub(crate) len: usize,

    // Actual owner of the bytes. None for static buffers.
    pub(crate) owner: Option<Arc<dyn AbstractOwner<T>>>,
}

/// The actual storage owning the bytes.
pub trait AbstractOwner<T: ?Sized>: AsRef<T> + Send + Sync + 'static {
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

impl<T: BytesOwner> AbstractOwner<[u8]> for T {
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

// AbstractOwner<T> is Send + Sync and AbstractBytes<T> is immutable.
unsafe impl<T: ?Sized> Send for AbstractBytes<T> {}
unsafe impl<T: ?Sized> Sync for AbstractBytes<T> {}

// #[derive(Clone)] does not work well with type parameters.
// Therefore implement Clone manually.
impl<T: ?Sized> Clone for AbstractBytes<T> {
    fn clone(&self) -> Self {
        Self {
            ptr: self.ptr,
            len: self.len,
            owner: self.owner.clone(),
        }
    }
}

// Core implementation of Bytes.
impl<T> AbstractBytes<T>
where
    T: SliceLike + ?Sized,
    T::Owned: AbstractOwner<T>,
{
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
            T::check_slice_bytes(self.as_bytes(), start, end);
            Self {
                ptr: unsafe { self.ptr.add(start) },
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
    pub fn slice_to_bytes(&self, slice: &T) -> Self {
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
    pub fn range_of_slice(&self, slice: &T) -> Option<Range<usize>> {
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
        let empty = T::EMPTY.as_bytes();
        Self {
            ptr: empty.as_ptr(),
            len: empty.len(),
            owner: None,
        }
    }

    /// Creates `Bytes` from a [`BytesOwner`] (for example, `Vec<u8>`).
    pub fn from_owner(value: impl AbstractOwner<T>) -> Self {
        let slice: &T = value.as_ref();
        let bytes = slice.as_bytes();
        Self {
            ptr: bytes.as_ptr(),
            len: bytes.len(),
            owner: Some(Arc::new(value)),
        }
    }

    /// Creates `Bytes` instance from slice, by copying it.
    pub fn copy_from_slice(data: &T) -> Self {
        Self::from_owner(data.to_owned())
    }

    #[inline]
    pub(crate) fn as_bytes(&self) -> &[u8] {
        unsafe { std::slice::from_raw_parts(self.ptr, self.len) }
    }

    /// Create a weak pointer. Returns `None` if backed by a static buffer.
    /// Note the weak pointer has the full range of the buffer.
    pub fn downgrade(&self) -> Option<Weak<dyn AbstractOwner<T>>> {
        self.owner.as_ref().map(Arc::downgrade)
    }

    /// The reverse of `downgrade`. Returns `None` if the value was dropped.
    /// Note the upgraded `Bytes` has the full range of the buffer.
    pub fn upgrade(weak: &Weak<dyn AbstractOwner<T>>) -> Option<Self> {
        let arc = weak.upgrade()?;
        let slice_like: &T = arc.as_ref().as_ref();
        Some(Self {
            ptr: slice_like.as_ptr(),
            len: slice_like.len(),
            owner: Some(arc),
        })
    }
}

impl Bytes {
    #[inline]
    pub(crate) fn as_slice(&self) -> &[u8] {
        self.as_bytes()
    }

    /// Creates `Bytes` from a static slice.
    pub const fn from_static(slice: &'static [u8]) -> Self {
        Self {
            ptr: slice.as_ptr(),
            len: slice.len(),
            owner: None,
        }
    }

    /// Convert to `Vec<u8>`, in a zero-copy way if possible.
    pub fn into_vec(mut self) -> Vec<u8> {
        let len = self.len();

        'zero_copy: {
            let arc_owner = match self.owner.as_mut() {
                None => break 'zero_copy,
                Some(owner) => owner,
            };
            let owner = match Arc::get_mut(arc_owner) {
                None => break 'zero_copy,
                Some(owner) => owner,
            };
            let any = owner.as_any_mut();
            let mut maybe_vec = any.downcast_mut::<Vec<u8>>();
            match maybe_vec {
                Some(ref mut owner) if owner.len() == len => {
                    let mut result: Vec<u8> = Vec::new();
                    std::mem::swap(&mut result, owner);
                    return result;
                }
                _ => break 'zero_copy,
            }
        }

        self.as_slice().to_vec()
    }
}

#[cfg(feature = "non-zerocopy-into")]
impl From<Bytes> for Vec<u8> {
    fn from(value: Bytes) -> Vec<u8> {
        value.into_vec()
    }
}

pub trait SliceLike: 'static {
    type Owned;
    const EMPTY: &'static Self;

    fn as_bytes(&self) -> &[u8];
    fn to_owned(&self) -> Self::Owned;

    #[inline]
    fn check_slice_bytes(bytes: &[u8], start: usize, end: usize) {
        let _ = (bytes, start, end);
    }

    #[inline]
    fn len(&self) -> usize {
        self.as_bytes().len()
    }

    #[inline]
    fn as_ptr(&self) -> *const u8 {
        self.as_bytes().as_ptr()
    }
}

impl SliceLike for [u8] {
    type Owned = Vec<u8>;
    const EMPTY: &'static Self = b"";

    #[inline]
    fn as_bytes(&self) -> &[u8] {
        self
    }
    #[inline]
    fn to_owned(&self) -> Self::Owned {
        self.to_vec()
    }
}
