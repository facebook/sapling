// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use failure_ext::Result;
use mercurial_revlog::file::File;
use mercurial_types::{HgFileEnvelope, HgFileNodeId, HgManifestEnvelope, HgParents, MPath};

pub trait HgBlobEnvelope {
    fn get_parents(&self) -> HgParents;
    fn get_copy_info(&self) -> Result<Option<(MPath, HgFileNodeId)>>;
    fn get_size(&self) -> Option<u64>;
}

impl HgBlobEnvelope for HgFileEnvelope {
    fn get_parents(&self) -> HgParents {
        let (p1, p2) = self.parents();
        HgParents::new(
            p1.map(HgFileNodeId::into_nodehash),
            p2.map(HgFileNodeId::into_nodehash),
        )
    }

    fn get_copy_info(&self) -> Result<Option<(MPath, HgFileNodeId)>> {
        File::extract_copied_from(self.metadata())
    }

    fn get_size(&self) -> Option<u64> {
        Some(self.content_size())
    }
}

impl HgBlobEnvelope for HgManifestEnvelope {
    fn get_parents(&self) -> HgParents {
        let (p1, p2) = self.parents();
        HgParents::new(p1, p2)
    }

    fn get_copy_info(&self) -> Result<Option<(MPath, HgFileNodeId)>> {
        Ok(None)
    }

    fn get_size(&self) -> Option<u64> {
        None
    }
}
