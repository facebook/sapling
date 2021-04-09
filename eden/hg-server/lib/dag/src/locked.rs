/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::ops::Persist;
use crate::Result;
use std::ops::Deref;
use std::ops::DerefMut;

/// Guard to make sure `T` on-disk writes are race-free.
pub struct Locked<'a, T: Persist> {
    pub inner: &'a mut T,
    pub lock: <T as Persist>::Lock,
}

impl<T: Persist> Locked<'_, T> {
    /// Write pending changes to disk. Release the exclusive lock.
    pub fn sync(self) -> Result<()> {
        self.inner.persist(&self.lock)?;
        Ok(())
    }
}

impl<T: Persist> Deref for Locked<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<T: Persist> DerefMut for Locked<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}
