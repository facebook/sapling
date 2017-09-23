// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::error;
use std::fmt::{self, Display};
use std::marker::PhantomData;

use futures::future::Future;
use futures::stream::Stream;

use blob::Blob;
use blobnode::Parents;
use futures_ext::{BoxFuture, BoxStream, FutureExt, StreamExt};
use nodehash::NodeHash;
use path::MPath;

/// Interface for a manifest
pub trait Manifest: Send + 'static {
    type Error: error::Error + Send + 'static;

    fn lookup(
        &self,
        path: &MPath,
    ) -> BoxFuture<Option<Box<Entry<Error = Self::Error> + Sync>>, Self::Error>;
    fn list(&self) -> BoxStream<Box<Entry<Error = Self::Error> + Sync>, Self::Error>;

    fn boxed(self) -> Box<Manifest<Error = Self::Error> + Sync>
    where
        Self: Sync + Sized,
    {
        Box::new(self)
    }
}

pub struct BoxManifest<M, E>
where
    M: Manifest,
{
    manifest: M,
    cvterr: fn(M::Error) -> E,
    _phantom: PhantomData<E>,
}

// The box can be Sync iff R is Sync, E doesn't matter as its phantom
unsafe impl<M, E> Sync for BoxManifest<M, E>
where
    M: Manifest + Sync,
{
}

impl<M, E> BoxManifest<M, E>
where
    M: Manifest + Sync + Send + 'static,
    E: error::Error + Send + 'static,
{
    pub fn new(manifest: M) -> Box<Manifest<Error = E> + Sync>
    where
        E: From<M::Error>,
    {
        Self::new_with_cvterr(manifest, E::from)
    }

    pub fn new_with_cvterr(
        manifest: M,
        cvterr: fn(M::Error) -> E,
    ) -> Box<Manifest<Error = E> + Sync> {
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
    E: error::Error + Send + 'static,
{
    type Error = E;

    fn lookup(
        &self,
        path: &MPath,
    ) -> BoxFuture<Option<Box<Entry<Error = Self::Error> + Sync>>, Self::Error> {
        let cvterr = self.cvterr;

        self.manifest
            .lookup(path)
            .map(move |oe| oe.map(|e| BoxEntry::new_with_cvterr(e, cvterr)))
            .map_err(cvterr)
            .boxify()
    }

    fn list(&self) -> BoxStream<Box<Entry<Error = Self::Error> + Sync>, Self::Error> {
        let cvterr = self.cvterr;

        self.manifest
            .list()
            .map(move |e| BoxEntry::new_with_cvterr(e, cvterr))
            .map_err(cvterr)
            .boxify()
    }
}

impl<E> Manifest for Box<Manifest<Error = E> + Sync>
where
    E: error::Error + Send + 'static,
{
    type Error = E;

    fn lookup(
        &self,
        path: &MPath,
    ) -> BoxFuture<Option<Box<Entry<Error = Self::Error> + Sync>>, Self::Error> {
        (**self).lookup(path)
    }

    fn list(&self) -> BoxStream<Box<Entry<Error = Self::Error> + Sync>, Self::Error> {
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

pub enum Content<E> {
    File(Blob<Vec<u8>>),       // TODO stream
    Executable(Blob<Vec<u8>>), // TODO stream
    Symlink(MPath),
    Tree(Box<Manifest<Error = E> + Sync>),
}

impl<E> Content<E>
where
    E: error::Error + Send + 'static,
{
    fn map_err<ME>(self, cvterr: fn(E) -> ME) -> Content<ME>
    where
        ME: error::Error + Send + 'static,
    {
        match self {
            Content::Tree(m) => Content::Tree(BoxManifest::new_with_cvterr(m, cvterr)),
            Content::File(b) => Content::File(b),
            Content::Executable(b) => Content::Executable(b),
            Content::Symlink(p) => Content::Symlink(p),
        }
    }
}

pub trait Entry: Send + 'static {
    type Error: error::Error + Send + 'static;

    fn get_type(&self) -> Type;
    fn get_parents(&self) -> BoxFuture<Parents, Self::Error>;
    fn get_raw_content(&self) -> BoxFuture<Blob<Vec<u8>>, Self::Error>;
    fn get_content(&self) -> BoxFuture<Content<Self::Error>, Self::Error>;
    fn get_size(&self) -> BoxFuture<Option<usize>, Self::Error>;
    fn get_hash(&self) -> &NodeHash;
    fn get_path(&self) -> &MPath;

    fn boxed(self) -> Box<Entry<Error = Self::Error> + Sync>
    where
        Self: Sync + Sized,
    {
        Box::new(self)
    }
}


pub struct BoxEntry<Ent, E>
where
    Ent: Entry,
{
    entry: Ent,
    cvterr: fn(Ent::Error) -> E,
    _phantom: PhantomData<E>,
}

unsafe impl<Ent, E> Sync for BoxEntry<Ent, E>
where
    Ent: Entry + Sync,
{
}

impl<Ent, E> BoxEntry<Ent, E>
where
    Ent: Entry + Sync + Send + 'static,
    E: error::Error + Send + 'static,
{
    pub fn new(entry: Ent) -> Box<Entry<Error = E> + Sync>
    where
        E: From<Ent::Error>,
    {
        Self::new_with_cvterr(entry, E::from)
    }

    pub fn new_with_cvterr(
        entry: Ent,
        cvterr: fn(Ent::Error) -> E,
    ) -> Box<Entry<Error = E> + Sync> {
        Box::new(BoxEntry {
            entry,
            cvterr,
            _phantom: PhantomData,
        })
    }
}

impl<Ent, E> Entry for BoxEntry<Ent, E>
where
    Ent: Entry + Sync + Send + 'static,
    E: error::Error + Send + 'static,
{
    type Error = E;

    fn get_type(&self) -> Type {
        self.entry.get_type()
    }

    fn get_parents(&self) -> BoxFuture<Parents, Self::Error> {
        self.entry.get_parents().map_err(self.cvterr).boxify()
    }

    fn get_raw_content(&self) -> BoxFuture<Blob<Vec<u8>>, Self::Error> {
        self.entry.get_raw_content().map_err(self.cvterr).boxify()
    }

    fn get_content(&self) -> BoxFuture<Content<Self::Error>, Self::Error> {
        let cvterr = self.cvterr;
        self.entry
            .get_content()
            .map(move |c| Content::map_err(c, cvterr))
            .map_err(self.cvterr)
            .boxify()
    }

    fn get_size(&self) -> BoxFuture<Option<usize>, Self::Error> {
        self.entry.get_size().map_err(self.cvterr).boxify()
    }

    fn get_hash(&self) -> &NodeHash {
        self.entry.get_hash()
    }

    fn get_path(&self) -> &MPath {
        self.entry.get_path()
    }
}

impl<E> Entry for Box<Entry<Error = E> + Sync>
where
    E: error::Error + Send + 'static,
{
    type Error = E;

    fn get_type(&self) -> Type {
        (**self).get_type()
    }

    fn get_parents(&self) -> BoxFuture<Parents, Self::Error> {
        (**self).get_parents()
    }

    fn get_raw_content(&self) -> BoxFuture<Blob<Vec<u8>>, Self::Error> {
        (**self).get_raw_content()
    }

    fn get_content(&self) -> BoxFuture<Content<Self::Error>, Self::Error> {
        (**self).get_content()
    }

    fn get_size(&self) -> BoxFuture<Option<usize>, Self::Error> {
        (**self).get_size()
    }

    fn get_hash(&self) -> &NodeHash {
        (**self).get_hash()
    }

    fn get_path(&self) -> &MPath {
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
