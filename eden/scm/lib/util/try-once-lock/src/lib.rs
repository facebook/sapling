/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::fmt;
use std::ops::Deref;
use std::ops::DerefMut;
use std::sync;

/// Similar to [`std::sync::OnceLock`], but `get_or_try_init` does not require
/// nightly rustc.
pub struct OnceLock<T> {
    cell: sync::OnceLock<T>,
    init_lock: sync::Mutex<()>,
}

impl<T> OnceLock<T> {
    pub const fn new() -> Self {
        Self {
            cell: sync::OnceLock::new(),
            init_lock: sync::Mutex::new(()),
        }
    }

    pub fn get_or_try_init<F, E>(&self, init: F) -> Result<&T, E>
    where
        F: FnOnce() -> Result<T, E>,
    {
        if let Some(value) = self.cell.get() {
            return Ok(value);
        }

        let _guard = self
            .init_lock
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if let Some(value) = self.cell.get() {
            return Ok(value);
        }

        let value = init()?;
        let _ = self.cell.set(value);
        Ok(self.cell.get().expect("value was just initialized"))
    }
}

impl<T> Deref for OnceLock<T> {
    type Target = sync::OnceLock<T>;

    fn deref(&self) -> &Self::Target {
        &self.cell
    }
}

impl<T> DerefMut for OnceLock<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.cell
    }
}

impl<T> Default for OnceLock<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> From<T> for OnceLock<T> {
    fn from(value: T) -> Self {
        Self {
            cell: value.into(),
            init_lock: sync::Mutex::new(()),
        }
    }
}

impl<T: Clone> Clone for OnceLock<T> {
    fn clone(&self) -> Self {
        Self {
            cell: self.cell.clone(),
            init_lock: sync::Mutex::new(()),
        }
    }
}

impl<T: fmt::Debug> fmt::Debug for OnceLock<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.cell.fmt(formatter)
    }
}
