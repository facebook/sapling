/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use bytes::Bytes;
use failure_ext::ensure;
use futures::Stream;
use futures_ext::{BoxStream, StreamExt};

use mercurial_bundles::changegroup::CgDeltaChunk;
use mercurial_revlog::changeset::RevlogChangeset;
use mercurial_types::{delta, HgBlob, HgBlobNode, HgChangesetId, NULL_HASH};

use crate::errors::*;

#[derive(Debug, Eq, PartialEq)]
pub struct ChangesetDeltaed {
    pub chunk: CgDeltaChunk,
}

pub fn convert_to_revlog_changesets<S>(
    deltaed: S,
) -> BoxStream<(HgChangesetId, RevlogChangeset), Error>
where
    S: Stream<Item = ChangesetDeltaed, Error = Error> + Send + 'static,
{
    deltaed
        .and_then(|ChangesetDeltaed { chunk }| {
            ensure!(
                chunk.base == NULL_HASH,
                "Changeset chunk base ({:?}) should be equal to root commit ({:?}), \
                 because it is never deltaed",
                chunk.base,
                NULL_HASH
            );
            ensure!(
                chunk.node == chunk.linknode,
                "Changeset chunk node ({:?}) should be equal to linknode ({:?})",
                chunk.node,
                chunk.linknode
            );

            Ok((
                HgChangesetId::new(chunk.node),
                RevlogChangeset::new(HgBlobNode::new(
                    HgBlob::from(Bytes::from(delta::apply(b"", &chunk.delta)?)),
                    chunk.p1.into_option(),
                    chunk.p2.into_option(),
                ))?,
            ))
        })
        .boxify()
}

#[cfg(test)]
mod tests {
    use super::*;

    use futures::stream::iter_ok;
    use futures::Future;
    use itertools::equal;
    use mercurial_types::HgNodeHash;
    use quickcheck::quickcheck;

    enum CheckResult {
        ExpectedOk(bool),
        ExpectedErr(bool),
    }
    use self::CheckResult::*;

    fn check_null_changeset(
        node: HgNodeHash,
        linknode: HgNodeHash,
        base: HgNodeHash,
        p1: HgNodeHash,
        p2: HgNodeHash,
    ) -> CheckResult {
        let blobnode = HgBlobNode::new(
            RevlogChangeset::new_null()
                .get_node()
                .unwrap()
                .as_blob()
                .clone(),
            p1.into_option(),
            p2.into_option(),
        );

        let delta = delta::Delta::new_fulltext(blobnode.as_blob().as_slice());
        let cs = RevlogChangeset::new(blobnode).unwrap();

        let chunk = CgDeltaChunk {
            node,
            p1,
            p2,
            base,
            linknode,
            delta,
            flags: None,
        };

        let result = convert_to_revlog_changesets(iter_ok(vec![ChangesetDeltaed { chunk }]))
            .collect()
            .wait();

        if base == NULL_HASH && node == linknode {
            ExpectedOk(equal(result.unwrap(), vec![(HgChangesetId::new(node), cs)]))
        } else {
            ExpectedErr(result.is_err())
        }
    }

    quickcheck! {
        fn null_changeset_random(
            node: HgNodeHash,
            linknode: HgNodeHash,
            base: HgNodeHash,
            p1: HgNodeHash,
            p2: HgNodeHash
        ) -> bool {
            match check_null_changeset(node, linknode, base, p1, p2) {
                ExpectedOk(true) | ExpectedErr(true) => true,
                _ => false
            }
        }

        fn null_changeset_correct(node: HgNodeHash, p1: HgNodeHash, p2: HgNodeHash) -> bool {
            match check_null_changeset(node.clone(), node, NULL_HASH, p1, p2) {
                ExpectedOk(true) => true,
                _ => false
            }
        }
    }
}
