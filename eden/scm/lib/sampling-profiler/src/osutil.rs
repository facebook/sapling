/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Operation system features used by the profiler.

use std::io;
use std::mem;
#[cfg(target_os = "linux")]
use std::ptr;
#[cfg(all(unix, not(target_os = "linux")))]
use std::sync::Arc;
#[cfg(all(unix, not(target_os = "linux")))]
use std::sync::atomic::AtomicBool;
#[cfg(all(unix, not(target_os = "linux")))]
use std::sync::atomic::AtomicIsize;
#[cfg(all(unix, not(target_os = "linux")))]
use std::sync::atomic::Ordering;
#[cfg(all(unix, not(target_os = "linux")))]
use std::thread::JoinHandle;
use std::time::Duration;

use anyhow::Context;

/// Atomic payload for passing data from the timer thread to the signal handler
/// on non-Linux Unix, where `sigevent`/`si_value` is unavailable. The timer
/// thread CAS's from -1 to the payload, sends the signal, and the signal
/// handler reads and resets it to -1.
#[cfg(all(unix, not(target_os = "linux")))]
pub static SIGNAL_PAYLOAD: AtomicIsize = AtomicIsize::new(-1);

// Block `sig` signals. Explicitly opt-out profiling for the current thread
// and new threads spawned from the current thread.
pub fn block_signal(sig: libc::c_int) {
    sigmask_sigprof(sig, true);
}

/// Unblock `sig` to enable profiling.
pub fn unblock_signal(sig: libc::c_int) {
    sigmask_sigprof(sig, false);
}

/// Thread identifier type: Linux uses kernel tid, others use pthread_t.
#[cfg(target_os = "linux")]
pub type ThreadId = libc::pid_t;

#[cfg(all(unix, not(target_os = "linux")))]
pub type ThreadId = libc::pthread_t;

// Get the current thread id. Must be async-signal-safe.
#[cfg(target_os = "linux")]
pub fn get_thread_id() -> ThreadId {
    unsafe { libc::syscall(libc::SYS_gettid) as ThreadId }
}

