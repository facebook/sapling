/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

pub(crate) const BATCH_SIZE: AtomicUsize = AtomicUsize::new(1024);

/**
 * Set the "batch size".
 * It's used by location -> hash API used during iteration.
 */
pub fn set_batch_size(value: usize) {
    BATCH_SIZE.store(value.max(1), Ordering::Release);
}
