/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;

mod commit_sync;

/// Trait for converting raw config into parsed config.
pub(crate) trait Convert {
    type Output;

    fn convert(self) -> Result<Self::Output>;
}