#[cfg(all(unix, not(target_os = "linux")))]
pub fn get_thread_id() -> ThreadId {
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
///
/// On Linux the pipe is configured with:
/// - O_DIRECT: Enables "packet-mode". No need to deal with payload boundaries.
/// - A larger buffer to reduce chances data gets dropped.
///
/// On other Unix systems a regular blocking pipe is used.
///
/// Returns `[read_fd, write_fd]`.
pub fn setup_pipe() -> anyhow::Result<[OwnedFd; 2]> {
    #[cfg(target_os = "linux")]
    unsafe {
        let mut pipe_fds: [libc::c_int; 2] = [0; 2];

        if libc::pipe2(pipe_fds.as_mut_ptr(), libc::O_DIRECT) != 0 {
            return Err(io::Error::last_os_error()).context("pipe2(O_DIRECT)");
        }
        let (rfd, wfd) = (OwnedFd(pipe_fds[0]), OwnedFd(pipe_fds[1]));

        // The default pipe buffer is 4KB. It fits ~100 frames. Try to use a larger
        // buffer so the signal handler is less likely blocking.
        // Linux has a per-user pipe pages limit /proc/sys/fs/pipe-user-pages-soft
        // (and -hard). Try to not use too much. 16x the original size gives us
        // ~1.6k frames.
        // If this fails, that's okay too. It's just an optimization.
        let buffer_size = 65536;
        let _ret = libc::fcntl(pipe_fds[1], libc::F_SETPIPE_SZ, buffer_size);

        Ok([rfd, wfd])
    }

    #[cfg(all(unix, not(target_os = "linux")))]
    unsafe {
        let mut pipe_fds: [libc::c_int; 2] = [0; 2];
        if libc::pipe(pipe_fds.as_mut_ptr()) != 0 {
            return Err(io::Error::last_os_error()).context("pipe");
        }
        Ok([OwnedFd(pipe_fds[0]), OwnedFd(pipe_fds[1])])
    }

    #[cfg(not(unix))]
    anyhow::bail!("unsupported platform")
}

/// Setup the signal handler. This is POSIX-only.
pub fn setup_signal_handler(
    sig: libc::c_int,
    signal_handler: extern "C" fn(libc::c_int, *const libc::siginfo_t, *const libc::c_void),
) -> anyhow::Result<()> {
    unsafe {
        let mut sa: libc::sigaction = std::mem::zeroed();
        sa.sa_sigaction = signal_handler as usize;
        sa.sa_flags = libc::SA_RESTART | libc::SA_SIGINFO;
        libc::sigemptyset(&mut sa.sa_mask);
        libc::sigaddset(&mut sa.sa_mask, sig); // Prevents re-entrancy
        if libc::sigaction(sig, &sa, std::ptr::null_mut()) != 0 {
            return Err(io::Error::last_os_error()).context("sigaction");
        }
    }

    Ok(())
}

/// Represents an owned timer that can be stopped.
/// On Linux, uses POSIX timer_create with SIGEV_THREAD_ID.
#[cfg(target_os = "linux")]
pub struct OwnedTimer(libc::timer_t);

/// Represents an owned timer that can be stopped.
/// On non-Linux Unix, uses a pthread-based timer thread.
#[cfg(all(unix, not(target_os = "linux")))]
pub struct OwnedTimer {
    stop_flag: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl Drop for OwnedTimer {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(target_os = "linux")]
impl OwnedTimer {
    pub fn stop(&mut self) {
        if !self.0.is_null() {
            let _ = stop_signal_timer(self.0);
            self.0 = ptr::null_mut();
        }
    }
}

#[cfg(all(unix, not(target_os = "linux")))]
impl OwnedTimer {
    pub fn stop(&mut self) {
        self.stop_flag.store(true, Ordering::Release);
        if let Some(h) = self.handle.take() {
            h.thread().unpark();
            let _ = h.join();
        }
    }
}
/// Send `sig` to `tid` at the specified interval.
/// On Linux, uses kernel timer_create with SIGEV_THREAD_ID.
/// Returns the timer handle that can be used to stop the timer later.
#[cfg(target_os = "linux")]
pub fn setup_signal_timer(
    sig: libc::c_int,
    tid: ThreadId,
    interval: Duration,
    sigev_value: isize,
) -> anyhow::Result<OwnedTimer> {
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
            return Err(io::Error::last_os_error()).context("timer_create");
        }

        let mut spec: libc::itimerspec = mem::zeroed();
        spec.it_interval.tv_sec = interval.as_secs() as _;
        spec.it_interval.tv_nsec = interval.subsec_nanos() as _;
        spec.it_value.tv_sec = interval.as_secs() as _;
        spec.it_value.tv_nsec = interval.subsec_nanos() as _;

        if libc::timer_settime(timer, 0, &spec, std::ptr::null_mut()) != 0 {
            let err = io::Error::last_os_error();
            libc::timer_delete(timer);
            return Err(err).context("timer_settime");
        }

        Ok(OwnedTimer(timer))
    }
}

