// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::marker::PhantomData;

use futures::Stream;
use futures::future::BoxFuture;
use futures::stream;
use futures::stream::BoxStream;

use mercurial_types::{Blob, Entry, Manifest, Path, Type};
use mercurial_types::blobnode::Parents;
use mercurial_types::manifest::Content as MContent;
use mercurial_types::nodehash::NodeHash;

pub struct MockManifest<E> {
    entries: Vec<MockEntry<E>>,
}

impl<E> MockManifest<E> {
    pub fn new(paths: Vec<&'static str>) -> Self {
        let entries = paths
            .into_iter()
            .map(|p| {
                MockEntry::new(Path::new(p).expect(&format!("invalid path {}", p)))
            })
            .collect();
        MockManifest { entries }
    }
}

impl<E> Manifest for MockManifest<E>
where
    E: Send + 'static + ::std::error::Error,
{
    type Error = E;

    fn lookup(
        &self,
        _path: &Path,
    ) -> BoxFuture<Option<Box<Entry<Error = Self::Error> + Sync>>, Self::Error> {
        unimplemented!();
    }
    fn list(&self) -> BoxStream<Box<Entry<Error = Self::Error> + Sync>, Self::Error> {
        stream::iter(self.entries.clone().into_iter().map(|e| Ok(e.boxed()))).boxed()
    }
}

struct MockEntry<E> {
    path: Path,
    phantom: PhantomData<E>,
}

unsafe impl<E> Sync for MockEntry<E> {}

impl<E> Clone for MockEntry<E> {
    fn clone(&self) -> Self {
        MockEntry {
            path: self.path.clone(),
            phantom: PhantomData,
        }
    }
}

impl<E> MockEntry<E> {
    fn new(path: Path) -> Self {
        MockEntry {
            path: path,
            phantom: PhantomData,
        }
    }
}

impl<E> Entry for MockEntry<E>
where
    E: Send + 'static + ::std::error::Error,
{
    type Error = E;
    fn get_type(&self) -> Type {
        unimplemented!();
    }
    fn get_parents(&self) -> BoxFuture<Parents, Self::Error> {
        unimplemented!();
    }
    fn get_raw_content(&self) -> BoxFuture<Blob<Vec<u8>>, Self::Error> {
        unimplemented!();
    }
    fn get_content(&self) -> BoxFuture<MContent<Self::Error>, Self::Error> {
        unimplemented!();
    }
    fn get_size(&self) -> BoxFuture<Option<usize>, Self::Error> {
        unimplemented!();
    }
    fn get_hash(&self) -> &NodeHash {
        unimplemented!();
    }
    fn get_path(&self) -> &Path {
        &self.path
    }
}
