// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use mercurial_types::{BlobNode, Path};

use file::File;
use errors::*;

#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct Symlink(File);

impl Symlink {
    pub fn new(node: BlobNode) -> Symlink {
        Symlink(File::new(node))
    }

    pub fn path(&self) -> Result<Option<Path>> {
        if let Some(path) = self.0.content().map(|s| Path::new(s)) {
            Ok(Some(path.chain_err(|| "invalid symlink target")?))
        } else {
            Ok(None)
        }
    }

    pub fn size(&self) -> Option<usize> {
        self.0.size()
    }
}
