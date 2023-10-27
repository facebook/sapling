/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! # storemodel
//!
//! Provides trait definitions for storage needs.
//! Useful to de-couple from heavyweight storage implementations.
//!
//! Traits defined by this crate are ideally tech-debt free and
//! consider both hg and git use-cases. This mainly means:
//! - APIs do expose hg details like filelog "copy from" or LFS pointer.
//! - History-related APIs should avoid linkrev or linknode, which do not exist
//!   in git.
//!
//! For flexibility, different features might be defined in different traits.
//! Traits can be combined later. For example, reading file content, metadata,
//! and history should probably be 3 different traits.

use std::any::Any;
use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use edenapi_trait::EdenApi;
pub use futures;
use futures::stream::BoxStream;
pub use minibytes;
pub use types;
use types::HgId;
use types::Key;
use types::RepoPath;

#[async_trait]
#[auto_impl::auto_impl(Arc)]
pub trait FileStore: Send + Sync + 'static {
    /// Read the content of specified files.
    ///
    /// The returned content should be just the file contents. This means:
    /// - The returned content does not contain the "copy from" header.
    /// - The returned content does not contain raw LFS content. LFS pointer
    ///   is resolved transparently.
    async fn get_content_stream(
        &self,
        keys: Vec<Key>,
    ) -> BoxStream<anyhow::Result<(minibytes::Bytes, Key)>>;

    /// Read rename metadata of sepcified files.
    ///
    /// The result is a vector of (key, Option<rename_from_key>) pairs for success case.
    async fn get_rename_stream(
        &self,
        keys: Vec<Key>,
    ) -> BoxStream<anyhow::Result<(Key, Option<Key>)>>;

    /// Read the content of the specified file without connecting to a remote server.
    /// Return `None` if the file is available locally.
    fn get_local_content(&self, key: &Key) -> anyhow::Result<Option<minibytes::Bytes>>;

    /// Refresh the store so it might pick up new contents written by other processes.
    fn refresh(&self) -> anyhow::Result<()> {
        Ok(())
    }

    /// Optional downcasting. If a store wants downcasting support, implement this
    /// as `Some(self)` explicitly.
    fn maybe_as_any(&self) -> Option<&dyn Any> {
        None
    }
}

#[async_trait]
pub trait ReadRootTreeIds {
    /// Read root tree nodes of given commits.
    /// Return `(commit_id, tree_id)` list.
    async fn read_root_tree_ids(&self, commits: Vec<HgId>) -> anyhow::Result<Vec<(HgId, HgId)>>;
}

/// The `TreeStore` is an abstraction layer for the tree manifest that decouples how or where the
/// data is stored. This allows more easy iteration on serialization format. It also simplifies
/// writing storage migration.
#[auto_impl::auto_impl(Arc)]
pub trait TreeStore: Send + Sync {
    /// Read the contents of a directory.
    ///
    /// The result is opaque bytes data, encoded using the format specified by `format()`.
    /// To parse the bytes consider `manifest_tree::TreeEntry`.
    fn get(&self, path: &RepoPath, hgid: HgId) -> anyhow::Result<minibytes::Bytes>;

    fn insert(&self, path: &RepoPath, hgid: HgId, data: minibytes::Bytes) -> anyhow::Result<()>;

    /// Indicate to the store that we will be attempting to access the given
    /// tree nodes soon. Some stores (especially ones that may perform network
    /// I/O) may use this information to prepare for these accesses (e.g., by
    /// by prefetching the nodes in bulk). For some stores this operation does
    /// not make sense, so the default implementation is a no-op.
    fn prefetch(&self, _keys: Vec<Key>) -> anyhow::Result<()> {
        Ok(())
    }

    /// Decides whether the store uses git or hg format.
    fn format(&self) -> TreeFormat {
        TreeFormat::Hg
    }

    /// Refresh the store so it might pick up new contents written by other processes.
    fn refresh(&self) -> anyhow::Result<()> {
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub enum TreeFormat {
    // NAME '\0' HEX_SHA1 MODE '\n'
    // MODE: 't' (tree), 'l' (symlink), 'x' (executable)
    Hg,

    // MODE ' ' NAME '\0' BIN_SHA1
    // MODE: '40000' (tree), '100644' (regular), '100755' (executable),
    //       '120000' (symlink), '160000' (gitlink)
    Git,
}

/// Provide information about how to build a file and tree store.
pub trait StoreInfo: 'static {
    /// Check requirement. Return `true` if the requirement is present.
    fn has_requirement(&self, requirement: &str) -> bool;
    /// Provides the config.
    fn config(&self) -> &dyn configmodel::Config;
    /// Provide the "storage path", which is usually `.sl/store` in the backing repo.
    fn store_path(&self) -> &Path;
    /// Provide the remote peer.
    fn remote_peer(&self) -> anyhow::Result<Option<Arc<dyn EdenApi>>>;
}

/// Provide ways to obtain file and tree stores.
pub trait StoreOutput: 'static {
    /// Obtain the file store.
    fn file_store(&self) -> Arc<dyn FileStore>;

    /// Obtain the tree store.
    ///
    /// Based on the implementation, this might or might not be the same as the
    /// file store under the hood.
    fn tree_store(&self) -> Arc<dyn TreeStore>;
}

impl<T: FileStore + TreeStore> StoreOutput for Arc<T> {
    fn file_store(&self) -> Arc<dyn FileStore> {
        self.clone() as Arc<dyn FileStore>
    }

    fn tree_store(&self) -> Arc<dyn TreeStore> {
        self.clone() as Arc<dyn TreeStore>
    }
}
