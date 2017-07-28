// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::collections::BTreeMap;
use std::ops::Deref;

use futures::future::{BoxFuture, Future};
use futures::stream::{self, BoxStream, Stream};

use repo::Repo;
use node::Node;
use nodehash::NodeHash;
use blobnode::Parents;
use path::Path;

pub trait Changeset: Send + 'static {
    fn manifestid(&self) -> &NodeHash;
    fn user(&self) -> &[u8];
    fn extra(&self) -> &BTreeMap<Vec<u8>, Vec<u8>>;
    fn comments(&self) -> &[u8];
    fn files(&self) -> &[Path];
    fn time(&self) -> &Time;
    fn parents(&self) -> &Parents;

    fn boxed(self) -> Box<Changeset>
    where
        Self: Sized,
    {
        Box::new(self)
    }
}

impl Changeset for Box<Changeset> {
    fn manifestid(&self) -> &NodeHash {
        (**self).manifestid()
    }

    fn user(&self) -> &[u8] {
        (**self).user()
    }

    fn extra(&self) -> &BTreeMap<Vec<u8>, Vec<u8>> {
        (**self).extra()
    }

    fn comments(&self) -> &[u8] {
        (**self).comments()
    }

    fn files(&self) -> &[Path] {
        (**self).files()
    }

    fn time(&self) -> &Time {
        (**self).time()
    }

    fn parents(&self) -> &Parents {
        (**self).parents()
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct Time {
    pub time: u64,
    pub tz: i32,
}

#[derive(Debug)]
pub struct RepoChangeset<R> {
    repo: R,
    csid: NodeHash,
}

impl<R> RepoChangeset<R> {
    pub fn new(repo: R, csid: NodeHash) -> Self {
        Self { repo, csid }
    }

    pub fn get_csid(&self) -> &NodeHash {
        &self.csid
    }
}

impl<R> AsRef<NodeHash> for RepoChangeset<R> {
    fn as_ref(&self) -> &NodeHash {
        self.get_csid()
    }
}

impl<R> Deref for RepoChangeset<R> {
    type Target = R;

    fn deref(&self) -> &Self::Target {
        &self.repo
    }
}

impl<R> Node for RepoChangeset<R>
where
    R: Repo + Clone + Send,
{
    type Content = Box<Changeset>;
    type Error = R::Error;

    type GetParents = BoxStream<Self, Self::Error>;
    type GetContent = BoxFuture<Self::Content, Self::Error>;

    fn get_parents(&self) -> Self::GetParents {
        self.repo.get_changeset_by_nodeid(&self.csid) // Future<Changeset>
            .map(|cs| stream::iter(cs.parents().into_iter().map(|p| Ok(p)))) // Future<Stream<>>
            .flatten_stream() // Stream<NodeHash>
            .map({
                let repo = self.repo.clone();
                move |p| Self::new(repo.clone(), p)
            }) // Stream<Self>
            .boxed()
    }

    fn get_content(&self) -> Self::GetContent {
        self.repo.get_changeset_by_nodeid(&self.csid).boxed()
    }
}
