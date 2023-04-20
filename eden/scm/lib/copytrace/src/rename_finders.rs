/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use manifest::DiffType;
use manifest_tree::Diff;
use manifest_tree::TreeManifest;
use pathmatcher::AlwaysMatcher;
use storemodel::futures::StreamExt;
use storemodel::ReadFileContents;
use types::Key;
use types::RepoPathBuf;

/// Finding rename between old and new trees (commits).
/// old_tree is a parent of new_tree
#[async_trait]
pub trait RenameFinder {
    /// Find rename file paris in the specified commits.
    async fn find_renames(
        &self,
        old_tree: &TreeManifest,
        new_tree: &TreeManifest,
    ) -> Result<HashMap<RepoPathBuf, RepoPathBuf>>;
}

/// Rename finder for Sapling repo.
pub struct SaplingRenameFinder {
    // Read content and rename metadata of a file
    file_reader: Arc<dyn ReadFileContents<Error = anyhow::Error> + Send + Sync>,
}

impl SaplingRenameFinder {
    pub fn new(
        file_reader: Arc<dyn ReadFileContents<Error = anyhow::Error> + Send + Sync>,
    ) -> Self {
        Self { file_reader }
    }

    async fn read_renamed_metadata(
        &self,
        keys: Vec<Key>,
    ) -> Result<HashMap<RepoPathBuf, RepoPathBuf>> {
        tracing::trace!(keys_len = keys.len(), " read_renamed_metadata");
        let mut renames = self.file_reader.read_rename_metadata(keys).await;

        let mut map: HashMap<RepoPathBuf, RepoPathBuf> = HashMap::new();
        while let Some(rename) = renames.next().await {
            let (key, rename_from_key) = rename?;
            if let Some(rename_from_key) = rename_from_key {
                map.insert(key.path, rename_from_key.path);
            }
        }
        tracing::trace!(result_map_len = map.len(), " read_renamed_metadata");
        Ok(map)
    }
}

#[async_trait]
impl RenameFinder for SaplingRenameFinder {
    async fn find_renames(
        &self,
        old_tree: &TreeManifest,
        new_tree: &TreeManifest,
    ) -> Result<HashMap<RepoPathBuf, RepoPathBuf>> {
        let mut new_files = Vec::new();

        {
            // this block is for dropping matcher and diff at the end of the block,
            // otherwise the compiler compilains variable might be used across 'await'

            let matcher = AlwaysMatcher::new();
            let diff = Diff::new(old_tree, new_tree, &matcher)?;
            for entry in diff {
                let entry = entry?;

                if let DiffType::RightOnly(file_metadata) = entry.diff_type {
                    let path = entry.path;
                    let key = Key {
                        path,
                        hgid: file_metadata.hgid,
                    };
                    new_files.push(key);
                }
            }
        }

        self.read_renamed_metadata(new_files).await
    }
}
