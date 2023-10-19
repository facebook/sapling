/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use anyhow::Result;
use dag::Set;
use dag::Vertex;
use manifest_tree::TreeStore;
use storemodel::ReadRootTreeIds;
use types::RepoPathBuf;

use crate::pathhistory::PathHistoryStats;
use crate::PathHistory;

/// State for answering rename questions about path.
pub struct RenameTracer {
    // RenameTracer is a thin wrapper of PathHistory
    path_history: PathHistory,
}

impl RenameTracer {
    /// RenameTracer is a tool for finding the vertex that added (or renamed to)
    /// the specified path. We will use this in the copy tracing component to trace
    /// a path's rename history.
    pub async fn new(
        set: Set,
        path: RepoPathBuf,
        root_tree_reader: Arc<dyn ReadRootTreeIds + Send + Sync>,
        tree_store: Arc<dyn TreeStore>,
    ) -> Result<Self> {
        let paths = vec![path];
        let ignore_file_content = true;
        let path_history = PathHistory::new_internal(
            set,
            paths,
            root_tree_reader,
            tree_store,
            ignore_file_content,
        )
        .await?;

        Ok(Self { path_history })
    }

    // Obtain statistics for performance analysis.
    pub fn stats(&self) -> &PathHistoryStats {
        self.path_history.stats()
    }

    /// Find the next vertex that renamed the specified path
    pub async fn next(&mut self) -> Result<Option<Vertex>> {
        self.path_history.next().await
    }
}
