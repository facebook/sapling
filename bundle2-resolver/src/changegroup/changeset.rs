// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use futures::Stream;
use futures_ext::{BoxStream, StreamExt};

use bytes::Bytes;
use mercurial::changeset::RevlogChangeset;
use mercurial_bundles::changegroup::CgDeltaChunk;
use mercurial_types::{delta, HgBlob, HgBlobNode, HgNodeHash, NULL_HASH};

use errors::*;

#[derive(Debug, Eq, PartialEq)]
pub struct ChangesetDeltaed {
    pub chunk: CgDeltaChunk,
}

pub fn convert_to_revlog_changesets<S>(
    deltaed: S,
) -> BoxStream<(HgNodeHash, RevlogChangeset), Error>
where
    S: Stream<Item = ChangesetDeltaed, Error = Error> + Send + 'static,
{
    deltaed
        .and_then(|ChangesetDeltaed { chunk }| {
            ensure_msg!(
                chunk.base == NULL_HASH,
                "Changeset chunk base ({:?}) should be equal to root commit ({:?}), \
                 because it is never deltaed",
                chunk.base,
                NULL_HASH
            );
            ensure_msg!(
                chunk.node == chunk.linknode,
                "Changeset chunk node ({:?}) should be equal to linknode ({:?})",
                chunk.node,
                chunk.linknode
            );

            Ok((
                chunk.node,
                RevlogChangeset::new(HgBlobNode::new(
                    HgBlob::from(Bytes::from(delta::apply(b"", &chunk.delta)?)),
                    chunk.p1.into_option().as_ref(),
                    chunk.p2.into_option().as_ref(),
                ))?,
            ))
        })
        .boxify()
}

#[cfg(test)]
mod tests {
    use super::*;

    use futures::Future;
    use futures::stream::iter_ok;
    use itertools::equal;

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
            p1.into_option().as_ref(),
            p2.into_option().as_ref(),
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
        };

        let result = convert_to_revlog_changesets(iter_ok(vec![ChangesetDeltaed { chunk }]))
            .collect()
            .wait();

        if base == NULL_HASH && node == linknode {
            ExpectedOk(equal(result.unwrap(), vec![(node, cs)]))
        } else {
            ExpectedErr(result.is_err())
        }
    }

    quickcheck!{
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
