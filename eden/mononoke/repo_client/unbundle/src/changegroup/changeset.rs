/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::ensure;
use anyhow::Result;
use bytes::Bytes;
use futures::stream::Stream;
use futures::stream::TryStreamExt;

use mercurial_bundles::changegroup::CgDeltaChunk;
use mercurial_revlog::changeset::RevlogChangeset;
use mercurial_types::delta;
use mercurial_types::HgBlob;
use mercurial_types::HgBlobNode;
use mercurial_types::HgChangesetId;
use mercurial_types::NULL_HASH;

#[derive(Debug, Eq, PartialEq)]
pub(crate) struct ChangesetDeltaed {
    pub chunk: CgDeltaChunk,
}

pub(crate) fn convert_to_revlog_changesets(
    deltaed: impl Stream<Item = Result<ChangesetDeltaed>>,
) -> impl Stream<Item = Result<(HgChangesetId, RevlogChangeset)>> {
    deltaed.and_then(|ChangesetDeltaed { chunk }| async move {
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
}

#[cfg(test)]
mod tests {
    use super::*;

    use futures::stream::iter;
    use itertools::equal;
    use mercurial_types::HgNodeHash;

    enum CheckResult {
        ExpectedOk(bool),
        ExpectedErr(bool),
    }
    use self::CheckResult::*;

    async fn check_null_changeset(
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

        let result = convert_to_revlog_changesets(iter(vec![Ok(ChangesetDeltaed { chunk })]))
            .try_collect::<Vec<_>>()
            .await;

        if base == NULL_HASH && node == linknode {
            ExpectedOk(equal(result.unwrap(), vec![(HgChangesetId::new(node), cs)]))
        } else {
            ExpectedErr(result.is_err())
        }
    }

    #[quickcheck_async::tokio]
    async fn null_changeset_random(
        node: HgNodeHash,
        linknode: HgNodeHash,
        base: HgNodeHash,
        p1: HgNodeHash,
        p2: HgNodeHash,
    ) -> bool {
        match check_null_changeset(node, linknode, base, p1, p2).await {
            ExpectedOk(true) | ExpectedErr(true) => true,
            _ => false,
        }
    }

    #[quickcheck_async::tokio]
    async fn null_changeset_correct(node: HgNodeHash, p1: HgNodeHash, p2: HgNodeHash) -> bool {
        match check_null_changeset(node.clone(), node, NULL_HASH, p1, p2).await {
            ExpectedOk(true) => true,
            _ => false,
        }
    }
}
