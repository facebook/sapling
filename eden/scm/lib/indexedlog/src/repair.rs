/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::Path;

// Public interface -------------------------------------------------------

/// Repair a structure at the given path.
pub trait Repair<T> {
    /// Repair a structure at the given path.
    ///
    /// Overload this method to repair recursively.
    fn repair(path: impl AsRef<Path>) -> crate::Result<String>;
}

/// A structure with a static [`OpenOptions`].
///
/// Structures implementing this trait with `T` being `log::OpenOptions`
/// or `rotate::OpenOptions` gets `Repair` implemented automatically.
pub trait DefaultOpenOptions<T> {
    fn default_open_options() -> T;
}

// Private implementations ------------------------------------------------

/// Repair defined on an instance. For example, `OpenOptions`.
pub trait OpenOptionsRepair {
    fn open_options_repair(&self, path: impl AsRef<Path>) -> crate::Result<String>;
}

impl<T: DefaultOpenOptions<O>, O: OpenOptionsRepair> Repair<O> for T {
    fn repair(path: impl AsRef<Path>) -> crate::Result<String> {
        T::default_open_options().open_options_repair(path.as_ref())
    }
}
