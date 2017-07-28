// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::fmt::{self, Display};

use futures::BoxFuture;
use futures::stream::BoxStream;

use blob::Blob;
use blobnode::Parents;
use path::Path;
use nodehash::NodeHash;

/// Interface for a manifest
pub trait Manifest: Send + 'static {
    type Error: Send + 'static;

    fn lookup(
        &self,
        path: &Path,
    ) -> BoxFuture<Option<Box<Entry<Error = Self::Error>>>, Self::Error>;
    fn list(&self) -> BoxStream<Box<Entry<Error = Self::Error>>, Self::Error>;

    fn boxed(self) -> Box<Manifest<Error = Self::Error>>
    where
        Self: Sized,
    {
        Box::new(self)
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub enum Type {
    File,
    Symlink,
    Tree,
    Executable,
}

pub enum Content<E> {
    File(Blob<Vec<u8>>), // TODO stream
    Executable(Blob<Vec<u8>>), // TODO stream
    Symlink(Path),
    Tree(Box<Manifest<Error = E>>),
}

pub trait Entry: Send + 'static {
    type Error: Send + 'static;

    fn get_type(&self) -> Type;
    fn get_parents(&self) -> BoxFuture<Parents, Self::Error>;
    fn get_content(&self) -> BoxFuture<Content<Self::Error>, Self::Error>;
    fn get_hash(&self) -> &NodeHash;
    fn get_path(&self) -> &Path;

    fn boxed(self) -> Box<Entry<Error = Self::Error>>
    where
        Self: Sized,
    {
        Box::new(self)
    }
}

impl<E> Entry for Box<Entry<Error = E>>
where
    E: Send + 'static,
{
    type Error = E;

    fn get_type(&self) -> Type {
        (**self).get_type()
    }

    fn get_parents(&self) -> BoxFuture<Parents, Self::Error> {
        (**self).get_parents()
    }

    fn get_content(&self) -> BoxFuture<Content<Self::Error>, Self::Error> {
        (**self).get_content()
    }

    fn get_hash(&self) -> &NodeHash {
        (**self).get_hash()
    }

    fn get_path(&self) -> &Path {
        (**self).get_path()
    }
}

impl Display for Type {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        let s = match self {
            &Type::Symlink => "l",
            &Type::Executable => "x",
            &Type::Tree => "t",
            &Type::File => "",
        };
        write!(fmt, "{}", s)
    }
}
