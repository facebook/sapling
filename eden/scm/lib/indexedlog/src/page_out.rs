/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! "Page out" logic as an attempt to reduce RSS / Working Set usage.

use std::sync::Mutex;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicI64;
use std::sync::atomic::Ordering;

use minibytes::Bytes;
use minibytes::WeakBytes;

/// See `crate::config::set_page_out_threshold`.
pub(crate) static THRESHOLD: AtomicI64 = AtomicI64::new(DEFAULT_THRESHOLD);

/// Track mmap regions in order to support `find_region`.
pub(crate) static NEED_FIND_REGION: AtomicBool = AtomicBool::new(false);

/// Remaining byte count to read without `page_out()`.
static AVAILABLE: AtomicI64 = AtomicI64::new(DEFAULT_THRESHOLD);

/// Tracked buffers. Also serve as a lock for `page_out()`.
static BUFFERS: Mutex<WeakBuffers<WeakBytes>> = Mutex::new(WeakBuffers::<WeakBytes>::new());

/// By default, trigger `page_out()` after reading 2GB `Log` entries.
const DEFAULT_THRESHOLD: i64 = 1i64 << 31;

/// Collection of weak buffers.
pub(crate) struct WeakBuffers<W> {
    buffers: Vec<W>,
    gc_tick: usize,
}

pub(crate) trait WeakSlice {
    type Upgraded;
    fn upgrade(&self) -> Option<Self::Upgraded>;
    fn as_slice(v: &Self::Upgraded) -> &[u8];
}

impl WeakSlice for WeakBytes {
    type Upgraded = Bytes;
    fn upgrade(&self) -> Option<Self::Upgraded> {
        Bytes::upgrade(self)
    }
    fn as_slice(v: &Bytes) -> &[u8] {
        Bytes::as_ref(v)
    }
}

/// Adjust the `AVAILABLE`.
/// If it becomes negative when `THRESHOLD` is positive, trigger `page_out`.
pub(crate) fn adjust_available(delta: i64) {
    let old_available = AVAILABLE.fetch_add(delta as _, Ordering::AcqRel);
    if old_available + delta < 0 {
        let threshold = THRESHOLD.load(Ordering::Acquire);
        if threshold > 0 {
            let mut buffers = BUFFERS.lock().unwrap();
            AVAILABLE.store(threshold, Ordering::Release);
            tracing::info!("running page_out()");
            buffers.page_out();
        }
    }
}

/// Track the mmap buffer as a weak ref.
pub(crate) fn track_mmap_buffer(bytes: &Bytes) {
    let threshold = THRESHOLD.load(Ordering::Acquire);
    if threshold > 0 || NEED_FIND_REGION.load(Ordering::Acquire) {
        let mut buffers = BUFFERS.lock().unwrap();
        if let Some(weak) = bytes.downgrade() {
            buffers.track(weak);
        }
    }
}

/// Find the mmap region that contains the given pointer. Best effort.
/// Returns `(start, end, should_be_writable)`.
/// Does not block. Returns `None` when unable to take the lock.
#[cfg(unix)]
pub(crate) fn find_region(addr: usize) -> Option<(usize, usize, bool)> {
    let locked = BUFFERS.try_lock().ok()?;
    if let Some((start, end)) = locked.find_region(addr) {
        return Some((start, end, false));
    }
    // Also check the change_detect mmap buffers.
    let locked = crate::change_detect::BUFFERS.try_lock().ok()?;
    locked
        .find_region(addr)
        .map(|(start, end)| (start, end, true))
}

impl<W: WeakSlice> WeakBuffers<W> {
    pub(crate) const fn new() -> Self {
        Self {
            buffers: Vec::new(),
            gc_tick: 0,
        }
    }

    pub(crate) fn track(&mut self, value: W) {
        self.buffers.push(value);
        self.gc_tick = self.gc_tick + 1;
        if self.gc_tick > crate::config::WEAK_BUFFER_GC_THRESHOLD.load(Ordering::Acquire) {
            self.for_each_alive_buffer(None); // side effect: gc
            self.gc_tick = 0;
        }
    }

    fn find_region(&self, addr: usize) -> Option<(usize, usize)> {
        for weak in self.buffers.iter() {
            let bytes = match WeakSlice::upgrade(weak) {
                None => continue,
                Some(bytes) => bytes,
            };
            let buf = W::as_slice(&bytes);
            let start = buf.as_ptr() as usize;
            let len = buf.len();
            if start <= addr && start.wrapping_add(len) > addr {
                return Some((start, len));
            }
        }
        None
    }

    /// Run logic on each buffer that is still alive.
    /// Drops buffers that are dead.
    fn for_each_alive_buffer(&mut self, callback: Option<fn(&[u8])>) {
        let mut new_buffers = Vec::new();
        for weak in self.buffers.drain(..) {
            let bytes = match WeakSlice::upgrade(&weak) {
                None => continue,
                Some(bytes) => bytes,
            };
            if let Some(callback) = callback {
                let slice: &[u8] = W::as_slice(&bytes);
                callback(slice);
            }
            new_buffers.push(weak);
        }
        self.buffers = new_buffers;
    }

    #[cfg(unix)]
    fn page_out(&mut self) {
        self.for_each_alive_buffer(Some(|slice| {
            let ret = unsafe {
                libc::madvise(
                    slice.as_ptr() as *const libc::c_void as *mut libc::c_void,
                    slice.len() as _,
                    libc::MADV_DONTNEED,
                )
            };
            tracing::debug!(
                "madvise({} bytes, MADV_DONTNEED) returned {}",
                slice.len(),
                ret
            );
        }));
    }

    #[cfg(windows)]
    fn page_out(&mut self) {
        use winapi::um::processthreadsapi::GetCurrentProcess;
        use winapi::um::psapi::EmptyWorkingSet;

        unsafe {
            let handle = GetCurrentProcess();
            let ret = EmptyWorkingSet(handle);
            tracing::debug!("EmptyWorkingSet returned {}", ret);
        }
    }
}
