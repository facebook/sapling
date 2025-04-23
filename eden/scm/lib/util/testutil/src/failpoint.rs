/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering::SeqCst;

use fail::FailScenario;

static FAIL_SETUP: AtomicBool = AtomicBool::new(false);

/// Install fail points based on $FAILPOINTS env var.
/// Fail points are removed when return value goes out of scope.
pub fn setup_fail_points<'a>() -> Option<FailScenario<'a>> {
    if std::env::var("FAILPOINTS").is_err() {
        // No need to setup failpoints.
        return None;
    }
    if FAIL_SETUP.fetch_or(true, SeqCst) {
        // Already setup.
        None
    } else {
        Some(FailScenario::setup())
    }
}

/// Install fail points based on $FAILPOINTS env var.
/// Prefer setup_fail_points() where possible.
pub fn setup_global_fail_points() {
    if let Ok(val) = std::env::var("FAILPOINTS") {
        for kv in val.split(';') {
            if let Some((k, v)) = kv.split_once('=') {
                fail::cfg(k, v).unwrap();
            }
        }
    }
}

pub fn teardown_fail_points<'a>(scenario: FailScenario<'a>) {
    scenario.teardown();
    FAIL_SETUP.store(false, SeqCst);
}
