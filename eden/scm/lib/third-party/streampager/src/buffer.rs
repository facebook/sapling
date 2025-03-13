//! Fillable buffer
//!
//! Buffers used for loading streams.

use std::ops::{Deref, DerefMut};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard};

use memmap2::MmapMut;

/// Fillable buffer
///
/// This struct enapsulates a memory-mapped buffer that can be simultaneously written to and read
/// from.  Writes can only be appends, and reads can only happen to the portion that has already
/// been written.
pub(crate) struct Buffer {
    /// The underlying memory map.  This can be accessed for both reading and writing, and so is
    /// stored in an UnsafeCell.
    _mmap: MmapMut,

    /// Pointer to the data in `_mmap`.
    data_ptr: *mut u8,

    /// The underlying memory map's capacity.
    capacity: usize,

    /// How much of the buffer has been filled.  Reads are permitted in the range `0..filled`.
    /// Writes are permitted in the range `filled..`.
    filled: AtomicUsize,

    /// Lock for write access, to ensure only one writer can write at a time.
    lock: Mutex<()>,
}

unsafe impl Send for Buffer {}
unsafe impl Sync for Buffer {}

pub(crate) struct BufferWrite<'buffer> {
    /// The buffer this write is for.
    buffer: &'buffer Buffer,

    /// Lock guard for write access.
    _guard: MutexGuard<'buffer, ()>,
}

impl Buffer {
    pub(crate) fn new(capacity: usize) -> Buffer {
        let mut mmap = MmapMut::map_anon(capacity).unwrap();
        let data_ptr = mmap.as_mut_ptr();
        Buffer {
            _mmap: mmap,
            data_ptr,
            capacity,
            filled: AtomicUsize::new(0usize),
            lock: Mutex::new(()),
        }
    }

    /// Returns the writable portion of the buffer
    pub(crate) fn write(&self) -> BufferWrite<'_> {
        BufferWrite {
            buffer: self,
            _guard: self.lock.lock().unwrap(),
        }
    }

    /// Returns the readable portion of the buffer
    pub(crate) fn read(&self) -> &[u8] {
        let end = self.filled.load(Ordering::SeqCst);
        // Safety: `BufferWrite::written()` checks that `end <= capacity`
        unsafe { std::slice::from_raw_parts(self.data_ptr, end) }
    }

    #[cfg(feature = "load_file")]
    /// Returns the size of the readable portion of the buffer
    pub(crate) fn available(&self) -> usize {
        self.filled.load(Ordering::SeqCst)
    }
}

impl<'buffer> BufferWrite<'buffer> {
    /// Completes the write operation for `len` bytes to the buffer.  After calling `written`, the
    /// data is made available to callers to `read`.
    pub(crate) fn written(self, len: usize) {
        let new_filled = self.buffer.filled.load(Ordering::SeqCst).saturating_add(len).clamp(0, isize::MAX as usize);
        assert!(new_filled <= self.buffer.capacity);
        self.buffer.filled.store(new_filled, Ordering::SeqCst);
    }
}

impl<'buffer> Deref for BufferWrite<'buffer> {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        let start = self.buffer.filled.load(Ordering::SeqCst);
        let start_ptr = unsafe { self.buffer.data_ptr.add( start) };
        // Safety:
        // * `BufferWrite::written()` enforces that `filled` is within capacity. It starts at 0 and
        //    never shrinks.
        // * `_guard` enforces that no concurrent mutable reference exists
        // * The slices returned from `BufferWrite::deref()/deref_mut()` never overlap with those
        //   from `Buffer::read` because the latter goes from `0..filled` and the former go from
        //   `filled..capacity`. The latter are no longer accessible once `filled` has been updated
        //   because `written()` consumes its argument.
        unsafe { std::slice::from_raw_parts(start_ptr, self.buffer.capacity - start) }
    }
}

impl<'buffer> DerefMut for BufferWrite<'buffer> {
    fn deref_mut(&mut self) -> &mut [u8] {
        let start = self.buffer.filled.load(Ordering::SeqCst);
        let start_ptr = unsafe { self.buffer.data_ptr.add( start) };
        // Safety:
        // * `BufferWrite::written()` enforces that `filled` is within capacity. It starts at 0 and
        //    never shrinks.
        // * `_guard` enforces that no concurrent mutable reference exists
        // * The slices returned from `BufferWrite::deref()/deref_mut()` never overlap with those
        //   from `Buffer::read` because the latter goes from `0..filled` and the former go from
        //   `filled..capacity`. The latter are no longer accessible once `filled` has been updated
        //   because `written()` consumes its argument.
        unsafe { std::slice::from_raw_parts_mut(start_ptr, self.buffer.capacity - start) }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use std::sync::Arc;
    use std::thread;

    #[test]
    fn test_buffer_write() {
        let b = Arc::new(Buffer::new(20));
        let b2 = b.clone();
        let b3 = b.clone();
        let mut w = b.write();
        // do a write
        w[0] = 42;
        w.written(1);
        // do some writes on other threads
        let t1 = thread::spawn(move || {
            let mut w = b2.write();
            w[0] = 64;
            w.written(1);
        });
        let t2 = thread::spawn(move || {
            let mut w = b3.write();
            w[0] = 81;
            w.written(1);
        });
        t1.join().unwrap();
        t2.join().unwrap();
        let mut w = b.write();
        // do another write
        w[0] = 101;
        w[1] = 99;
        w.written(2);
        assert_eq!(b.read().len(), 5);
        assert_eq!(b.read()[0], 42);
        // these two writes could have happened in any order
        assert_eq!(b.read()[1] + b.read()[2], 64 + 81);
        assert_eq!(b.read()[3], 101);
        assert_eq!(b.read()[4], 99);
    }
}
