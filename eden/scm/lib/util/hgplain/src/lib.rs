/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::HashSet;

/// Return whether plain mode is active, similar to python ui.plain().
pub fn is_plain(feature: Option<&str>) -> bool {
    let plain = identity::try_env_var("PLAIN");
    let plain_except = identity::try_env_var("PLAINEXCEPT");

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

pub fn exceptions() -> HashSet<String> {
    match identity::try_env_var("PLAINEXCEPT") {
        Ok(value) => value.split(',').map(|s| s.to_string()).collect(),
        Err(_) => HashSet::new(),
    }
}
