/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use dag::DagAlgorithm;
use manifest::DiffType;
use manifest_tree::Diff;
use manifest_tree::TreeManifest;
use manifest_tree::TreeStore;
use pathmatcher::AlwaysMatcher;
use storemodel::ReadFileContents;
use storemodel::ReadRootTreeIds;
use types::Key;
use types::RepoPathBuf;

use crate::CopyTrace;

#[allow(dead_code)]
pub struct DagCopyTrace {
    /* Input */
    /// src commit
    src: dag::Vertex,

    /// dst commit
    dst: dag::Vertex,

    /// Resolve commit ids to trees in batch.
    root_tree_reader: Arc<dyn ReadRootTreeIds + Send + Sync>,

    /// Resolve and prefetch trees in batch.
    tree_store: Arc<dyn TreeStore + Send + Sync>,

    // Read content and rename metadata of a file
    file_reader: Arc<dyn ReadFileContents<Error = anyhow::Error> + Send + Sync>,

    /* Derived from input */
    /// Commit graph algorithms
    dag: Arc<dyn DagAlgorithm + Send + Sync>,
}

impl DagCopyTrace {
    #[allow(dead_code)]
    #[allow(unused_variables)]
    fn new(
        src: dag::Vertex,
        dst: dag::Vertex,
        root_tree_reader: Arc<dyn ReadRootTreeIds + Send + Sync>,
        tree_store: Arc<dyn TreeStore + Send + Sync>,
    ) -> Result<Self> {
        todo!()
    }

    fn read_renamed_metadata(&self, keys: Vec<Key>) -> Result<HashMap<RepoPathBuf, RepoPathBuf>> {
        // TODO: add metrics for the size of the result
        let renames = self.file_reader.read_rename_metadata(keys)?;
        let map: HashMap<_, _> = renames
            .into_iter()
            .filter(|(_, v)| v.is_some())
            .map(|(key, rename_from_key)| (key.path, rename_from_key.unwrap().path))
            .collect();
        Ok(map)
    }
}

impl CopyTrace for DagCopyTrace {
    #[allow(unused_variables)]
    fn trace_rename(
        &self,
        src: dag::Vertex,
        dst: dag::Vertex,
        src_path: types::RepoPathBuf,
    ) -> Option<types::RepoPathBuf> {
        todo!()
    }

    #[allow(unused_variables)]
    fn trace_rename_backward(
        &self,
        src: dag::Vertex,
        dst: dag::Vertex,
        dst_path: types::RepoPathBuf,
    ) -> Option<types::RepoPathBuf> {
        todo!()
    }

    #[allow(unused_variables)]
    fn trace_rename_forward(
        &self,
        src: dag::Vertex,
        dst: dag::Vertex,
        src_path: types::RepoPathBuf,
    ) -> Option<types::RepoPathBuf> {
        todo!()
    }

    fn find_renames(
        &self,
        old_tree: &TreeManifest,
        new_tree: &TreeManifest,
    ) -> Result<HashMap<RepoPathBuf, RepoPathBuf>> {
        // todo:
        // * [x] parse file header and get mv info
        // * support content similarity for sl repo
        // * support content similarity for git repo
        let matcher = AlwaysMatcher::new();
        let diff = Diff::new(old_tree, new_tree, &matcher)?;
        let mut new_files = Vec::new();
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

        self.read_renamed_metadata(new_files)
    }
}
