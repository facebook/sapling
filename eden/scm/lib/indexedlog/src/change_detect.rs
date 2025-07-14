/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::sync::Arc;
use std::sync::Mutex;
use std::sync::Weak;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;

use memmap2::MmapMut;

use crate::page_out::WeakBuffers;
use crate::page_out::WeakSlice;

/// Detect changes by using a u64 counter backed by mmap.
pub(crate) struct SharedChangeDetector {
    mmap: Arc<MmapMut>,
    last_read: AtomicU64,
}

/// Useful for the SIGBUS handler to prevent the process from crashing if
/// something truncates the file backing the mmap buffer.
pub(crate) static BUFFERS: Mutex<WeakBuffers<Weak<MmapMut>>> = Mutex::new(WeakBuffers::new());

impl Clone for SharedChangeDetector {
    fn clone(&self) -> Self {
        Self {
            mmap: self.mmap.clone(),
            last_read: AtomicU64::new(self.last_read.load(Ordering::Acquire)),
        }
    }
}

impl SharedChangeDetector {
    /// Creates a new `SharedChangeDetector` from a mutable mmap buffer.
    /// Panics if the buffer is less than 8 bytes.
    pub fn new(mmap: MmapMut) -> Self {
        assert!(mmap.len() >= std::mem::size_of::<AtomicU64>());
        let last_read = AtomicU64::new(mmap_as_atomic_u64(&mmap).load(Ordering::Acquire));
        let mmap = Arc::new(mmap);
        BUFFERS.lock().unwrap().track(Arc::downgrade(&mmap));
        Self { mmap, last_read }
    }

    /// Set the shared value and clear this detector.
    /// If the value is changed, other detectors' `is_changed` would return true.
    pub fn set(&self, value: u64) {
        mmap_as_atomic_u64(&self.mmap).store(value, Ordering::Release);
        self.last_read.store(value, Ordering::Release);
    }

    /// Returns `true` if the value is changed since the last `reset` or `bump` call.
    pub fn is_changed(&self) -> bool {
        let current = mmap_as_atomic_u64(&self.mmap).load(Ordering::Acquire);
        let last = self.last_read.load(Ordering::Acquire);
        last != current
    }
}

fn mmap_as_atomic_u64(mmap: &MmapMut) -> &AtomicU64 {
    let ptr = mmap.as_ptr() as *mut u8 as *mut u64;
    // safety: we checked that the mmap buffer is large enough at new().
    unsafe { AtomicU64::from_ptr(ptr) }
}

impl Drop for SharedChangeDetector {
    fn drop(&mut self) {
        // Attempt to write to disk when the last MmapMut is being dropped.
        if let Some(mmap) = Arc::get_mut(&mut self.mmap) {
            let _ = mmap.flush_async();
        }
    }
}

impl WeakSlice for Weak<MmapMut> {
    type Upgraded = Arc<MmapMut>;
    fn upgrade(&self) -> Option<Self::Upgraded> {
        Weak::upgrade(&self)
    }
    fn as_slice(v: &Self::Upgraded) -> &[u8] {
        AsRef::as_ref(v)
    }
}
