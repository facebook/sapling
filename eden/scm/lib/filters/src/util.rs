/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::HashSet;

use configmodel::Config;
use configmodel::Text;

pub fn filter_paths_from_config(config: &mut impl Config) -> Option<Vec<Text>> {
    // Get unique set of filter paths
    let filter_paths = config
        .keys("clone")
        .iter()
        .filter_map(|k| {
            if k.starts_with("eden-sparse-filter") {
                tracing::debug!("found filter config key: {}", k);
                config.get("clone", k)
            } else {
                None
            }
        })
        .collect::<HashSet<_>>();
    if filter_paths.is_empty() {
        None
    } else if filter_paths.len() > 1 {
        // If more than 1 filter path is supplied, remove "" as that represents the null filter and
        // cannot be combined with other filters
        Some(
            filter_paths
                .into_iter()
                .filter(|e| !e.as_ref().is_empty())
                .collect::<Vec<_>>(),
        )
    } else {
        Some(filter_paths.into_iter().collect())
    }
}
