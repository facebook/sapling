/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::marker::PhantomData;
use std::thread;
use std::thread::JoinHandle;
use std::time::Duration;

use crate::ResolvedBacktraceProcessFunc;
use crate::frame_handler;
use crate::osutil;
use crate::osutil::OwnedFd;
use crate::osutil::OwnedTimer;
use crate::signal_handler;

/// Represents a profiling configuration for the owning thread.
/// Contains resources (fd, timer) allocated.
/// Dropping this struct stops the profiler.
pub struct Profiler {
    /// (Linux) Timer ID for the SIGPROF timer.
    timer_id: OwnedTimer,
    /// Frame information to write to (from signal handler).
    pipe_write_fd: OwnedFd,
    /// Frame handling thread.
    handle: Option<JoinHandle<()>>,
    // Unimplement Send+Sync.
    // This avoids tricky race conditions during "stop".
    // Without this, a race condition might look like:
    // 1. [thread 1] Start profiling for thread 1.
    // 2. [thread 1] Enter signal handler. Before reading pipe fd.
    // 3. [thread 2] Stop profiling. Close the pipe fd.
    // 4. [thread ?] Create a new fd that happened to match the closed fd.
    // 5. [thread 1] Read pipe fd. Got the wrong fd.
    // If the profiling for thread 1 can only be stopped by thread 1,
    // then the stop logic can stop the timer, assume the signal handler isn't
    // (and won't) run, then close the fd.
    _marker: PhantomData<*const ()>,
}

const SIG: i32 = libc::SIGPROF;

impl Profiler {
    /// Start profiling the current thread with the given interval.
    /// `backtrace_process_func` is a callback to receive resolved frames
    /// (most recent call first).
    pub fn new(
        interval: Duration,
        backtrace_process_func: ResolvedBacktraceProcessFunc,
    ) -> anyhow::Result<Self> {
        // Prepare the pipe fds.
        // - read_fd: used and owned by the frame_reader_loop thread.
        //   will be closed on EOF (closing write_fd).
        // - write_fd: used by the signal handler. owned by `Profiler`.
        //   will be closed when dropping `Profiler`.
        let [read_fd, write_fd] = osutil::setup_pipe()?;

        osutil::setup_signal_handler(SIG, signal_handler::signal_handler)?;
        osutil::unblock_signal(SIG);

        // Spawn a thread to read the pipe. The thread exits on pipe EOF.
        let thread_id = osutil::get_thread_id();
        let handle = thread::Builder::new()
            .name(format!("profiler-consumer-{thread_id:?}"))
            .spawn(move || {
                osutil::block_signal(SIG);
                frame_handler::frame_reader_loop(read_fd, backtrace_process_func);
            })?;

        // Start a timer that sends signals to the target thread periodically.
        let timer_id = osutil::setup_signal_timer(SIG, thread_id, interval, write_fd.0 as isize)?;

        Ok(Self {
            timer_id,
            pipe_write_fd: write_fd,
            handle: Some(handle),
            _marker: PhantomData,
        })
    }
}

impl Drop for Profiler {
    fn drop(&mut self) {
        // Stop timer before dropping fds.
        self.timer_id.stop();
        osutil::block_signal(SIG);
        // Consume pending signals previously queued by the timer to avoid
        // surprises. This might cancel unrelated signal queues from nested
        // profilers, losing some profiling accuracy.
        osutil::drain_pending_signals(SIG);
        self.pipe_write_fd.close();
        // Wait for `backtrace_process_func` to complete.
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}
