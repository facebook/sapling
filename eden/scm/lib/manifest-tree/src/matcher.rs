/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use anyhow::Result;
use manifest::FsNodeMetadata::Directory;
use manifest::FsNodeMetadata::File;
use manifest::Manifest;
use parking_lot::RwLock;
use pathmatcher::DirectoryMatch;
use pathmatcher::Matcher;
use types::RepoPath;

use crate::TreeManifest;

pub struct ManifestMatcher {
    manifest: Arc<RwLock<TreeManifest>>,
}

impl ManifestMatcher {
    pub fn new(manifest: Arc<RwLock<TreeManifest>>) -> Self {
        ManifestMatcher { manifest }
    }
}

impl Matcher for ManifestMatcher {
    fn matches_directory(&self, path: &RepoPath) -> Result<DirectoryMatch> {
        Ok(match self.manifest.read().get(path)? {
            Some(File(_)) => DirectoryMatch::Nothing,
            Some(Directory(_)) => DirectoryMatch::ShouldTraverse,
            None => DirectoryMatch::Nothing,
        })
    }

    fn matches_file(&self, path: &RepoPath) -> Result<bool> {
        Ok(match self.manifest.read().get(path)? {
            Some(File(_)) => true,
            Some(Directory(_)) => false,
            None => false,
        })
    }
}
