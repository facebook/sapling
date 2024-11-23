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

pub fn teardown_fail_points<'a>(scenario: FailScenario<'a>) {
    scenario.teardown();
    FAIL_SETUP.store(false, SeqCst);
}
