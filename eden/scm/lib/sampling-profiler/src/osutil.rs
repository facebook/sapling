/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Operation system features used by the profiler.

use std::io;
use std::mem;
use std::ptr;
use std::time::Duration;

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
#[cfg(target_os = "linux")]
pub fn get_thread_id() -> libc::pid_t {
    unsafe { libc::syscall(libc::SYS_gettid) as libc::pid_t }
}

#[cfg(target_os = "macos")]
pub fn get_thread_id() -> libc::pthread_t {
    unsafe { libc::pthread_self() }
}

/// Similar to stdlib `OwnedFd`.
/// But also allows a "null" state, and supports `close` early.
pub struct OwnedFd(pub i32);

impl OwnedFd {
    pub fn close(&mut self) {
        if self.0 >= 0 {
            let _ = unsafe { libc::close(self.0) };
            self.0 = -1;
        }
    }

    pub fn into_raw_fd(mut self) -> Option<i32> {
        let mut ret = None;
        if self.0 >= 0 {
            ret = Some(self.0);
            self.0 = -1;
        }
        ret
    }
}

impl Drop for OwnedFd {
    fn drop(&mut self) {
        self.close();
    }
}

/// Create a pipe for SIGPROF signal handler use.
/// The SIGPROF handler sends raw stack trace info to the pipe.
/// The other end of the pipe consumes the data and might resolve symbols.
/// The pipe is configured with:
/// - O_DIRECT: Enables "packet-mode". No need to deal with payload boundaries.
/// - O_NONBLOCK on write: Slow reader won't block writers.
/// - have a larger buffer to reduce changes data gets dropped (on supported
///   platforms like Linux).
/// Returns `[read_fd, write_fd]`.
pub fn setup_pipe() -> io::Result<[OwnedFd; 2]> {
    #[cfg(target_os = "linux")]
    unsafe {
        let mut pipe_fds: [libc::c_int; 2] = [0; 2];

        if libc::pipe2(pipe_fds.as_mut_ptr(), libc::O_DIRECT) != 0 {
            return Err(io::Error::last_os_error());
        }
        let (rfd, wfd) = (OwnedFd(pipe_fds[0]), OwnedFd(pipe_fds[1]));

        // The default pipe buffer is 4KB. It only fits 4 frames, too small for
        // a backtrace. Increase it to 1MB, which might fit 900 frames.
        let buffer_size = 1 << 20;
        let ret = libc::fcntl(pipe_fds[1], libc::F_SETPIPE_SZ, buffer_size);
        if ret == -1 {
            return Err(io::Error::last_os_error());
        } else if ret < buffer_size {
            return Err(io::Error::other(format!(
                "pipe buffer {} is too small",
                ret
            )));
        }

        // Set the write end as non-blocking.
        let flags = libc::fcntl(pipe_fds[1], libc::F_GETFL, 0);
        if flags == -1 {
            return Err(io::Error::last_os_error());
        }
        if libc::fcntl(pipe_fds[1], libc::F_SETFL, flags | libc::O_NONBLOCK) == -1 {
            return Err(io::Error::last_os_error());
        }

        Ok([rfd, wfd])
    }

    #[cfg(not(target_os = "linux"))]
    Err(io::ErrorKind::Unsupported.into())
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

pub struct OwnedTimer(*mut libc::c_void);

impl Drop for OwnedTimer {
    fn drop(&mut self) {
        self.stop();
    }
}

impl OwnedTimer {
    pub fn stop(&mut self) {
        if !self.0.is_null() {
            let _ = stop_signal_timer(self.0);
            self.0 = ptr::null_mut();
        }
    }
}
#[cfg(target_os = "linux")]
pub(crate) use libc::timer_t;

#[cfg(not(target_os = "linux"))]
#[allow(non_camel_case_types)]
pub(crate) type timer_t = *mut libc::c_void;

/// Send `sig` to `tid` at the specified interval. This is a Linux-only feature.
/// Returns the timer handle that can be used to stop the timer later.
pub fn setup_signal_timer(
    sig: libc::c_int,
    tid: libc::pid_t,
    interval: Duration,
    sigev_value: isize,
) -> io::Result<OwnedTimer> {
    #[cfg(target_os = "linux")]
    unsafe {
        let mut sev: libc::sigevent = mem::zeroed();
        sev.sigev_notify = libc::SIGEV_THREAD_ID;
        sev.sigev_signo = sig;
        sev.sigev_notify_thread_id = tid;
        // In C, sigev is a union of int and `void*`.
        // So it's okay to treat it as an int, not a pointer.
        sev.sigev_value = mem::transmute_copy(&sigev_value);

        let mut timer: libc::timer_t = mem::zeroed();

        // CLOCK_MONOTONIC does not include system suspend time.
        if libc::timer_create(libc::CLOCK_MONOTONIC, &mut sev, &mut timer) != 0 {
            return Err(io::Error::last_os_error());
        }
        let timer = OwnedTimer(timer);

        let mut spec: libc::itimerspec = mem::zeroed();
        spec.it_interval.tv_sec = interval.as_secs() as _;
        spec.it_interval.tv_nsec = interval.subsec_nanos() as _;
        spec.it_value.tv_sec = interval.as_secs() as _;
        spec.it_value.tv_nsec = interval.subsec_nanos() as _;

        if libc::timer_settime(timer.0, 0, &spec, std::ptr::null_mut()) != 0 {
            return Err(io::Error::last_os_error());
        }

        Ok(timer)
    }

    #[cfg(not(target_os = "linux"))]
    {
        let _ = (sig, tid, interval, sigev_value);
        Err(io::ErrorKind::Unsupported.into())
    }
}

/// Stop and delete a signal timer created by `setup_signal_timer`.
pub fn stop_signal_timer(timer: timer_t) -> io::Result<()> {
    #[cfg(target_os = "linux")]
    unsafe {
        if libc::timer_delete(timer) != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    #[cfg(not(target_os = "linux"))]
    {
        let _ = timer;
        Err(io::ErrorKind::Unsupported.into())
    }
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
