// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::sync::Arc;

use failure::Error;
use futures::{stream, IntoFuture};
use futures_ext::{BoxFuture, BoxStream, FutureExt, StreamExt};

use mercurial_types::{Blob, Entry, MPath, Manifest, RepoPath, Type};
use mercurial_types::blobnode::Parents;
use mercurial_types::manifest::Content;
use mercurial_types::nodehash::NodeHash;

type ContentFactory = Arc<Fn() -> Content + Send + Sync>;

pub fn make_file<C: AsRef<str>>(content: C) -> ContentFactory {
    let content = content.as_ref().to_owned().into_bytes();
    Arc::new(move || Content::File(Blob::Dirty(content.clone())))
}

pub struct MockManifest {
    entries: Vec<MockEntry>,
}

impl MockManifest {
    fn p(p: &'static str) -> RepoPath {
        // This should also allow directory paths eventually.
        RepoPath::file(p.as_ref()).expect(&format!("invalid path {}", p))
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

    pub fn with_content(content: Vec<(&'static str, ContentFactory)>) -> Self {
        let entries = content
            .into_iter()
            .map(|(p, c)| MockEntry::new(Self::p(p), c))
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

struct MockEntry {
    path: RepoPath,
    content_factory: ContentFactory,
}

impl Clone for MockEntry {
    fn clone(&self) -> Self {
        MockEntry {
            path: self.path.clone(),
            content_factory: self.content_factory.clone(),
        }
    }
}

impl MockEntry {
    fn new(path: RepoPath, content_factory: ContentFactory) -> Self {
        MockEntry {
            path,
            content_factory,
        }
    }
}

impl Entry for MockEntry {
    fn get_type(&self) -> Type {
        unimplemented!();
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
    fn get_hash(&self) -> &NodeHash {
        unimplemented!();
    }
    fn get_path(&self) -> &RepoPath {
        &self.path
    }
    fn get_mpath(&self) -> &MPath {
        self.path
            .mpath()
            .expect("entries should always have an associated path")
    }
}
