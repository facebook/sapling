/*
 * Copyright (c) Facebook, Inc. and its affiliates.
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
use futures::stream::BoxStream;
pub use minibytes::Bytes;
use types::Key;

#[async_trait]
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
    ) -> BoxStream<Result<(Bytes, Key), Self::Error>>;
}
