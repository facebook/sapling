// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::fmt::{self, Display};

use failure::Error;
use futures::future::Future;
use futures::stream::Stream;

use blob::Blob;
use blobnode::Parents;
use futures_ext::{BoxFuture, BoxStream, FutureExt, StreamExt};
use nodehash::NodeHash;
use path::{MPath, RepoPath};

/// Interface for a manifest
pub trait Manifest: Send + 'static {
    fn lookup(&self, path: &MPath) -> BoxFuture<Option<Box<Entry + Sync>>, Error>;
    fn list(&self) -> BoxStream<Box<Entry + Sync>, Error>;

    fn boxed(self) -> Box<Manifest + Sync>
    where
        Self: Sync + Sized,
    {
        Box::new(self)
    }
}

pub struct BoxManifest<M>
where
    M: Manifest,
{
    manifest: M,
}

impl<M> BoxManifest<M>
where
    M: Manifest + Sync + Send + 'static,
{
    pub fn new(manifest: M) -> Box<Manifest + Sync> {
        let bm = BoxManifest { manifest };

        Box::new(bm)
    }
}

impl<M> Manifest for BoxManifest<M>
where
    M: Manifest + Sync + Send + 'static,
{
    fn lookup(&self, path: &MPath) -> BoxFuture<Option<Box<Entry + Sync>>, Error> {
        self.manifest
            .lookup(path)
            .map(move |oe| oe.map(|e| BoxEntry::new(e)))
            .boxify()
    }

    fn list(&self) -> BoxStream<Box<Entry + Sync>, Error> {
        self.manifest.list().map(move |e| BoxEntry::new(e)).boxify()
    }
}

impl Manifest for Box<Manifest + Sync> {
    fn lookup(&self, path: &MPath) -> BoxFuture<Option<Box<Entry + Sync>>, Error> {
        (**self).lookup(path)
    }

    fn list(&self) -> BoxStream<Box<Entry + Sync>, Error> {
        (**self).list()
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Serialize)]
pub enum Type {
    File,
    Symlink,
    Tree,
    Executable,
}

pub enum Content {
    File(Blob<Vec<u8>>),       // TODO stream
    Executable(Blob<Vec<u8>>), // TODO stream
    Symlink(MPath),
    Tree(Box<Manifest + Sync>),
}

pub trait Entry: Send + 'static {
    fn get_type(&self) -> Type;
    fn get_parents(&self) -> BoxFuture<Parents, Error>;
    fn get_raw_content(&self) -> BoxFuture<Blob<Vec<u8>>, Error>;
    fn get_content(&self) -> BoxFuture<Content, Error>;
    fn get_size(&self) -> BoxFuture<Option<usize>, Error>;
    fn get_hash(&self) -> &NodeHash;
    fn get_path(&self) -> &RepoPath;
    fn get_mpath(&self) -> &MPath;

    fn boxed(self) -> Box<Entry + Sync>
    where
        Self: Sync + Sized,
    {
        Box::new(self)
    }
}

pub struct BoxEntry<Ent>
where
    Ent: Entry,
{
    entry: Ent,
}

impl<Ent> BoxEntry<Ent>
where
    Ent: Entry + Sync + Send + 'static,
{
    pub fn new(entry: Ent) -> Box<Entry + Sync> {
        Box::new(BoxEntry { entry })
    }
}

impl<Ent> Entry for BoxEntry<Ent>
where
    Ent: Entry + Sync + Send + 'static,
{
    fn get_type(&self) -> Type {
        self.entry.get_type()
    }

    fn get_parents(&self) -> BoxFuture<Parents, Error> {
        self.entry.get_parents().boxify()
    }

    fn get_raw_content(&self) -> BoxFuture<Blob<Vec<u8>>, Error> {
        self.entry.get_raw_content().boxify()
    }

    fn get_content(&self) -> BoxFuture<Content, Error> {
        self.entry.get_content().boxify()
    }

    fn get_size(&self) -> BoxFuture<Option<usize>, Error> {
        self.entry.get_size().boxify()
    }

    fn get_hash(&self) -> &NodeHash {
        self.entry.get_hash()
    }

    fn get_path(&self) -> &RepoPath {
        self.entry.get_path()
    }

    fn get_mpath(&self) -> &MPath {
        self.entry.get_mpath()
    }
}

impl Entry for Box<Entry + Sync> {
    fn get_type(&self) -> Type {
        (**self).get_type()
    }

    fn get_parents(&self) -> BoxFuture<Parents, Error> {
        (**self).get_parents()
    }

    fn get_raw_content(&self) -> BoxFuture<Blob<Vec<u8>>, Error> {
        (**self).get_raw_content()
    }

    fn get_content(&self) -> BoxFuture<Content, Error> {
        (**self).get_content()
    }

    fn get_size(&self) -> BoxFuture<Option<usize>, Error> {
        (**self).get_size()
    }

    fn get_hash(&self) -> &NodeHash {
        (**self).get_hash()
    }

    fn get_path(&self) -> &RepoPath {
        (**self).get_path()
    }

    fn get_mpath(&self) -> &MPath {
        (**self).get_mpath()
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
