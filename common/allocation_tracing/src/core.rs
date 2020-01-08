/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use anyhow::Error;
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::UnsafeCell;
use std::convert::TryInto;
use std::mem;
use std::thread_local;

thread_local! {
    // NOTE: We use an UnsafeCell here: ALLOCATION_STATS is a thread local that is never borrowed
    // by more than one callsite at a time.
    static ALLOCATION_STATS: UnsafeCell<AllocationStats> = UnsafeCell::new(AllocationStats::default());
}

struct TracingAllocator;

// NOTE: This code assumes that we don't try to allocate more than a u64 at a time: that's probably
// a reasonable assumption to make.
// NOTE: thread_local initialization doesn't allocate, which is why we can use a thread local here.
// if it did allocate, any allocation with TracingAllocator would hang forever.
unsafe impl GlobalAlloc for TracingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATION_STATS.with(|cell| {
            (*cell.get()).allocated += layout.size() as u64;
        });

        System.alloc(layout)
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        ALLOCATION_STATS.with(|cell| {
            (*cell.get()).freed += layout.size() as u64;
        });

        System.dealloc(ptr, layout)
    }
}

#[global_allocator]
static GLOBAL: TracingAllocator = TracingAllocator;

#[derive(Debug, Copy, Clone, Default, Hash, PartialEq, Eq)]
pub struct AllocationStats {
    allocated: u64,
    freed: u64,
}

impl AllocationStats {
    pub fn delta(&self) -> Result<i64, Error> {
        let allocated: i64 = self.allocated.try_into()?;
        let freed: i64 = self.freed.try_into()?;
        Ok(allocated - freed)
    }
}

/// trace_allocations returns AllocationStats representing memory that was allocated and freed
/// during the execution of f. Note that the two numbers are somewhat independent. For example:
/// - Objects allocated in f that are moved out of f won't be freed by f.
/// - Objects moved into f might be freed by f even though they were not allocated by f.
pub fn trace_allocations<T, F>(f: F) -> (T, AllocationStats)
where
    F: FnOnce() -> T,
{
    let new = AllocationStats::default();
    let curr = ALLOCATION_STATS.with(|cell| mem::replace(unsafe { &mut *cell.get() }, new));
    let ret = f();
    let stats = ALLOCATION_STATS.with(|cell| mem::replace(unsafe { &mut *cell.get() }, curr));
    (ret, stats)
}

#[cfg(test)]
mod test {
    use super::*;

    #[derive(Copy, Clone, Default, Debug)]
    struct TestStruct1 {
        a: u64,
    }

    #[derive(Copy, Clone, Default, Debug)]
    struct TestStruct2 {
        a: u64,
        b: u64,
    }

    #[derive(Copy, Clone, Default, Debug)]
    struct TestStruct3 {
        a: u64,
        b: u64,
        c: u64,
    }

    #[derive(Copy, Clone, Default, Debug)]
    struct TestStruct4 {
        a: u64,
        b: u64,
        c: u64,
        d: u64,
    }

    #[derive(Copy, Clone, Default, Debug)]
    struct TestStruct5 {
        a: u64,
        b: u64,
        c: u64,
        d: u64,
        e: u64,
    }

    fn alloc_some<T: Default>() -> Box<T> {
        Box::new(T::default())
    }

    #[test]
    fn test_baseline_allocation() -> Result<(), Error> {
        let (_, baseline) = trace_allocations(|| ());
        assert_eq!(baseline.delta()?, 0);
        Ok(())
    }

    #[test]
    fn test_trace_allocations() -> Result<(), Error> {
        fn run_test<T: Default>() -> Result<(), Error> {
            let (b, stats) = trace_allocations(alloc_some::<T>);
            mem::drop(b);
            assert_eq!(stats.delta()?, mem::size_of::<T>() as i64);
            Ok(())
        }

        run_test::<TestStruct1>()?;
        run_test::<TestStruct2>()?;
        run_test::<TestStruct3>()?;
        run_test::<TestStruct4>()?;
        run_test::<TestStruct5>()?;
        Ok(())
    }

    #[test]
    fn test_trace_frees() -> Result<(), Error> {
        fn run_test<T: Default>() -> Result<(), Error> {
            let b = alloc_some::<T>();
            let (_, stats) = trace_allocations(move || mem::drop(b));
            assert_eq!(stats.delta()?, -(mem::size_of::<T>() as i64));
            Ok(())
        }

        run_test::<TestStruct1>()?;
        run_test::<TestStruct2>()?;
        run_test::<TestStruct3>()?;
        run_test::<TestStruct4>()?;
        run_test::<TestStruct5>()?;
        Ok(())
    }

    #[test]
    fn test_nested() -> Result<(), Error> {
        fn run_test<T: Default>() -> Result<(), Error> {
            let ((b, stats1), stats2) = trace_allocations(|| {
                let (b1, stats1) = trace_allocations(alloc_some::<T>);
                (b1, stats1)
            });
            mem::drop(b);
            let delta = stats1.delta()? + stats2.delta()?;
            assert_eq!(delta, mem::size_of::<T>() as i64);
            Ok(())
        }

        run_test::<TestStruct1>()?;
        run_test::<TestStruct2>()?;
        run_test::<TestStruct3>()?;
        run_test::<TestStruct4>()?;
        run_test::<TestStruct5>()?;
        Ok(())
    }
}
