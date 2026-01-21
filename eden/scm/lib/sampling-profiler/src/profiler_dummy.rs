/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::marker::PhantomData;
use std::time::Duration;

use anyhow::bail;

use crate::ResolvedBacktraceProcessFunc;

/// Dummy profiler that does nothing.
pub struct Profiler {
    _marker: PhantomData<*const ()>,
}

impl Profiler {
    pub fn new(
        _interval: Duration,
        _backtrace_process_func: ResolvedBacktraceProcessFunc,
    ) -> anyhow::Result<Self> {
        bail!("not supported");
    }
}
