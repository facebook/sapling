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

use async_trait::async_trait;
pub use futures;
use futures::stream::BoxStream;
pub use minibytes;
pub use types;
use types::HgId;
use types::Key;
use types::RepoPath;

#[async_trait]
#[auto_impl::auto_impl(Arc)]
pub trait ReadFileContents {
    type Error;

    /// Read the content of specified files.
    ///
    /// The returned content should be just the file contents. This means:
    /// - The returned content does not contain the "copy from" header.
    /// - The returned content does not contain raw LFS content. LFS pointer
    ///   is resolved transparently.
    /// - If the file content is redacted, it's an error instead of an explicit
    ///   instead of a placeholder of dummy data.
    async fn read_file_contents(
        &self,
        keys: Vec<Key>,
    ) -> BoxStream<Result<(minibytes::Bytes, Key), Self::Error>>;
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
pub trait TreeStore {
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
