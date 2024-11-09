/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use configmodel::Config;
use configmodel::ConfigExt;

use crate::errors::DeprecatedFeature;

pub fn deprecate(
    config: &dyn Config,
    name: &str,
    error_str: String,
) -> Result<(), DeprecatedFeature> {
    if config.get_or("deprecate", name, || false).unwrap_or(false) {
        Err(DeprecatedFeature(error_str))
    } else {
        Ok(())
    }
}
