/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use mercurial_types::Delta;
use mercurial_types::HgNodeHash;
use mercurial_types::NonRootMPath;
use mercurial_types::RevFlags;

pub mod packer;
pub mod unpacker;
pub use unpacker::CgVersion;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Section {
    Changeset,
    Manifest,
    Treemanifest,
    Filelog(NonRootMPath),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Part {
    CgChunk(Section, CgDeltaChunk),
    SectionEnd(Section),
    End,
}

impl Part {
    pub fn is_section_end(&self) -> bool {
        match self {
            &Part::SectionEnd(_) => true,
            _ => false,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CgDeltaChunk {
    pub node: HgNodeHash,
    pub p1: HgNodeHash,
    pub p2: HgNodeHash,
    pub base: HgNodeHash,
    pub linknode: HgNodeHash,
    pub delta: Delta,
    pub flags: Option<RevFlags>,
}
