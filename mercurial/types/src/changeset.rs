/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use std::collections::BTreeMap;

use mononoke_types::{DateTime, MPath};

use crate::blobnode::HgParents;
use crate::nodehash::HgManifestId;

pub trait Changeset: Send + 'static {
    fn manifestid(&self) -> HgManifestId;
    fn user(&self) -> &[u8];
    fn extra(&self) -> &BTreeMap<Vec<u8>, Vec<u8>>;
    fn comments(&self) -> &[u8];
    fn files(&self) -> &[MPath];
    fn time(&self) -> &DateTime;
    // XXX Change this to return p1 and p2 directly.
    fn parents(&self) -> HgParents;

    fn boxed(self) -> Box<dyn Changeset>
    where
        Self: Sized,
    {
        Box::new(self)
    }
}

impl Changeset for Box<dyn Changeset> {
    fn manifestid(&self) -> HgManifestId {
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

    fn time(&self) -> &DateTime {
        (**self).time()
    }

    fn parents(&self) -> HgParents {
        (**self).parents()
    }
}
