/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! `configloader` is about how to load config locally and remotely for a repo.
//!
//! It is heavyweight because of the remote config logic. There are lightweight
//! choices if you do not need logic to load configs, for example, if you can
//! just get a config from elsewhere.
//!
//! If you're looking for just reading configs, use `&dyn configmodel::Config`.
//! If you're looking for reading configs and some extra features like setting
//! configs, use `configset::ConfigSet`.

pub mod hg;

pub use configmodel;
pub use configmodel::convert;
pub use configmodel::error;
pub use configmodel::Config;
pub use configmodel::Error;
pub use configmodel::Result;
pub use configset::config;
pub use error::Errors;
// Re-export
pub use minibytes::Text;

#[cfg(feature = "fb")]
mod fb;

mod builtin_static;

#[cfg(test)]
mod test_util;

#[cfg(test)]
use lazy_static::lazy_static;
#[cfg(test)]
use parking_lot::Mutex;

#[cfg(test)]
lazy_static! {
    static ref ENV_LOCK: Mutex<()> = Mutex::new(());
}

#[cfg(test)]
/// Lock the environment and return an object that allows setting env
/// vars, undoing env changes when the object goes out of scope. This
/// should be used by tests that rely on particular environment
/// variable values that might be overwritten by other tests.
pub(crate) fn lock_env() -> test_util::EnvLock<'static> {
    test_util::EnvLock::new(ENV_LOCK.lock())
}
