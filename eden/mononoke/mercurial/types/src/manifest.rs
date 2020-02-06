/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use std::fmt;
use std::iter;

use anyhow::Error;
use context::CoreContext;
use futures_ext::{BoxFuture, BoxStream};
use mononoke_types::{FileType, MPathElement};
use serde_derive::Serialize;

use crate::blob::HgBlob;
use crate::blobnode::HgParents;
use crate::file::FileBytes;
use crate::nodehash::HgEntryId;

/// Interface for a manifest
///
/// A `Manifest` represents the mapping between a list of names and `Entry`s - ie,
/// functionally equivalent to a directory.
///
/// The name "Manifest" comes from Mercurial, where a single object represents the entire repo
/// namespace ("flat manifest"). But modern Mercurial and Mononoke use a distinct Manifest for
/// each directory ("tree manifest"). As a result, operations on a manifest are path element at
/// a time.
///
/// NOTE: We have similar `Manifest` trait which is generic over different kinds of manifests
/// (unodes, hg manifests etc). Prefer to use `Manifest` trait over HgManifest is possible.
pub trait HgManifest: Send + 'static {
    /// Look up a specific entry in the Manifest by name
    ///
    /// If the name exists, return it as Some(entry). If it doesn't exist, return None.
    /// If it returns an error, it indicates something went wrong with the underlying
    /// infrastructure.
    fn lookup(&self, path: &MPathElement) -> Option<Box<dyn HgEntry + Sync>>;

    /// List all the entries in the Manifest.
    ///
    /// Entries are returned in canonical order.
    fn list(&self) -> Box<dyn Iterator<Item = Box<dyn HgEntry + Sync>> + Send>;

    /// Return self as a type-erased boxed trait (still needed as a trait method? T25577105)
    fn boxed(self) -> Box<dyn HgManifest + Sync>
    where
        Self: Sync + Sized,
    {
        Box::new(self)
    }
}

pub fn get_empty_manifest() -> Box<dyn HgManifest + Sync> {
    Box::new(HgEmptyManifest::new())
}

pub struct HgEmptyManifest;

impl HgEmptyManifest {
    #[inline]
    pub fn new() -> Self {
        HgEmptyManifest
    }
}

impl HgManifest for HgEmptyManifest {
    fn lookup(&self, _path: &MPathElement) -> Option<Box<dyn HgEntry + Sync>> {
        None
    }

    fn list(&self) -> Box<dyn Iterator<Item = Box<dyn HgEntry + Sync>> + Send> {
        Box::new(iter::empty())
    }
}

impl HgManifest for Box<dyn HgManifest + Sync> {
    fn lookup(&self, path: &MPathElement) -> Option<Box<dyn HgEntry + Sync>> {
        (**self).lookup(path)
    }

    fn list(&self) -> Box<dyn Iterator<Item = Box<dyn HgEntry + Sync>> + Send> {
        (**self).list()
    }
}

impl HgManifest for Box<dyn HgManifest> {
    fn lookup(&self, path: &MPathElement) -> Option<Box<dyn HgEntry + Sync>> {
        (**self).lookup(path)
    }

    fn list(&self) -> Box<dyn Iterator<Item = Box<dyn HgEntry + Sync>> + Send> {
        (**self).list()
    }
}

/// Type of an Entry
///
/// File and Executable are identical - they both represent files containing arbitrary content.
/// The only difference is that the Executables are created with executable permission when
/// checked out.
///
/// Symlink is also the same as File, but the content of the file is interpolated into a path
/// being traversed during lookup.
///
/// Tree is a reference to another Manifest (directory-like) object.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Serialize)]
pub enum Type {
    File(FileType),
    Tree,
}

impl Type {
    #[inline]
    pub fn is_tree(&self) -> bool {
        self == &Type::Tree
    }

