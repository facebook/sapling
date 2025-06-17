/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under both the MIT license found in the
 * LICENSE-MIT file in the root directory of this source tree and the Apache
 * License, Version 2.0 found in the LICENSE-APACHE file in the root directory
 * of this source tree.
 */

use futures::Future;

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
    let mut runtime = tokio::runtime::Builder::new_multi_thread();
    if let Some(tokio_workers) = tokio_workers {
        runtime.worker_threads(tokio_workers);
    }
    runtime.enable_all().build().unwrap().block_on(f)
}
