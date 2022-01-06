/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use fbinit::FacebookInit;

pub struct ScribeClientImplementation {}

impl ScribeClientImplementation {
    pub fn new(_fb: FacebookInit) -> Self {
        Self {}
    }

    pub fn offer(&self, _category: &str, _sample: &str) -> Result<()> {
        Ok(())
    }
}
