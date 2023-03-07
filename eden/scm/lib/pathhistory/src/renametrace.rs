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

/// State for answering rename questions about path. This is similar to
/// the `PathHistory`.
///
/// TODO: add detailed fields
#[allow(dead_code)]
pub struct RenameTracer {}

impl RenameTracer {
    /// RenameTracer is a tool for finding the vertex that added (or renmaed to)
    /// the specified path. We will use this in the copy tracing component to trace
    /// a path's rename history.
    #[allow(dead_code)]
    #[allow(unused_variables)]
    pub async fn new(
        root_tree_reader: Arc<dyn ReadRootTreeIds + Send + Sync>,
        tree_store: Arc<dyn TreeStore + Send + Sync>,
    ) -> Result<Self> {
        Ok(RenameTracer {})
    }

    /// Find the vertex that added (can be added by a mv operation)
    /// the specified path.
    #[allow(dead_code)]
    #[allow(unused_variables)]
    pub async fn execute(&mut self, set: Set, path: RepoPathBuf) -> Result<Option<Vertex>> {
        Ok(None)
    }
}
