/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::cell::Cell;
use std::cell::RefCell;
use std::fmt::Write;

#[cxx::bridge(namespace = "facebook::eden")]
mod ffi {
    extern "Rust" {
        /// Capture a backtrace (frames only, no symbolization).
        /// Stores the result in thread-local storage.
        fn capture_backtrace(max_depth: usize);

        /// Check if a backtrace has been captured in the current thread.
        fn has_captured_trace() -> bool;

        /// Symbolize the captured backtrace and return a formatted string.
        /// Strips infrastructure frames above the throw hook so the trace
        /// starts at the actual throw site.
        /// Clears the stored trace after symbolization.
        fn symbolize_captured_trace() -> String;

        /// Clear any stored backtrace without symbolizing.
        fn clear_captured_trace();
    }
}

thread_local! {
    static CAPTURED: RefCell<Option<backtrace::Backtrace>> = const { RefCell::new(None) };
    static CAPTURING: Cell<bool> = const { Cell::new(false) };
}

struct CapturingGuard;

impl CapturingGuard {
    fn new() -> Option<Self> {
        CAPTURING.with(|c| {
            if c.get() {
                return None;
            }
            c.set(true);
            Some(CapturingGuard)
        })
    }
}

impl Drop for CapturingGuard {
    fn drop(&mut self) {
        CAPTURING.with(|c| c.set(false));
    }
}

fn capture_backtrace(max_depth: usize) {
    let Some(_guard) = CapturingGuard::new() else {
        return;
    };
    let mut frames = Vec::with_capacity(max_depth);
    backtrace::trace(|frame| {
        if frames.len() >= max_depth {
            return false;
        }
        frames.push(backtrace::BacktraceFrame::from(frame.clone()));
        true
    });
    let bt = backtrace::Backtrace::from(frames);
    CAPTURED.with(|slot| *slot.borrow_mut() = Some(bt));
}

fn has_captured_trace() -> bool {
    CAPTURED.with(|slot| slot.borrow().is_some())
}

fn clear_captured_trace() {
    CAPTURED.with(|slot| *slot.borrow_mut() = None);
}

// Each platform uses a different function to throw C++ exceptions.
// We scan for these names to find the throw site in the backtrace
// and strip all infrastructure frames above it.
// Windows uses CxxThrowException,
// Linux uses __wrap___cxa_throw (via --wrap linker flag),
// macOS overrides __cxa_throw directly (via dlsym RTLD_NEXT).
fn is_throw_hook(name: &[u8]) -> bool {
    let s = match std::str::from_utf8(name) {
        Ok(s) => s,
        Err(_) => return false,
    };
    if cfg!(target_os = "windows") {
        s.contains("CxxThrowException")
    } else {
        s.contains("__cxa_throw")
    }
}

// Symbolize captured frames, strip infrastructure frames above the throw
// hook, format remaining frames, and clear the stored trace.
fn symbolize_captured_trace() -> String {
    let Some(mut bt) = CAPTURED.with(|slot| slot.borrow_mut().take()) else {
        return String::new();
    };

    bt.resolve();
    let frames = bt.frames();

    let mut hook_idx = None;
    'outer: for (i, frame) in frames.iter().enumerate() {
        for symbol in frame.symbols() {
            if let Some(name) = symbol.name() {
                if is_throw_hook(name.as_bytes()) {
                    hook_idx = Some(i);
                    break 'outer;
                }
            }
        }
    }

    let start = hook_idx.map_or(0, |i| i + 1);
    let mut result = String::new();
    for (i, frame) in frames[start..].iter().enumerate() {
        for symbol in frame.symbols() {
            let name = symbol.name();
            let file = symbol.filename().map(|f| f.to_string_lossy());
            let line = symbol.lineno();
            match (name, file, line) {
                (Some(n), Some(f), Some(l)) => {
                    let _ = writeln!(result, "#{i} {n} at {f}:{l}");
                }
                (Some(n), Some(f), None) => {
                    let _ = writeln!(result, "#{i} {n} at {f}");
                }
                (Some(n), None, _) => {
                    let _ = writeln!(result, "#{i} {n}");
                }
                (None, _, _) => {
                    let _ = writeln!(result, "#{i} <unknown>");
                }
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicBool;
    use std::sync::atomic::Ordering;

    use super::*;

    #[test]
    fn test_is_throw_hook_matching() {
        if cfg!(target_os = "windows") {
            assert!(is_throw_hook(b"CxxThrowException"));
            assert!(!is_throw_hook(b"__cxa_throw"));
        } else {
            assert!(is_throw_hook(b"__cxa_throw"));
            assert!(is_throw_hook(b"__wrap___cxa_throw"));
            assert!(!is_throw_hook(b"CxxThrowException"));
        }
    }

    #[test]
    fn test_is_throw_hook_non_matching() {
        assert!(!is_throw_hook(b"std::runtime_error"));
        assert!(!is_throw_hook(b"main"));
        assert!(!is_throw_hook(b""));
    }

    #[test]
    fn test_is_throw_hook_invalid_utf8() {
        assert!(!is_throw_hook(&[0xff, 0xfe]));
    }

    #[test]
    fn test_capture_max_depth_zero() {
        capture_backtrace(0);
        assert!(has_captured_trace());
        let trace = symbolize_captured_trace();
        assert!(trace.is_empty());
    }

    // A C++ exception thrown while a thread runs its TLS destructors reaches
    // the throw hook after CAPTURED is gone.
    #[test]
    fn test_capture_after_tls_destruction() {
        static PANICKED: AtomicBool = AtomicBool::new(false);
        struct CaptureOnDrop;
        impl Drop for CaptureOnDrop {
            fn drop(&mut self) {
                let result = std::panic::catch_unwind(|| {
                    capture_backtrace(8);
                    let _ = has_captured_trace();
                    let _ = symbolize_captured_trace();
                    clear_captured_trace();
                });
                PANICKED.store(result.is_err(), Ordering::SeqCst);
            }
        }
        thread_local! {
            static ON_DROP: RefCell<Option<CaptureOnDrop>> = const { RefCell::new(None) };
        }
        std::thread::spawn(|| {
            // TLS destructors run in reverse registration order: register
            // ON_DROP first so CAPTURED is destroyed before it runs.
            ON_DROP.with(|slot| *slot.borrow_mut() = Some(CaptureOnDrop));
            capture_backtrace(8);
            assert!(has_captured_trace());
        })
        .join()
        .expect("thread should exit cleanly");
        // FIXME: the capture must not panic once CAPTURED is gone. In the
        // daemon this panic is raised inside a cxx bridge function and aborts
        // the process.
        assert!(PANICKED.load(Ordering::SeqCst));
    }
}
