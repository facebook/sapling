/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Logic to run at exit (`std::process:exit`).
//! Intended to be used as an alternative to Python's
//! `except KeyboardInterrupt`.

use std::borrow::Cow;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::Weak;

use once_cell::sync::Lazy;

/// Call `drop` on drop if `ignored` is `false`.
pub struct AtExit {
    drop: Option<Box<dyn FnOnce() + Send + Sync>>,
    name: Cow<'static, str>,
    ignored: AtomicBool,
}

/// Reference to `AtExit`. Can be used to cancel it.
/// Dropping `AtExitRef` does not call `drop`.
pub struct AtExitRef {
    // Private to prevent Arc::upgrade.
    inner: Weak<AtExit>,
}

/// Central place for global `AtExit`s.
/// Use `drop_queued` to drop them.
static AT_EXIT_QUEUED: Lazy<Mutex<Vec<Arc<AtExit>>>> = Lazy::new(Default::default);

impl Drop for AtExit {
    fn drop(&mut self) {
        let mut drop = None;
        std::mem::swap(&mut drop, &mut self.drop);
        if let Some(func) = drop {
            if !self.ignored.load(Ordering::Acquire) {
                tracing::debug!("running AtExit handler: {}", self.name);
                func();
            } else {
                tracing::debug!("skipping AtExit handler: {}", self.name);
            }
        }
    }
}

impl AtExit {
    /// Create `AtExit` that calls `drop` on drop.
    ///
    /// The `AtExit` is intended to be a (Rust) stack variable that gets dropped
    /// when exiting the (Rust) function. `exit()` will unroll stacks so `drop`
    /// will be called if another thread calls `exit()`.
    ///
    /// If you don't want the drop behavior on (Rust) function return, or have
    /// to store the `AtExit` in heap, consider using `queued()`. For example,
    /// in a CPython function, the Python objects that wraps the `AtExit` are
    /// not on (Rust) stack and won't be cleaned up on `exit()`. So for Python
    /// logic `queued()` should probably be always used.
    pub fn new(drop: Box<dyn FnOnce() + Send + Sync>) -> Self {
        Self {
            drop: Some(drop),
            ignored: AtomicBool::new(false),
            name: "unnamed".into(),
        }
    }

    /// Assign a name to the `AtExit` handler.
    pub fn named(mut self, name: Cow<'static, str>) -> Self {
        self.name = name;
        self
    }

    /// Move the `AtExit` to a global queue.
    ///
    /// Return `AtExitRef`, which can be used to cancel the `drop`.
    /// Dropping `AtExitRef` wouldn't trigger `drop`.
    ///
    /// The global queue can be dropped by `drop_queued`.
    pub fn queued(self) -> AtExitRef {
        let arc = Arc::new(self);
        let weak = Arc::downgrade(&arc);
        let mut queue = AT_EXIT_QUEUED.lock().unwrap();
        queue.push(arc);
        AtExitRef { inner: weak }
    }

    /// Skip calling `drop` on drop.
    pub fn cancel(&self) {
        self.ignored.store(true, Ordering::Release);
    }
}

impl AtExitRef {
    /// Skip calling `drop` on drop.
    pub fn cancel(&self) {
        if let Some(arc) = self.inner.upgrade() {
            arc.cancel();
        }
    }
}

/// Drop `AtExit`s that are previously `queued`.
/// This is usually called at the end of a program.
pub fn drop_queued() {
    if let Ok(mut lock) = AT_EXIT_QUEUED.lock() {
        tracing::debug!("running {} AtExit handlers by drop_queued()", lock.len());
        let mut to_drop: Vec<_> = lock.drain(..).collect();
        // Unlock first so drop(to_drop) can call `drop_queued`
        // without deadlock.
        drop(lock);
        // Drop in reverse push order (first push last drop)
        // as if it is a stack.
        to_drop.drain(..).rev().for_each(drop);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bool_atexit() -> (Arc<AtomicBool>, AtExit) {
        let v = Arc::new(AtomicBool::new(false));
        let a = {
            let v = v.clone();
            AtExit::new(Box::new(move || v.store(true, Ordering::Release)))
        };
        (v, a)
    }

    #[test]
    fn test_drop() {
        let (v, a) = bool_atexit();
        drop(a);
        assert!(v.load(Ordering::Acquire));
    }

    #[test]
    fn test_cancel() {
        let (v, a) = bool_atexit();
        a.cancel();
        drop(a);
        assert!(!v.load(Ordering::Acquire));
    }

    #[test]
    fn test_queued() {
        let (v1, a1) = bool_atexit();
        let (v2, a2) = bool_atexit();
        let r1 = a1.queued();
        let r2 = a2.queued();
        r1.cancel();
        assert!(!v1.load(Ordering::Acquire));
        assert!(!v2.load(Ordering::Acquire));
        drop_queued();
        assert!(!v1.load(Ordering::Acquire));
        assert!(v2.load(Ordering::Acquire));
        drop(r2);

        // Does not deadlock if drop_queued is called by AtExit
        // inside drop_queued.
        let r3 = AtExit::new(Box::new(drop_queued));
        let _r3 = r3.queued();
        drop_queued();
    }

    #[test]
    fn test_queued_drop_order() {
        let drop_order = Arc::new(Mutex::new(Vec::new()));
        let push_atexit = |value: u8| -> AtExit {
            let drop_order = drop_order.clone();
            AtExit::new(Box::new(move || {
                drop_order.lock().unwrap().push(value);
            }))
        };

        let a1 = push_atexit(1);
        let a2 = push_atexit(2);
        let a3 = push_atexit(3);
        a1.queued();
        a3.queued();
        a2.queued();

        drop_queued();
        assert_eq!(drop_order.lock().unwrap().clone(), [2, 3, 1]);
    }
}
