/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! "Page out" logic as an attempt to reduce RSS / Working Set usage.

use std::sync::Mutex;
use std::sync::OnceLock;
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

/// By default, trigger `page_out()` after approximately 2GB of `Log` reads.
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

pub(crate) fn account_read(len: usize) {
    let accounted = page_size().map_or(len, |page_size| len.max(page_size));
    adjust_available(-i64::try_from(accounted).unwrap_or(i64::MAX));
}

pub(crate) fn page_size() -> Option<usize> {
    static PAGE_SIZE: OnceLock<Option<usize>> = OnceLock::new();
    *PAGE_SIZE.get_or_init(query_page_size)
}

#[cfg(unix)]
fn query_page_size() -> Option<usize> {
    // SAFETY: `_SC_PAGESIZE` does not require any caller-provided pointers.
    let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    usize::try_from(page_size)
        .ok()
        .filter(|page_size| *page_size > 0)
}

#[cfg(windows)]
fn query_page_size() -> Option<usize> {
    use std::mem::MaybeUninit;

    use winapi::um::sysinfoapi::GetSystemInfo;
    use winapi::um::sysinfoapi::SYSTEM_INFO;

    let mut info = MaybeUninit::<SYSTEM_INFO>::uninit();
    // SAFETY: `info` provides writable storage that `GetSystemInfo` initializes.
    let page_size = unsafe {
        GetSystemInfo(info.as_mut_ptr());
        info.assume_init().dwPageSize as usize
    };
    (page_size > 0).then_some(page_size)
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
    if let Ok(locked) = BUFFERS.try_lock() {
        if let Some((start, end)) = locked.find_region(addr) {
            return Some((start, end, false));
        }
    }

    // Also check the change_detect mmap buffers.
    if let Ok(locked) = crate::change_detect::BUFFERS.try_lock() {
        if let Some((start, end)) = locked.find_region(addr) {
            return Some((start, end, true));
        }
    }

    None
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
        self.gc_tick += 1;
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
                    slice.len(),
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

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    #[test]
    fn find_region_checks_change_detector_when_log_buffer_lock_is_busy() {
        use crate::change_detect::SharedChangeDetector;
        use crate::lock::DirLockOptions;
        use crate::lock::ScopedDirLock;

        let dir = tempfile::tempdir().unwrap();
        let opts = DirLockOptions {
            exclusive: false,
            non_blocking: false,
            file_name: "rlock",
        };
        let lock = ScopedDirLock::new_with_options(dir.path(), &opts).unwrap();
        let mmap = lock.shared_mmap_mut(std::mem::size_of::<u64>()).unwrap();
        let addr = mmap.as_ptr() as usize;

        let _detector = SharedChangeDetector::new(mmap);

        let _log_buffers = super::BUFFERS.lock().unwrap();
        // Holding BUFFERS must not prevent the SIGBUS handler from finding
        // rlock mmaps tracked by change_detect::BUFFERS.
        assert_eq!(
            super::find_region(addr).map(|(_start, _end, writable)| writable),
            Some(true)
        );
    }
}
