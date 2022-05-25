/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use configmodel::Config;
use configmodel::ConfigExt;

use crate::errors::DeprecatedFeature;

pub fn deprecate(
    config: &dyn Config,
    name: &str,
    error_str: &str,
) -> Result<(), DeprecatedFeature> {
    if config.get_or("deprecate", name, || false).unwrap_or(false) {
        Err(DeprecatedFeature(error_str.to_string()))
    } else {
        Ok(())
    }
}
