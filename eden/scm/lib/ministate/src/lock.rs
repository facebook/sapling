/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::sync::OnceLock;

use parking_lot::RawRwLock;
use parking_lot::lock_api::GuardNoSend;
use parking_lot::lock_api::RawRwLock as RawRwLockTrait;

pub struct WrappedRwLock {
    inner: RawRwLock,
    pub(crate) on_unlock_exclusive: OnceLock<Box<dyn Fn() + Send + Sync + 'static>>,
}

unsafe impl RawRwLockTrait for WrappedRwLock {
    // https://rust-lang.github.io/rust-clippy/master/index.html#declare_interior_mutable_const
    // > Consts are copied everywhere they are referenced, i.e., every time you refer to the
    // > const a fresh instance of the Cell or Mutex or AtomicXxxx will be created, which defeats
    // > the whole purpose of using these types in the first place.
    //
    // We actually want this behavior. Every time `INIT` should give us a fresh new lock that
    // has an empty `on_unlock_exclusive`.
    #[allow(clippy::declare_interior_mutable_const)]
    const INIT: Self = Self {
        inner: RawRwLock::INIT,
        on_unlock_exclusive: OnceLock::new(),
    };

    type GuardMarker = GuardNoSend;

    fn lock_shared(&self) {
        self.inner.lock_shared()
    }

    fn try_lock_shared(&self) -> bool {
        self.inner.try_lock_shared()
    }

    unsafe fn unlock_shared(&self) {
        unsafe { self.inner.unlock_shared() }
    }

    fn lock_exclusive(&self) {
        self.inner.lock_exclusive()
    }

    fn try_lock_exclusive(&self) -> bool {
        self.inner.try_lock_exclusive()
    }

    unsafe fn unlock_exclusive(&self) {
        unsafe {
            self.inner.unlock_exclusive();
            if let Some(f) = self.on_unlock_exclusive.get() {
                f()
            }
        }
    }
}

/// Wrapped `RwLock` that does something extra on unlock_exclusive.
pub type RwLock<T> = parking_lot::lock_api::RwLock<WrappedRwLock, T>;
