// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::collections::BTreeMap;

use blobnode::Parents;
use nodehash::NodeHash;
use path::MPath;

pub trait Changeset: Send + 'static {
    fn manifestid(&self) -> &NodeHash;
    fn user(&self) -> &[u8];
    fn extra(&self) -> &BTreeMap<Vec<u8>, Vec<u8>>;
    fn comments(&self) -> &[u8];
    fn files(&self) -> &[MPath];
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

    fn files(&self) -> &[MPath] {
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
