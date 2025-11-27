/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

static ENFORCE_SQL_TIMEOUTS: AtomicBool = AtomicBool::new(true);

pub fn should_enforce_sql_timeouts() -> bool {
    ENFORCE_SQL_TIMEOUTS.load(Ordering::Relaxed)
}

pub fn disable_sql_timeouts() {
    ENFORCE_SQL_TIMEOUTS.store(false, Ordering::Relaxed);
}
