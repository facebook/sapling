/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use serde_derive::{Deserialize, Serialize};

use types::{hgid::HgId, path::RepoPathBuf};

/// Struct reprenting the arguments to a "gettreepack" operation, which
/// is used by Mercurial to prefetch treemanifests. This struct is intended
/// to provide a way to support requests compatible with Mercurial's existing
/// gettreepack wire protocol command.
///
/// In the future, we'd like to migrate away from requesting trees in this way.
/// In general, trees can be requested from the API server using a `TreeRequest`
/// containing the keys of the desired tree nodes.
///
/// In all cases, trees will be returned in a `DataResponse`.
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct CompleteTreeRequest {
    pub rootdir: RepoPathBuf,
    pub mfnodes: Vec<HgId>,
    pub basemfnodes: Vec<HgId>,
    pub depth: Option<usize>,
}

impl CompleteTreeRequest {
    pub fn new(
        rootdir: RepoPathBuf,
        mfnodes: Vec<HgId>,
        basemfnodes: Vec<HgId>,
        depth: Option<usize>,
    ) -> Self {
        Self {
            rootdir,
            mfnodes,
            basemfnodes,
            depth,
        }
    }
}

#[cfg(any(test, feature = "for-tests"))]
use quickcheck::Arbitrary;

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for CompleteTreeRequest {
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> Self {
        Self {
            rootdir: Arbitrary::arbitrary(g),
            mfnodes: Arbitrary::arbitrary(g),
            basemfnodes: Arbitrary::arbitrary(g),
            depth: Arbitrary::arbitrary(g),
        }
    }
}
