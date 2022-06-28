/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use abomonation_derive::Abomonation;
use anyhow::Error;

use filenodes::FilenodeInfo;
use filenodes::FilenodeInfoCached;

#[derive(Abomonation, Clone)]
pub struct FilenodeHistoryCached {
    // TODO: We could store this more efficiently by deduplicating filenode IDs.
    history: Vec<FilenodeInfoCached>,
}

impl FilenodeHistoryCached {
    pub fn into_filenode_info(self) -> Result<Vec<FilenodeInfo>, Error> {
        self.history.into_iter().map(|c| c.try_into()).collect()
    }

    pub fn from_filenodes(filenodes: Vec<FilenodeInfo>) -> Self {
        let history = filenodes.into_iter().map(|f| f.into()).collect::<Vec<_>>();
        Self { history }
    }
}
