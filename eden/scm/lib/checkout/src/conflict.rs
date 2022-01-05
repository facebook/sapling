/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::fmt;
use std::ops::Deref;
use std::ops::DerefMut;

use manifest::FileMetadata;
use types::RepoPathBuf;

use crate::actions::UpdateAction;

pub enum Conflict {
    // ("m", (f, f, f, False, pa.node()), "versions differ")
    // -or-
    // ("m", (f, f, None, False, pa.node()), "both created")
    BothChanged {
        ancestor: Option<FileMetadata>,
        dest: FileMetadata,
        src: FileMetadata,
    },
    SrcRemovedDstChanged(UpdateAction),
    // ("cd", (f, None, f, False, pa.node()), "prompt changed/deleted")
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

impl fmt::Display for ConflictState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (path, conflict) in &self.map {
            match conflict {
                Conflict::SrcRemovedDstChanged(up) => write!(f, "cd {}=>{}\n", path, up.to.hgid)?,
                Conflict::DstRemovedSrcChanged(up) => write!(f, "dc {}=>{}\n", path, up.to.hgid)?,
                Conflict::BothChanged { dest, src, .. } => {
                    write!(f, "m  {} [src=>{}, dest=>{}]\n", path, src.hgid, dest.hgid)?
                }
            }
        }
        Ok(())
    }
}
