/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::Path;

use crate::errors::ResultExt;

// Public interface -------------------------------------------------------

/// Repair a structure at the given path.
pub trait Repair<T> {
    /// Repair a structure at the given path.
    ///
    /// Overload this method to repair recursively.
    fn repair(path: impl AsRef<Path>) -> crate::Result<String>;
}

/// Repair on open.
///
/// Use with causion. See warnings in `open_with_repair`.
pub trait OpenWithRepair {
    type Output;

    /// Call `open`. If it fails with data corruption errors, try `repair`
    /// once, then `open` again.
    ///
    /// This conveniently fixes a subset of corruptions usually caused by OS
    /// crash or hard reboots. It does not fix corruptions that may occur during
    /// data reading after `open`.
    ///
    /// For performance reasons, this does not perform a full verification
    /// of all data and corruption can still happen when reading data.
    ///
    /// Warning: indexedlog requires append-only for lock-free reads.
    /// Repair is not append-only. It can silently cause other running
    /// processes reading the data, or keeping the data previously read
    /// to get silently wrong data without detection.
    fn open_with_repair(&self, path: impl AsRef<Path>) -> crate::Result<Self::Output>
    where
        Self: Sized;
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

/// Defines the output of OpenOptions.
pub trait OpenOptionsOutput {
    type Output;

    fn open_path(&self, path: &Path) -> crate::Result<Self::Output>;
}

impl<T: DefaultOpenOptions<O>, O: OpenOptionsRepair> Repair<O> for T {
    fn repair(path: impl AsRef<Path>) -> crate::Result<String> {
        T::default_open_options().open_options_repair(path.as_ref())
    }
}

pub(crate) fn open_with_repair<T>(opts: &T, path: &Path) -> crate::Result<T::Output>
where
    T: OpenOptionsOutput + OpenOptionsRepair,
{
    let res = opts.open_path(path);
    if let Err(e) = &res {
        if e.is_corruption() {
            // Repair and retry.
            let repair_message = opts
                .open_options_repair(path)
                .context(|| format!("in open_with_repair({:?}), attempt to repair", path))?;
            tracing::info!("Auto-repair {:?} Result:\n{}", path, &repair_message);
            return opts.open_path(path).context(|| {
                format!(
                    "in open_with_repair({:?}), after repair ({})",
                    path, repair_message
                )
            });
        }
    }
    res
}

impl<T> OpenWithRepair for T
where
    T: OpenOptionsOutput + OpenOptionsRepair,
{
    type Output = T::Output;

    fn open_with_repair(&self, path: impl AsRef<Path>) -> crate::Result<Self::Output>
    where
        Self: Sized,
    {
        let path = path.as_ref();
        open_with_repair(self, path)
    }
}
