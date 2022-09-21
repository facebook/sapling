/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::env;

pub const HGPLAIN: &str = "HGPLAIN";
pub const HGPLAINEXCEPT: &str = "HGPLAINEXCEPT";

/// Return whether plain mode is active, similar to python ui.plain().
pub fn is_plain(feature: Option<&str>) -> bool {
    let plain = env::var(HGPLAIN);
    let plain_except = env::var(HGPLAINEXCEPT);

    if plain.is_err() && plain_except.is_err() {
        return false;
    }

    if let Some(feature) = feature {
        !plain_except
            .unwrap_or_default()
            .split(',')
            .any(|s| s == feature)
    } else {
        true
    }
}
