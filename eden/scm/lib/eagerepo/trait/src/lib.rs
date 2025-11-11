/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! # eagerepo-trait
//!
//! Toegher with `Id20StoreExtension`, make agerRepo provide more objects
//! (commits, trees, blobs) without actually storing the objects.
//!
//! Provide interfaces to:
//! - Extend EagerRepo's "dag":
//!   - To "pull_lazy" segments automatically when accessing a special commit hash.
//!   - To resolve lazy commits via"RemoteIdConvertProtocol".
//!
//! Currently mainly used by `virtual-repo` to construct synthetic repos.

use std::sync::Arc;

use dag::protocol::RemoteIdConvertProtocol;

/// Extends the EagerRepo's commit graph.
pub trait EagerRepoExtension: Send + Sync + 'static {
    /// Useful to support lazy commit hashes. For example,
    /// `virtual-repo` might want to add millions of (lazy) commits as a segment,
    /// by `dag.import_pull_data`. It does not want O(N) complexity specifying the
    /// commit hashes one by one.
    fn get_dag_remote_protocol(&self) -> Option<Arc<dyn RemoteIdConvertProtocol>>;

    /// The name of the extension.
    fn name(&self) -> &'static str;
}
