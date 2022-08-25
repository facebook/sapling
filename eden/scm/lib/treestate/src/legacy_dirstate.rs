/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::path::Path;

use anyhow::Result;

use crate::filestate::FileStateV2;
use crate::metadata::Metadata;

pub fn read_dirstate(_dirstate_path: &Path) -> Result<(Metadata, HashMap<Box<[u8]>, FileStateV2>)> {
    todo!();
}

pub fn write_dirstate(
    _dirstate_path: &Path,
    _metadata: Metadata,
    _entries: HashMap<Box<[u8]>, FileStateV2>,
) -> Result<()> {
    todo!();
}
