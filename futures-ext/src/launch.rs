/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use futures::Future;
use std::sync::{Arc, Mutex};

/// Starts the Tokio runtime using the supplied future to bootstrap the
/// execution.
///
/// # Similar APIs
///
/// This function is equivalent to `tokio::run` except that it also returns the
/// future's resolved value as a Result. Thus it requires `F::Item: Send` and
/// `F::Error: Send`.
///
/// This function has the same signature as `Future::wait` which also goes from
/// `F: Future` -> `Result<F::Item, F::Error>`, but `wait` requires an ambient
/// futures runtime to already exist.
///
/// # Details
///
/// This function does the following:
///
///   - Start the Tokio runtime using a default configuration.
///   - Spawn the given future onto the thread pool.
///   - Block the current thread until the runtime shuts down.
///   - Send ownership of the future's resolved value back to the caller's
///     thread.
///
/// Note that the function will not return immediately once `future` has
/// completed. Instead it waits for the entire runtime to become idle.
///
/// # Panics
///
/// This function panics if called from the context of an executor.
pub fn top_level_launch<F>(future: F) -> Result<F::Item, F::Error>
where
    F: Future + Send + 'static,
    F::Item: Send,
    F::Error: Send,
{
    let result = Arc::new(Mutex::new(None));
    let result_handle = Arc::clone(&result);

    tokio::run(future.then(move |result| {
        *result_handle
            .lock()
            .expect("this mutex can never be poisoned") = Some(result);
        Ok(())
    }));

    let mut result = result.lock().expect("this mutex can never be poisoned");
    result
        .take()
        .expect("tokio runtime terminated without resolving future")
}
