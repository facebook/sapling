// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::sync::Arc;

use failure::Error;
use futures::{stream, IntoFuture};
use futures_ext::{BoxFuture, BoxStream, FutureExt, StreamExt};

use mercurial_types::{Blob, Entry, MPath, MPathElement, Manifest, RepoPath, Type};
use mercurial_types::blobnode::Parents;
use mercurial_types::manifest::Content;
use mercurial_types::nodehash::EntryId;

pub type ContentFactory = Arc<Fn() -> Content + Send + Sync>;

pub fn make_file<C: AsRef<str>>(content: C) -> ContentFactory {
    let content = content.as_ref().to_owned().into_bytes();
    Arc::new(move || Content::File(Blob::Dirty(content.clone())))
}

#[derive(Clone)]
pub struct MockManifest {
    entries: Vec<MockEntry>,
}

impl MockManifest {
    fn p(p: &'static str) -> RepoPath {
        // This should also allow directory paths eventually.
        RepoPath::file(p).expect(&format!("invalid path {}", p))
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

    pub fn with_content(content: Vec<(&'static str, ContentFactory, Type)>) -> Self {
        let entries = content
            .into_iter()
            .map(|(p, c, ty)| {
                let mut mock_entry = MockEntry::new(Self::p(p), c);
                mock_entry.set_type(ty);
                mock_entry
            })
            .collect();
        MockManifest { entries }
    }
}

impl Manifest for MockManifest {
    fn lookup(&self, _path: &MPath) -> BoxFuture<Option<Box<Entry + Sync>>, Error> {
        unimplemented!();
    }
    fn list(&self) -> BoxStream<Box<Entry + Sync>, Error> {
        stream::iter_ok(self.entries.clone().into_iter().map(|e| e.boxed())).boxify()
    }
}

pub struct MockEntry {
    path: RepoPath,
    name: Option<MPathElement>,
    content_factory: ContentFactory,
    ty: Option<Type>,
    hash: Option<EntryId>,
}

impl Clone for MockEntry {
    fn clone(&self) -> Self {
        MockEntry {
            path: self.path.clone(),
            name: self.name.clone(),
            content_factory: self.content_factory.clone(),
            ty: self.ty.clone(),
            hash: self.hash.clone(),
        }
    }
}

impl MockEntry {
    pub fn new(path: RepoPath, content_factory: ContentFactory) -> Self {
        let name = match path.clone() {
            RepoPath::RootPath => None,
            RepoPath::FilePath(path) | RepoPath::DirectoryPath(path) => {
                path.clone().into_iter().next_back()
            }
        };
        MockEntry {
            path,
            name,
            content_factory,
            ty: None,
            hash: None,
        }
    }

    pub fn set_type(&mut self, ty: Type) {
        self.ty = Some(ty);
    }

    pub fn set_hash(&mut self, hash: EntryId) {
        self.hash = Some(hash);
    }
}

impl Entry for MockEntry {
    fn get_type(&self) -> Type {
        self.ty.expect("ty is not set!")
    }
    fn get_parents(&self) -> BoxFuture<Parents, Error> {
        unimplemented!();
    }
    fn get_raw_content(&self) -> BoxFuture<Blob<Vec<u8>>, Error> {
        unimplemented!();
    }
    fn get_content(&self) -> BoxFuture<Content, Error> {
        Ok((self.content_factory)()).into_future().boxify()
    }
    fn get_size(&self) -> BoxFuture<Option<usize>, Error> {
        unimplemented!();
    }
    fn get_hash(&self) -> &EntryId {
        match self.hash {
            Some(ref hash) => hash,
            None => panic!("hash is not set!"),
        }
    }
    fn get_name(&self) -> &Option<MPathElement> {
        &self.name
    }
}
