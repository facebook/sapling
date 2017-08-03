// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::fmt::{self, Display};
use std::marker::PhantomData;

use futures::future::{BoxFuture, Future};
use futures::stream::{BoxStream, Stream};

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

    fn boxed(self) -> Box<Manifest<Error=Self::Error> + Sync> where Self: Sync + Sized {
        Box::new(self)
    }
}

pub struct BoxManifest<M, E> where M: Manifest {
    manifest: M,
    cvterr: fn(M::Error) -> E,
    _phantom: PhantomData<E>,
}

// The box can be Sync iff R is Sync, E doesn't matter as its phantom
unsafe impl<M, E> Sync for BoxManifest<M, E> where M: Manifest + Sync {}

impl<M, E> BoxManifest<M, E>
where
    M: Manifest + Sync + Send + 'static,
    E: Send + 'static,
{
    pub fn new(manifest: M) -> Box<Manifest<Error=E> + Sync> where E: From<M::Error> {
        Self::new_with_cvterr(manifest, E::from)
    }

    pub fn new_with_cvterr(manifest: M, cvterr: fn(M::Error) -> E) -> Box<Manifest<Error=E> + Sync> {
        let bm = BoxManifest {
            manifest,
            cvterr,
            _phantom: PhantomData,
        };

        Box::new(bm)
    }
}

impl<M, E> Manifest for BoxManifest<M, E>
where
    M: Manifest + Sync + Send + 'static,
    E: Send + 'static,
{
    type Error = E;

    fn lookup(&self, path: &Path)
        -> BoxFuture<Option<Box<Entry<Error=Self::Error>>>, Self::Error> {
        let cvterr = self.cvterr;

        self.manifest.lookup(path)
            .map(move |oe| oe.map(|e| BoxEntry::new_with_cvterr(e, cvterr)))
            .map_err(cvterr)
            .boxed()
    }

    fn list(&self) -> BoxStream<Box<Entry<Error=Self::Error>>, Self::Error> {
        let cvterr = self.cvterr;

        self.manifest.list()
            .map(move |e| BoxEntry::new_with_cvterr(e, cvterr))
            .map_err(cvterr)
            .boxed()
    }
}

impl<E: Send + 'static> Manifest for Box<Manifest<Error=E> + Sync> {
    type Error = E;

    fn lookup(&self, path: &Path)
        -> BoxFuture<Option<Box<Entry<Error=Self::Error>>>, Self::Error> {
        (**self).lookup(path)
    }

    fn list(&self) -> BoxStream<Box<Entry<Error=Self::Error>>, Self::Error> {
        (**self).list()
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
    Tree(Box<Manifest<Error=E> + Sync>),
}

impl<E> Content<E> where E: Send + 'static {
    fn map_err<ME>(self, cvterr: fn(E) -> ME) -> Content<ME> where ME: Send + 'static {
        match self {
            Content::Tree(m) => Content::Tree(BoxManifest::new_with_cvterr(m, cvterr)),
            Content::File(b) => Content::File(b),
            Content::Executable(b) => Content::Executable(b),
            Content::Symlink(p) => Content::Symlink(p),
        }
    }
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


pub struct BoxEntry<Ent, E> where Ent: Entry {
    entry: Ent,
    cvterr: fn(Ent::Error) -> E,
    _phantom: PhantomData<E>,
}

unsafe impl<Ent, E> Sync for BoxEntry<Ent, E> where Ent: Entry + Sync {}

impl<Ent, E> BoxEntry<Ent, E>
where
    Ent: Entry,
    E: Send + 'static,
{
    pub fn new(entry: Ent) -> Box<Entry<Error=E>> where E: From<Ent::Error> {
        Self::new_with_cvterr(entry, E::from)
    }

    pub fn new_with_cvterr(entry: Ent, cvterr: fn(Ent::Error) -> E) -> Box<Entry<Error=E>> {
        Box::new(BoxEntry {
            entry,
            cvterr,
            _phantom: PhantomData,
        })
    }
}

impl<Ent, E> Entry for BoxEntry<Ent, E>
where
    Ent: Entry + Send + 'static,
    E: Send + 'static
{
    type Error = E;

    fn get_type(&self) -> Type {
        self.entry.get_type()
    }

    fn get_parents(&self) -> BoxFuture<Parents, Self::Error> {
        self.entry.get_parents()
            .map_err(self.cvterr)
            .boxed()
    }

    fn get_content(&self) -> BoxFuture<Content<Self::Error>, Self::Error> {
        let cvterr = self.cvterr;
        self.entry.get_content()
            .map(move |c| Content::map_err(c, cvterr))
            .map_err(self.cvterr)
            .boxed()
    }

    fn get_hash(&self) -> &NodeHash {
        self.entry.get_hash()
    }

    fn get_path(&self) -> &Path {
        self.entry.get_path()
    }
}

impl<E> Entry for Box<Entry<Error=E>> where E: Send + 'static {
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
