/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Convenient env_logger for testing purpose.
//!
//! # Example
//!
//! ```
//! // In lib.rs:
//! #[cfg(test)]
//! dev_logger::init!();
//!
//! // In test function:
//! tracing::info!(name = "message");
//!
//! // Set RUST_LOG=info and run the test.
//! ```

pub use ctor::ctor;

/// Initialize tracing and env_logger for adhoc logging (ex. in a library test)
/// purpose.
pub fn init() {
    tracing_collector::init();
    let _ = env_logger::try_init();
}

/// Call `init` on startup. This is useful for tests.
#[macro_export]
macro_rules! init {
    () => {
        #[dev_logger::ctor]
        fn dev_logger_init_ctor() {
            dev_logger::init();
        }
    };
}
