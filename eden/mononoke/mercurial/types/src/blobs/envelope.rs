/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use super::file::File;
use crate::blobs::HgBlobManifest;
use crate::HgFileEnvelope;
use crate::HgFileNodeId;
use crate::HgManifestEnvelope;
use crate::HgParents;
use crate::MPath;
use anyhow::Result;

pub trait HgBlobEnvelope: Send + Sync + 'static {
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

impl HgBlobEnvelope for HgBlobManifest {
    fn get_parents(&self) -> HgParents {
        self.hg_parents()
    }

    fn get_copy_info(&self) -> Result<Option<(MPath, HgFileNodeId)>> {
        Ok(None)
    }

    fn get_size(&self) -> Option<u64> {
        None
    }
}
