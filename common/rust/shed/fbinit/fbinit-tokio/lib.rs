/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is dual-licensed under either the MIT license found in the
 * LICENSE-MIT file in the root directory of this source tree or the Apache
 * License, Version 2.0 found in the LICENSE-APACHE file in the root directory
 * of this source tree. You may select, at your option, one of the
 * above-listed licenses.
 */

use futures::Future;

#[cfg(all(fbcode_build, target_os = "linux"))]
mod facebook;

pub fn tokio_test<F>(tokio_workers: Option<usize>, f: F) -> <F as Future>::Output
where
    F: Future,
{
    let mut builder = if let Some(workers) = tokio_workers {
        let mut builder = tokio::runtime::Builder::new_multi_thread();
        builder.worker_threads(workers);
        builder
    } else {
        tokio::runtime::Builder::new_current_thread()
    };
    builder.enable_all().build().unwrap().block_on(f)
}

pub fn tokio_main<F>(tokio_workers: Option<usize>, f: F) -> <F as Future>::Output
where
    F: Future,
{
    tokio_main_impl(tokio_workers, None, f)
}

/// Runs a future on the standard fbinit Tokio runtime with the requested worker
/// and blocking-thread stack size.
pub fn tokio_main_with_thread_stack_size<F>(
    tokio_workers: Option<usize>,
    thread_stack_size: usize,
    f: F,
) -> <F as Future>::Output
where
    F: Future,
{
    tokio_main_impl(tokio_workers, Some(thread_stack_size), f)
}

fn tokio_main_impl<F>(
    tokio_workers: Option<usize>,
    thread_stack_size: Option<usize>,
    f: F,
) -> <F as Future>::Output
where
    F: Future,
{
    let mut runtime = tokio::runtime::Builder::new_multi_thread();
    if let Some(tokio_workers) = tokio_workers {
        runtime.worker_threads(tokio_workers);
    }
    if let Some(thread_stack_size) = thread_stack_size {
        runtime.thread_stack_size(thread_stack_size);
    }
    #[cfg(all(fbcode_build, target_os = "linux"))]
    facebook::maybe_install_request_context_hooks(&mut runtime);
    runtime.enable_all().build().unwrap().block_on(f)
}
