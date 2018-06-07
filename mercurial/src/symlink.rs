// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use mercurial_types::HgBlobNode;

use file::File;

#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct Symlink(File);

impl Symlink {
    pub fn new(node: HgBlobNode) -> Symlink {
        Symlink(File::from_blobnode(node))
    }

    pub fn size(&self) -> usize {
        self.0.size()
    }
}
