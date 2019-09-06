// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use crate::{ErrorKind, FastlogParent};
use blobstore::{Blobstore, BlobstoreBytes};
use cloned::cloned;
use context::CoreContext;
use failure_ext::Error;
use futures::{future, Future};
use futures_ext::{BoxFuture, FutureExt};
use manifest::Entry;
use mononoke_types::{fastlog_batch::FastlogBatch, ChangesetId, FileUnodeId, ManifestUnodeId};
use std::collections::VecDeque;
use std::sync::Arc;

pub(crate) fn create_new_batch(
    ctx: CoreContext,
    blobstore: Arc<dyn Blobstore>,
    unode_parents: Vec<Entry<ManifestUnodeId, FileUnodeId>>,
    linknode: ChangesetId,
) -> impl Future<Item = FastlogBatch, Error = Error> {
    let f = future::join_all(unode_parents.clone().into_iter().map({
        cloned!(ctx, blobstore);
        move |entry| {
            fetch_fastlog_batch_by_unode_id(ctx.clone(), blobstore.clone(), entry)
                .and_then(move |maybe_batch| maybe_batch.ok_or(ErrorKind::NotFound(entry).into()))
        }
    }));

    f.and_then(move |parent_batches| {
        if parent_batches.len() < 2 {
            match parent_batches.get(0) {
                Some(parent_batch) => parent_batch
                    .prepend_child_with_single_parent(ctx, blobstore, linknode)
                    .left_future(),
                None => {
                    let mut d = VecDeque::new();
                    d.push_back((linknode, vec![]));
                    FastlogBatch::new_from_raw_list(ctx, blobstore, d).right_future()
                }
            }
        } else {
            // TODO(stash): handle merges as well
            unimplemented!()
        }
    })
}

pub(crate) fn fetch_fastlog_batch_by_unode_id(
    ctx: CoreContext,
    blobstore: Arc<dyn Blobstore>,
    unode_entry: Entry<ManifestUnodeId, FileUnodeId>,
) -> impl Future<Item = Option<FastlogBatch>, Error = Error> {
    let fastlog_batch_key = generate_fastlog_batch_key(unode_entry);

    blobstore
        .get(ctx, fastlog_batch_key.clone())
        .and_then(move |maybe_bytes| match maybe_bytes {
            Some(serialized) => FastlogBatch::from_bytes(serialized.as_bytes()).map(Some),
            None => Ok(None),
        })
}

pub(crate) fn save_fastlog_batch_by_unode_id(
    ctx: CoreContext,
    blobstore: Arc<dyn Blobstore>,
    unode_entry: Entry<ManifestUnodeId, FileUnodeId>,
    batch: FastlogBatch,
) -> BoxFuture<(), Error> {
    let fastlog_batch_key = generate_fastlog_batch_key(unode_entry);
    let serialized = batch.into_bytes();

    blobstore.put(
        ctx,
        fastlog_batch_key,
        BlobstoreBytes::from_bytes(serialized),
    )
}

fn generate_fastlog_batch_key(unode_entry: Entry<ManifestUnodeId, FileUnodeId>) -> String {
    let key_part = match unode_entry {
        Entry::Leaf(file_unode_id) => format!("fileunode.{}", file_unode_id),
        Entry::Tree(mf_unode_id) => format!("manifestunode.{}", mf_unode_id),
    };
    format!("fastlogbatch.{}", key_part)
}

pub(crate) fn fetch_flattened(
    batch: &FastlogBatch,
    ctx: CoreContext,
    blobstore: Arc<dyn Blobstore>,
) -> impl Future<Item = Vec<(ChangesetId, Vec<FastlogParent>)>, Error = Error> {
    batch.fetch_raw_list(ctx, blobstore).map(|full_batch| {
        let mut res = vec![];
        for (index, (cs_id, parent_offsets)) in full_batch.iter().enumerate() {
            let mut batch_parents = vec![];
            for offset in parent_offsets {
                let parent_index = index + offset.num() as usize;
                let batch_parent = match full_batch.get(parent_index) {
                    Some((p_cs_id, _)) => FastlogParent::Known(*p_cs_id),
                    None => FastlogParent::Unknown,
                };
                batch_parents.push(batch_parent);
            }

            res.push((*cs_id, batch_parents));
        }

        res
    })
}

#[cfg(test)]
mod test {
    use super::*;
    use fixtures::linear;
    use mononoke_types_mocks::changesetid::{ONES_CSID, THREES_CSID, TWOS_CSID};
    use tokio::runtime::Runtime;

    #[test]
    fn fetch_flattened_simple() -> Result<(), Error> {
        let ctx = CoreContext::test_mock();
        let repo = linear::getrepo();
        let mut rt = Runtime::new().unwrap();
        let mut d = VecDeque::new();
        d.push_back((ONES_CSID, vec![]));
        let blobstore = Arc::new(repo.get_blobstore());
        let batch = rt.block_on(FastlogBatch::new_from_raw_list(
            ctx.clone(),
            blobstore.clone(),
            d,
        ))?;

        assert_eq!(
            vec![(ONES_CSID, vec![])],
            rt.block_on(fetch_flattened(&batch, ctx, blobstore))
                .unwrap()
        );
        Ok(())
    }

    #[test]
    fn fetch_flattened_prepend() -> Result<(), Error> {
        let ctx = CoreContext::test_mock();
        let repo = linear::getrepo();
        let mut rt = Runtime::new().unwrap();
        let mut d = VecDeque::new();
        d.push_back((ONES_CSID, vec![]));
        let blobstore = Arc::new(repo.get_blobstore());
        let batch = rt.block_on(FastlogBatch::new_from_raw_list(
            ctx.clone(),
            blobstore.clone(),
            d,
        ))?;

        assert_eq!(
            vec![(ONES_CSID, vec![])],
            rt.block_on(fetch_flattened(&batch, ctx.clone(), blobstore.clone()))
                .unwrap()
        );

        let prepended = rt
            .block_on(batch.prepend_child_with_single_parent(
                ctx.clone(),
                blobstore.clone(),
                TWOS_CSID,
            ))
            .unwrap();
        assert_eq!(
            vec![
                (TWOS_CSID, vec![FastlogParent::Known(ONES_CSID)]),
                (ONES_CSID, vec![])
            ],
            rt.block_on(fetch_flattened(&prepended, ctx.clone(), blobstore.clone()))
                .unwrap()
        );

        let prepended = rt
            .block_on(prepended.prepend_child_with_single_parent(
                ctx.clone(),
                blobstore.clone(),
                THREES_CSID,
            ))
            .unwrap();
        assert_eq!(
            vec![
                (THREES_CSID, vec![FastlogParent::Known(TWOS_CSID)]),
                (TWOS_CSID, vec![FastlogParent::Known(ONES_CSID)]),
                (ONES_CSID, vec![])
            ],
            rt.block_on(fetch_flattened(&prepended, ctx, blobstore))
                .unwrap()
        );

        Ok(())
    }
}
