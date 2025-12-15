/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Operation system features used by the profiler.

use std::io;
use std::mem;

// Block `sig` signals. Explicitly opt-out profiling for the current thread
// and new threads spawned from the current thread.
pub fn block_signal(sig: libc::c_int) {
    sigmask_sigprof(sig, true);
}

/// Unblock `sig` to enable profiling.
pub fn unblock_signal(sig: libc::c_int) {
    sigmask_sigprof(sig, false);
}

// Get the current thread id. Must be async-signal-safe.
pub fn get_thread_id() -> libc::pid_t {
    #[cfg(target_os = "linux")]
    unsafe {
        libc::syscall(libc::SYS_gettid) as libc::pid_t
    }
    #[cfg(not(target_os = "linux"))]
    unimplemented!()
}

/// Create a pipe for SIGPROF signal handler use.
/// The SIGPROF handler sends raw stack trace info to the pipe.
/// The other end of the pipe consumes the data and might resolve symbols.
/// The pipe is configured to have a larger buffer, so it's less likely to block.
/// Returns `[read_fd, write_fd]`.
pub fn setup_pipe() -> io::Result<[libc::c_int; 2]> {
    unsafe {
        let mut pipe_fds: [libc::c_int; 2] = [0; 2];

        if libc::pipe2(pipe_fds.as_mut_ptr(), libc::O_DIRECT) != 0 {
            return Err(io::Error::last_os_error());
        }

        // F_SETPIPE_SZ is linux specific.
        #[cfg(target_os = "linux")]
        {
            let buffer_size = 1 << 6;
            // Failing to set buffer size is not fatal.
            let _ = libc::fcntl(pipe_fds[1], libc::F_SETPIPE_SZ, buffer_size);
        }

        Ok(pipe_fds)
    }
}

/// Setup the signal handler. This is POSIX-only.
pub fn setup_signal_handler(
    sig: libc::c_int,
    signal_handler: extern "C" fn(libc::c_int, *const libc::siginfo_t, *const libc::c_void),
) -> io::Result<()> {
    unsafe {
        let mut sa: libc::sigaction = std::mem::zeroed();
        sa.sa_sigaction = signal_handler as usize;
        sa.sa_flags = libc::SA_RESTART | libc::SA_SIGINFO;
        libc::sigemptyset(&mut sa.sa_mask);

        if libc::sigaction(sig, &sa, std::ptr::null_mut()) != 0 {
            return Err(io::Error::last_os_error());
        }
    }

    Ok(())
}

/// Send `sig` to `tid` at the specified interval. This is a Linux-only feature.
/// Returns the timer handle that can be used to stop the timer later.
pub fn setup_signal_timer(
    sig: libc::c_int,
    tid: libc::pid_t,
    interval_secs: i64,
    interval_nsecs: i64,
) -> io::Result<libc::timer_t> {
    #[cfg(target_os = "linux")]
    unsafe {
        let mut sev: libc::sigevent = mem::zeroed();
        sev.sigev_notify = libc::SIGEV_THREAD_ID;
        sev.sigev_signo = sig;
        sev.sigev_notify_thread_id = tid;

        let mut timer: libc::timer_t = mem::zeroed();

        // CLOCK_MONOTONIC does not include system suspend time.
        if libc::timer_create(libc::CLOCK_MONOTONIC, &mut sev, &mut timer) != 0 {
            return Err(io::Error::last_os_error());
        }

        let mut spec: libc::itimerspec = mem::zeroed();
        spec.it_interval.tv_sec = interval_secs;
        spec.it_interval.tv_nsec = interval_nsecs;
        spec.it_value.tv_sec = interval_secs;
        spec.it_value.tv_nsec = interval_nsecs;

        if libc::timer_settime(timer, 0, &spec, std::ptr::null_mut()) != 0 {
            return Err(io::Error::last_os_error());
        }

        Ok(timer)
    }

    #[cfg(not(target_os = "linux"))]
    unimplemented!();
}

/// Stop and delete a signal timer created by `setup_signal_timer`.
pub fn stop_signal_timer(timer: libc::timer_t) -> io::Result<()> {
    #[cfg(target_os = "linux")]
    unsafe {
        if libc::timer_delete(timer) != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    #[cfg(not(target_os = "linux"))]
    unimplemented!();
}

fn sigmask_sigprof(sig: libc::c_int, block: bool) {
    unsafe {
        let mut set: libc::sigset_t = mem::zeroed();
        libc::sigemptyset(&mut set);
        libc::sigaddset(&mut set, sig);
        let how = match block {
            true => libc::SIG_BLOCK,
            _ => libc::SIG_UNBLOCK,
        };
        libc::pthread_sigmask(how, &set, std::ptr::null_mut());
    }
}