    pub fn manifest_suffix(&self) -> &'static str {
        // It's a little weird that this returns a Unicode string and not a bytestring, but that's
        // what callers demand.
        match self {
            Type::Tree => "t",
            Type::File(FileType::Symlink) => "l",
            Type::File(FileType::Executable) => "x",
            Type::File(FileType::Regular) => "",
        }
    }
}

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Type::Tree => write!(f, "tree"),
            Type::File(ft) => write!(f, "{}", ft),
        }
    }
}

/// Concrete representation of various Entry Types.
pub enum Content {
    File(BoxStream<FileBytes, Error>),
    Executable(BoxStream<FileBytes, Error>),
    // Symlinks typically point to files but can have arbitrary content, so represent them as
    // blobs rather than as MPath instances.
    Symlink(BoxStream<FileBytes, Error>),
    Tree(Box<dyn HgManifest + Sync>),
}

impl Content {
    pub fn new_file(file_type: FileType, contents: BoxStream<FileBytes, Error>) -> Self {
        match file_type {
            FileType::Regular => Content::File(contents),
            FileType::Executable => Content::Executable(contents),
            FileType::Symlink => Content::Symlink(contents),
        }
    }
}

impl fmt::Debug for Content {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            Content::Tree(_) => write!(f, "Tree(...)"),
            Content::File(..) => write!(f, "File(...)"),
            Content::Executable(..) => write!(f, "Executable(...)"),
            Content::Symlink(..) => write!(f, "Symlink(...)"),
        }
    }
}

/// An entry represents a single entry in a Manifest
///
/// The Entry has at least a name, a type, and the identity of the object it refers to

pub trait HgEntry: Send + 'static {
    /// Type of the object this entry refers to
    fn get_type(&self) -> Type;

    /// Get the parents (in the history graph) of the referred-to object
    fn get_parents(&self, ctx: CoreContext) -> BoxFuture<HgParents, Error>;

    /// Get the raw content of the object as it exists in the blobstore,
    /// without any interpretation. This is only really useful for doing a bit-level duplication.
    fn get_raw_content(&self, ctx: CoreContext) -> BoxFuture<HgBlob, Error>;

    /// Get the interpreted content of the object. This will likely require IO
    fn get_content(&self, ctx: CoreContext) -> BoxFuture<Content, Error>;

    /// Get the logical size of the entry. Some entries don't really have a meaningful size.
    fn get_size(&self, ctx: CoreContext) -> BoxFuture<Option<u64>, Error>;

    /// Get the identity of the object this entry refers to.
    fn get_hash(&self) -> HgEntryId;

    /// Get the name of the entry. None means that this is a root entry
    fn get_name(&self) -> Option<&MPathElement>;

    /// Return an Entry as a type-erased trait object.
    /// (Do we still need this as a trait method? T25577105)
    fn boxed(self) -> Box<dyn HgEntry + Sync>
    where
        Self: Sync + Sized,
    {
        Box::new(self)
    }
}

impl HgEntry for Box<dyn HgEntry + Sync> {
    fn get_type(&self) -> Type {
        (**self).get_type()
    }

    fn get_parents(&self, ctx: CoreContext) -> BoxFuture<HgParents, Error> {
        (**self).get_parents(ctx)
    }

    fn get_raw_content(&self, ctx: CoreContext) -> BoxFuture<HgBlob, Error> {
        (**self).get_raw_content(ctx)
    }

    fn get_content(&self, ctx: CoreContext) -> BoxFuture<Content, Error> {
        (**self).get_content(ctx)
    }

    fn get_size(&self, ctx: CoreContext) -> BoxFuture<Option<u64>, Error> {
        (**self).get_size(ctx)
    }

    fn get_hash(&self) -> HgEntryId {
        (**self).get_hash()
    }

    fn get_name(&self) -> Option<&MPathElement> {
        (**self).get_name()
    }
}

impl fmt::Debug for Box<dyn HgEntry + Sync> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Entry")
            .field("name", &self.get_name())
            .field("hash", &format!("{}", self.get_hash()))
            .field("type", &self.get_type())
            .finish()
    }
}
