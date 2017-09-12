// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::marker::PhantomData;
use std::sync::Arc;

use futures::{Future, IntoFuture, Stream};
use futures::future::BoxFuture;
use futures::stream::{self, BoxStream};

use mercurial_types::{Blob, Entry, Manifest, Path, Type};
use mercurial_types::blobnode::Parents;
use mercurial_types::manifest::Content;
use mercurial_types::nodehash::NodeHash;

type ContentFactory<E> = Arc<Fn() -> Content<E> + Send + Sync>;

pub fn make_file<C: AsRef<str>, E>(content: C) -> ContentFactory<E> {
    let content = content.as_ref().to_owned().into_bytes();
    Arc::new(move || Content::File(Blob::Dirty(content.clone())))
}

pub struct MockManifest<E> {
    entries: Vec<MockEntry<E>>,
}

impl<E> MockManifest<E> {
    fn p(p: &'static str) -> Path {
        Path::new(p).expect(&format!("invalid path {}", p))
    }

    pub fn new(paths: Vec<&'static str>) -> Self {
        let entries = paths
            .into_iter()
            .map(|p| {
                MockEntry::new(
                    Self::p(p),
                    Arc::new(move || {
                        panic!("This MockEntry(path: {:?}) was not created with content", p)
                    }),
                )
            })
            .collect();
        MockManifest { entries }
    }

    pub fn with_content(content: Vec<(&'static str, ContentFactory<E>)>) -> Self {
        let entries = content
            .into_iter()
            .map(|(p, c)| MockEntry::new(Self::p(p), c))
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
    content_factory: ContentFactory<E>,
    phantom: PhantomData<E>,
}

unsafe impl<E> Sync for MockEntry<E> {}

impl<E> Clone for MockEntry<E> {
    fn clone(&self) -> Self {
        MockEntry {
            path: self.path.clone(),
            content_factory: self.content_factory.clone(),
            phantom: PhantomData,
        }
    }
}

impl<E> MockEntry<E> {
    fn new(path: Path, content_factory: ContentFactory<E>) -> Self {
        MockEntry {
            path,
            content_factory,
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
    fn get_content(&self) -> BoxFuture<Content<Self::Error>, Self::Error> {
        Ok((self.content_factory)()).into_future().boxed()
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
