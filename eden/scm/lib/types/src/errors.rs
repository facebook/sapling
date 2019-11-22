/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use thiserror::Error;

#[derive(Debug, Error)]
#[error("Key Error: {0:?}")]
pub struct KeyError(#[source] Error);

impl KeyError {
    pub fn new(err: Error) -> Self {
        KeyError(err)
    }
}
