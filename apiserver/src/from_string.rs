// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

// This file should only contain functions that accept a String and returns an internal type

use std::convert::TryFrom;
use std::str::FromStr;

use failure::{Result, ResultExt};

use mercurial_types::HgChangesetId;
use mononoke_types::MPath;

use errors::ErrorKind;

pub fn get_mpath(path: String) -> Result<MPath> {
    MPath::try_from(&*path)
        .with_context(|_| ErrorKind::InvalidInput(path))
        .map_err(From::from)
}

pub fn get_changeset_id(changesetid: String) -> Result<HgChangesetId> {
    HgChangesetId::from_str(&changesetid)
        .with_context(|_| ErrorKind::InvalidInput(changesetid))
        .map_err(From::from)
}
