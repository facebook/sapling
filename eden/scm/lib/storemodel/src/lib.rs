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
pub use minibytes::Bytes;
use serde::Deserialize;
use serde::Serialize;
pub use types;
use types::HgId;
use types::Key;
use types::RepoPath;

/// Boxed dynamic iterator. Similar to `BoxStream`.
pub type BoxIterator<'a, T> = Box<dyn Iterator<Item = T> + Send + 'a>;

/// A store where content is indexed by "(path, hash)", aka "Key".
#[async_trait]
pub trait KeyStore: Send + Sync {
    /// Read the content of specified files.
    ///
    /// The returned content should be just the file contents. This means:
    /// - The returned content does not contain the "copy from" header.
    /// - The returned content does not contain raw LFS content. LFS pointer
    ///   is resolved transparently.
    ///
    /// The iterator might block waiting for network.
    fn get_content_iter(
        &self,
        keys: Vec<Key>,
    ) -> anyhow::Result<BoxIterator<anyhow::Result<(Key, Bytes)>>> {
        let iter = keys
            .into_iter()
            .map(|k| match self.get_local_content(&k.path, k.hgid) {
                Err(e) => Err(e),
                Ok(None) => Err(anyhow::format_err!(
                    "{}@{}: not found locally",
                    k.path,
                    k.hgid
                )),
                Ok(Some(data)) => Ok((k, data)),
            });
        Ok(Box::new(iter))
    }

    /// Read the content of the specified file without connecting to a remote server.
    /// Return `None` if the file is not available locally.
    fn get_local_content(
        &self,
        _path: &RepoPath,
        _hgid: HgId,
    ) -> anyhow::Result<Option<minibytes::Bytes>> {
        Ok(None)
    }

    /// Read the content of the specified file. Ask a remote server on demand.
    /// When fetching many files, use `get_content_iter` instead of calling
    /// this in a loop.
    fn get_content(&self, path: &RepoPath, hgid: HgId) -> anyhow::Result<minibytes::Bytes> {
        // Handle "broken" implementation that returns Err(_) not Ok(None) on not found.
        if let Ok(Some(data)) = self.get_local_content(path, hgid) {
            return Ok(data);
        }

        let key = Key::new(path.to_owned(), hgid);
        match self.get_content_iter(vec![key])?.next() {
            None => Err(anyhow::format_err!("{}@{}: not found remotely", path, hgid)),
            Some(Err(e)) => Err(e),
            Some(Ok((_k, data))) => Ok(data),
        }
    }

    /// Indicate to the store that we will be attempting to access the given
    /// items soon. Some stores (especially ones that may perform network
    /// I/O) may use this information to prepare for these accesses (e.g., by
    /// by prefetching the nodes in bulk). For some stores this operation does
    /// not make sense, so the default implementation is a no-op.
    ///
    /// This is an old API. Consider `get_content_iter` instead.
    fn prefetch(&self, _keys: Vec<Key>) -> anyhow::Result<()> {
        Ok(())
    }

    /// Insert a serialized entry. Return the hash to the entry.
    ///
    /// After calling this function, the data can be fetched via
    /// `get_local_content` on the same store. The store can buffer pending data
    /// in memory until `flush()`, or `flush()` automatically to keep memory
    /// usage bounded.
    ///
    /// For stores using hg format:
    /// - `parents` is required, and will affect the hash.
    /// - `data` should contain the filelog metadata header.
    ///
    /// For stores using git format:
    /// - `parents` is a hint to choose delta base.
    /// - `data` is the pure content without headers.
    fn insert_data(
        &self,
        _opts: InsertOpts,
        _path: &RepoPath,
        _data: &[u8],
    ) -> anyhow::Result<HgId> {
        anyhow::bail!("store is read-only")
    }

    /// Write pending changes to disk.
    fn flush(&self) -> anyhow::Result<()> {
        anyhow::bail!("store is read-only")
    }

    /// Refresh the store so it might pick up new contents written by other processes.
    fn refresh(&self) -> anyhow::Result<()> {
        Ok(())
    }

    /// Decides whether the store uses git or hg format.
    fn format(&self) -> SerializationFormat {
        SerializationFormat::Hg
    }

    /// Optional downcasting. If a store wants downcasting support, implement this
    /// as `Some(self)` explicitly.
    fn maybe_as_any(&self) -> Option<&dyn Any> {
        None
    }
}

/// A store for files.
#[async_trait]
pub trait FileStore: KeyStore + 'static {
    /// Read rename metadata of specified files.
    ///
    /// The result is a vector of (key, rename_from_key) pairs for files with
    /// rename information.
    async fn get_rename_stream(&self, _keys: Vec<Key>) -> BoxStream<anyhow::Result<(Key, Key)>> {
        Box::pin(futures::stream::empty())
    }

    fn as_key_store(&self) -> &dyn KeyStore
    where
        Self: Sized,
    {
        self
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
pub trait TreeStore: KeyStore {
    fn as_key_store(&self) -> &dyn KeyStore
    where
        Self: Sized,
    {
        self
    }
}

/// Decides the serialization format. This exists so different parts of the code
/// base can agree on how to generate a SHA1, how to lookup in a tree, etc.
/// Ideally this information is private and the differences are behind
/// abstractions too.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub enum SerializationFormat {
    // Hg SHA1:
    //   SORTED_PARENTS CONTENT
    //
    // Hg file:
    //   FILELOG_METADATA CONTENT
    //
    // Hg tree:
    //   NAME '\0' HEX_SHA1 MODE '\n'
    //   MODE: 't' (tree), 'l' (symlink), 'x' (executable)
    //   (sorted by name)
    Hg,

    // Git SHA1:
    //   TYPE LENGTH CONTENT
    //
    // Git file:
    //   CONTENT
    //
    // Git tree:
    //   MODE ' ' NAME '\0' BIN_SHA1
    //   MODE: '40000' (tree), '100644' (regular), '100755' (executable),
    //         '120000' (symlink), '160000' (gitlink)
    //   (sorted by name, but directory names are treated as ending with '/')
    Git,
}

/// Options used by `insert_data`
#[derive(Deserialize, Default)]
pub struct InsertOpts {
    /// Parent hashes.
    /// For Hg it's required and affects SHA1.
    /// For Git it's a hint about the delta bases.
    pub parents: Vec<HgId>,

    /// Whether this is a file or a tree.
    /// For Hg it's ignored. For Git it affects SHA1.
    pub kind: Kind,

    /// Forced SHA1 to use. Mainly for testing purpose.
    #[serde(default)]
    pub forced_id: Option<Box<HgId>>,

    /// Hg flags to use. Used for legacy LFS support.
    #[serde(default)]
    pub hg_flags: u32,
}

/// Distinguish between a file and a tree.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    File,
    Tree,
}

impl Default for Kind {
    fn default() -> Self {
        Kind::File
    }
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