/// Setup a pthread-based timer that sends `sig` to `target_thread` at the specified interval.
/// On non-Linux Unix, uses pthread_kill since SIGEV_THREAD_ID is not available.
/// The `sigev_value` is passed to the signal handler via `SIGNAL_PAYLOAD` atomic:
/// the timer thread CAS's from -1 to `sigev_value`, then sends the signal.
/// The signal handler reads and resets `SIGNAL_PAYLOAD` to -1.
#[cfg(all(unix, not(target_os = "linux")))]
pub fn setup_signal_timer(
    sig: libc::c_int,
    target_thread: ThreadId,
    interval: Duration,
    sigev_value: isize,
) -> anyhow::Result<OwnedTimer> {
    anyhow::ensure!(sigev_value != -1, "sigev_value must not be -1 (sentinel)");

    let stop_flag = Arc::new(AtomicBool::new(false));

    let handle = std::thread::Builder::new()
        .name("profiler-timer-{target_thread:?}".into())
        .spawn({
            let stop_flag = stop_flag.clone();
            move || {
                // Block the profiling signal in the timer thread itself.
                block_signal(sig);
                loop {
                    std::thread::park_timeout(interval);
                    if stop_flag.load(Ordering::Acquire) {
                        break;
                    }
                    // Spin until the signal handler has consumed the previous payload.
                    let mut wait_count: u32 = 0;
                    loop {
                        match SIGNAL_PAYLOAD.compare_exchange(
                            -1,
                            sigev_value,
                            Ordering::AcqRel,
                            Ordering::Acquire,
                        ) {
                            Ok(_) => break,
                            Err(_) => {
                                if SIGNAL_PAYLOAD.load(Ordering::Acquire) == sigev_value {
                                    // Already have the desired value.
                                    break;
                                }
                                if stop_flag.load(Ordering::Acquire) {
                                    break;
                                }
                                // Is signal handling or delivery stuck? If so, avoid burning CPU.
                                if wait_count >= 0x10000 {
                                    std::thread::park_timeout(Duration::from_millis(16));
                                } else if wait_count >= 0x1000 {
                                    wait_count += 0x1000;
                                    std::thread::park_timeout(Duration::from_millis(1));
                                } else {
                                    wait_count += 1;
                                    std::hint::spin_loop();
                                }
                            }
                        }
                    }
                    if stop_flag.load(Ordering::Acquire) {
                        // Restore SIGNAL_PAYLOAD. Do not call signal handler.
                        // If this fails, the value might be restored by the signal handler.
                        let _ = SIGNAL_PAYLOAD.compare_exchange(
                            sigev_value,
                            -1,
                            Ordering::AcqRel,
                            Ordering::Acquire,
                        );
                        break;
                    }
                    unsafe {
                        libc::pthread_kill(target_thread, sig);
                    }
                }
                // Stopped. Clean up if the signal handler hasn't consumed our payload.
                let _ = SIGNAL_PAYLOAD.compare_exchange(
                    sigev_value,
                    -1,
                    Ordering::AcqRel,
                    Ordering::Relaxed,
                );
            }
        })?;

    Ok(OwnedTimer {
        stop_flag,
        handle: Some(handle),
    })
}

/// Stop and delete a signal timer created by `setup_signal_timer`.
#[cfg(target_os = "linux")]
fn stop_signal_timer(timer: libc::timer_t) -> anyhow::Result<()> {
    unsafe {
        if libc::timer_delete(timer) != 0 {
            return Err(io::Error::last_os_error()).context("timer_delete");
        }
        Ok(())
    }
}

/// Consume all pending instances of `sig` for the current thread.
/// The signal must be blocked before calling this function (see `block_signal`),
/// otherwise signals may be delivered to the handler instead of being drained.
pub fn drain_pending_signals(sig: libc::c_int) {
    unsafe {
        let mut set: libc::sigset_t = mem::zeroed();
        libc::sigemptyset(&mut set);
        libc::sigaddset(&mut set, sig);

        let mut pending: libc::sigset_t = mem::zeroed();
        while libc::sigpending(&mut pending) == 0 && libc::sigismember(&pending, sig) == 1 {
            let mut caught: libc::c_int = 0;
            libc::sigwait(&set, &mut caught);
        }
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

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use super::*;

    #[test]
    fn test_timer_drop_is_fast() {
        setup_signal_handler(libc::SIGUSR1, noop_handler).unwrap();

        let interval = Duration::from_secs(5);
        let start = Instant::now();
        let timer = setup_signal_timer(libc::SIGUSR1, get_thread_id(), interval, 42).unwrap();
        std::thread::sleep(Duration::from_millis(10));
        drop(timer);
        let elapsed = start.elapsed();

        assert!(
            elapsed < Duration::from_secs(2),
            "drop took {:?}, expected < 2s",
            elapsed,
        );

        extern "C" fn noop_handler(
            _: libc::c_int,
            _: *const libc::siginfo_t,
            _: *const libc::c_void,
        ) {
        }
    }
}
