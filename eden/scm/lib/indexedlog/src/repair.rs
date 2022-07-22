/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::fs;
use std::io::Write;
use std::ops::AddAssign;
use std::path::Path;

use crate::errors::ResultExt;
use crate::lock::DirLockOptions;
use crate::lock::ScopedDirLock;
use crate::lock::READER_LOCK_OPTS;

// Public interface -------------------------------------------------------

/// Repair a structure at the given path.
pub trait Repair<T> {
    /// Repair a structure at the given path.
    ///
    /// Overload this method to repair recursively.
    fn repair(path: impl AsRef<Path>) -> crate::Result<String>;
}

/// Repair on open.
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
    /// Repair is skipped if there are other readers for safety. This is
    /// because indexedlog requires append-only for lock-free reads.
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

/// Repair message as a string.
/// Also write the message to other places (ex. a file, best-effort).
pub(crate) struct RepairMessage {
    output: String,
    additional_outputs: Vec<Box<dyn Write>>,
}

impl RepairMessage {
    /// Creates the `RepairMessage`. Attempt to write to `repair.log`
    /// in `dir`, but unable to doing so is not fatal.
    pub(crate) fn new(dir: &Path) -> Self {
        let mut additional_outputs = Vec::new();

        // Truncate the file if it's too large (ex. when repair is run
        // in a loop).
        let path = dir.join("repair.log");
        let mut need_truncate = false;
        if let Ok(meta) = fs::metadata(&path) {
            const REPAIR_LOG_SIZE_LIMIT: u64 = 1 << 20;
            if meta.len() > REPAIR_LOG_SIZE_LIMIT {
                need_truncate = true;
            }
        }

        let mut opts = fs::OpenOptions::new();
        opts.write(true).create(true);
        if !need_truncate {
            opts.append(true);
        }

        if let Ok(mut file) = opts.open(path) {
            if need_truncate {
                let _ = file.write_all(b"# This file was truncated\n\n");
            }
            if let Ok(duration) = std::time::UNIX_EPOCH.elapsed() {
                let msg = format!("date -d @{}\n", duration.as_secs());
                let _ = file.write_all(msg.as_bytes());
            }
            additional_outputs.push(Box::new(file) as Box<dyn Write>);
        }
        Self {
            output: String::new(),
            additional_outputs,
        }
    }

    pub(crate) fn as_str(&self) -> &str {
        self.output.as_str()
    }

    pub(crate) fn into_string(mut self) -> String {
        for out in self.additional_outputs.iter_mut() {
            let _ = out.write_all(b"\n");
            let _ = out.flush();
        }
        self.output
    }
}

impl AddAssign<&str> for RepairMessage {
    fn add_assign(&mut self, rhs: &str) {
        self.output += rhs;
        for out in self.additional_outputs.iter_mut() {
            let _ = out.write_all(rhs.as_bytes());
        }
    }
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
    match opts.open_path(path) {
        Ok(v) => Ok(v),
        Err(e) if e.is_corruption() => {
            // Check if it's safe to repair (no active readers).
            static CHECK_READER_LOCK_OPTS: DirLockOptions = DirLockOptions {
                exclusive: true,
                non_blocking: true,
                ..READER_LOCK_OPTS
            };

            let mut msg = RepairMessage::new(path);
            msg += &format!("Corruption detected: {:?}.\n", &e);

            let lock = match ScopedDirLock::new_with_options(path, &CHECK_READER_LOCK_OPTS) {
                Ok(lock) => lock,
                Err(lock_err) => {
                    msg += &"Auto-repair is skipped due to active readers.\n";
                    let _ = msg.into_string();
                    return Err(e.source(lock_err))
                        .context(|| format!("in open_with_repair({:?})", path))
                        .context("repair is skipped due to active readers");
                }
            };

            // Release the lock. It prevents open, optionally used by repair.
            // Without this it will deadlock.
            // We don't need to prevent others from obtaining the reader lock
            // to `open` because the `open` will fail anyway.
            drop(lock);

            msg += &"Starting auto repair.\n";
            let _ = msg.into_string();

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
        Err(e) => Err(e),
    }
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
