/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::actions::UpdateAction;
use manifest::FileMetadata;
use std::collections::HashMap;
use std::ops::{Deref, DerefMut};
use types::RepoPathBuf;

pub enum Conflict {
    // ("m", (f, f, f, False, pa.node()), "versions differ")
    // -or-
    // ("m", (f, f, None, False, pa.node()), "both created")
    BothChanged {
        ancestor: Option<FileMetadata>,
        dest: FileMetadata,
        src: FileMetadata,
    },
    SrcRemovedDstChanged(UpdateAction), // ("cd", (f, None, f, False, pa.node()), "prompt changed/deleted")
    DstRemovedSrcChanged(UpdateAction), // ("dc", (None, f, f, False, pa.node()), "prompt deleted/changed")
}

// mergestate in python
#[derive(Default)]
pub struct ConflictState {
    map: HashMap<RepoPathBuf, Conflict>,
}

impl Deref for ConflictState {
    type Target = HashMap<RepoPathBuf, Conflict>;

    fn deref(&self) -> &Self::Target {
        &self.map
    }
}

impl DerefMut for ConflictState {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.map
    }
}
