/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::ErrorKind;
use crate::FastlogParent;
use anyhow::Error;
use blobstore::Blobstore;
use blobstore::BlobstoreBytes;
use context::CoreContext;
use futures::future::try_join_all;
use manifest::Entry;
use maplit::hashset;
use mononoke_types::fastlog_batch::FastlogBatch;
use mononoke_types::fastlog_batch::ParentOffset;
use mononoke_types::ChangesetId;
use mononoke_types::FileUnodeId;
use mononoke_types::ManifestUnodeId;
use std::collections::HashMap;
use std::collections::HashSet;
use std::collections::VecDeque;
use std::sync::Arc;

pub(crate) async fn create_new_batch(
    ctx: &CoreContext,
    blobstore: &Arc<dyn Blobstore>,
    unode_parents: Vec<Entry<ManifestUnodeId, FileUnodeId>>,
    linknode: ChangesetId,
) -> Result<FastlogBatch, Error> {
    let parent_batches = try_join_all(unode_parents.clone().into_iter().map({
        move |entry| async move {
            let maybe_batch = fetch_fastlog_batch_by_unode_id(ctx, blobstore, &entry).await?;
            maybe_batch.ok_or(Error::from(ErrorKind::NotFound(entry)))
        }
    }))
    .await?;

    if parent_batches.len() < 2 {
        match parent_batches.get(0) {
            Some(parent_batch) => {
                parent_batch
                    .prepend_child_with_single_parent(ctx, blobstore, linknode)
                    .await
            }
            None => {
                let mut d = VecDeque::new();
                d.push_back((linknode, vec![]));
                FastlogBatch::new_from_raw_list(ctx, &*blobstore, d).await
            }
        }
    } else {
        let parents_flattened =
            try_join_all(parent_batches.into_iter().map({
                move |batch| async move { fetch_flattened(&batch, ctx, blobstore).await }
            }))
            .await?;
        let raw_list = convert_to_raw_list(create_merged_list(linknode, parents_flattened));
        FastlogBatch::new_from_raw_list(ctx, &*blobstore, raw_list).await
    }
}

// This function creates a FastlogBatch list for a merge unode.
// It does so by taking a merge_cs_id (i.e. a linknode of this merge unode) and
// FastlogBatches for it's parents and merges them together in BFS order
//
// For example, let's say we have a unode whose history graph is the following:
//
//             o <- commit A
//            / \
// commit B  o   \
//           \   o <- commit C
//            \ /
//             o <- commit D
//
// create_merged_list() accepts commit A as merge_cs_id, [B, D] as a first parent's list
// and [C, D] as the second parent's list. The expected output is [A, B, C, D].
fn create_merged_list(
    merge_cs_id: ChangesetId,
    parents_lists: Vec<Vec<(ChangesetId, Vec<FastlogParent>)>>,
) -> Vec<(ChangesetId, Vec<FastlogParent>)> {
    // parents_of_merge_commits preserve the order of `parents_lists`
    let mut parents_of_merge_commit = vec![];
    for list in parents_lists.iter() {
        if let Some((p, _)) = list.get(0) {
            parents_of_merge_commit.push(FastlogParent::Known(*p));
        }
    }
    {
        // Make sure we have unique parents
        let mut used = HashSet::new();
        parents_of_merge_commit.retain(move |p| used.insert(p.clone()));
    }

    let mut cs_id_to_parents: HashMap<_, _> = parents_lists
        .into_iter()
        .flat_map(|list| list.into_iter())
        .collect();
    cs_id_to_parents.insert(merge_cs_id, parents_of_merge_commit.clone());

    let mut q = VecDeque::new();
    q.push_back((merge_cs_id, parents_of_merge_commit));

    let mut res = vec![];
    let mut used = hashset! {merge_cs_id};
    while let Some((cs_id, parents)) = q.pop_front() {
        res.push((cs_id, parents.clone()));

        for p in parents {
            if let FastlogParent::Known(p) = p {
                if let Some(parents) = cs_id_to_parents.get(&p) {
                    if used.insert(p) {
                        q.push_back((p, parents.clone()));
                    }
                }
            }
        }
    }

    res
}

// Converts from an "external" representation (i.e. the one used by users of this library)
// to an "internal" representation (i.e. the one that we store in the blobstore).
fn convert_to_raw_list(
    list: Vec<(ChangesetId, Vec<FastlogParent>)>,
) -> Vec<(ChangesetId, Vec<ParentOffset>)> {
    let cs_to_idx: HashMap<_, _> = list
        .iter()
        .enumerate()
        .map(|(idx, (cs_id, _))| (*cs_id, idx as i32))
        .collect();

    // Special offset that points outside of the list.
    // It's used for unknown parents
    let max_idx = (list.len() + 1) as i32;
    let mut res = vec![];
    for (current_idx, (cs_id, fastlog_parents)) in list.into_iter().enumerate() {
        let current_idx = current_idx as i32;
        let mut parent_offsets = vec![];
        for p in fastlog_parents {
            let maybe_idx = match p {
                FastlogParent::Known(cs_id) => {
                    cs_to_idx.get(&cs_id).cloned().map(|idx| idx - current_idx)
                }
                FastlogParent::Unknown => None,
            };

            parent_offsets.push(ParentOffset::new(maybe_idx.unwrap_or(max_idx)))
        }
        res.push((cs_id, parent_offsets));
    }

    res
}

pub async fn fetch_fastlog_batch_by_unode_id<B: Blobstore>(
    ctx: &CoreContext,
    blobstore: &B,
    unode_entry: &Entry<ManifestUnodeId, FileUnodeId>,
) -> Result<Option<FastlogBatch>, Error> {
    let fastlog_batch_key = unode_entry_to_fastlog_batch_key(unode_entry);

    let maybe_bytes = blobstore.get(ctx, &fastlog_batch_key).await?;

    match maybe_bytes {
        Some(serialized) => FastlogBatch::from_bytes(serialized.as_raw_bytes()).map(Some),
        None => Ok(None),
    }
}

pub(crate) async fn save_fastlog_batch_by_unode_id<B: Blobstore>(
    ctx: &CoreContext,
    blobstore: &B,
    unode_entry: Entry<ManifestUnodeId, FileUnodeId>,
    batch: FastlogBatch,
) -> Result<(), Error> {
    let fastlog_batch_key = unode_entry_to_fastlog_batch_key(&unode_entry);
    let serialized = batch.into_bytes();

    blobstore
        .put(
            ctx,
            fastlog_batch_key,
            BlobstoreBytes::from_bytes(serialized),
        )
        .await
}

pub fn unode_entry_to_fastlog_batch_key(
    unode_entry: &Entry<ManifestUnodeId, FileUnodeId>,
) -> String {
    let key_part = match unode_entry {
        Entry::Leaf(file_unode_id) => format!("fileunode.{}", file_unode_id),
        Entry::Tree(mf_unode_id) => format!("manifestunode.{}", mf_unode_id),
    };
    format!("fastlogbatch.{}", key_part)
}

pub async fn fetch_flattened<B: Blobstore>(
    batch: &FastlogBatch,
    ctx: &CoreContext,
    blobstore: &B,
) -> Result<Vec<(ChangesetId, Vec<FastlogParent>)>, Error> {
    let raw_list = batch.fetch_raw_list(ctx, blobstore).await?;
    Ok(flatten_raw_list(raw_list))
}

fn flatten_raw_list(
    raw_list: Vec<(ChangesetId, Vec<ParentOffset>)>,
) -> Vec<(ChangesetId, Vec<FastlogParent>)> {
    let mut res = vec![];
    for (index, (cs_id, parent_offsets)) in raw_list.iter().enumerate() {
        let mut batch_parents = vec![];
        for offset in parent_offsets {
            // NOTE: Offset can be negative!
            let parent_index = index as i32 + offset.num();
            let batch_parent = if parent_index >= 0 {
                match raw_list.get(parent_index as usize) {
                    Some((p_cs_id, _)) => FastlogParent::Known(*p_cs_id),
                    None => FastlogParent::Unknown,
                }
            } else {
                FastlogParent::Unknown
            };
            batch_parents.push(batch_parent);
        }

        res.push((*cs_id, batch_parents));
    }

    res
}

#[cfg(test)]
mod test {
    use super::*;
    use borrowed::borrowed;
    use fbinit::FacebookInit;
    use fixtures::Linear;
    use fixtures::TestRepoFixture;
    use mononoke_types_mocks::changesetid::ONES_CSID;
    use mononoke_types_mocks::changesetid::THREES_CSID;
    use mononoke_types_mocks::changesetid::TWOS_CSID;

    #[fbinit::test]
    async fn fetch_flattened_simple(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo = Linear::getrepo(fb).await;
        let blobstore = repo.blobstore();
        borrowed!(ctx);
        let mut d = VecDeque::new();
        d.push_back((ONES_CSID, vec![]));
        let batch = FastlogBatch::new_from_raw_list(ctx, blobstore, d).await?;

        assert_eq!(
            vec![(ONES_CSID, vec![])],
            fetch_flattened(&batch, ctx, blobstore).await.unwrap()
        );
        Ok(())
    }

    #[fbinit::test]
    async fn fetch_flattened_prepend(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo = Linear::getrepo(fb).await;
        let blobstore = repo.blobstore();
        borrowed!(ctx);
        let mut d = VecDeque::new();
        d.push_back((ONES_CSID, vec![]));
        let batch = FastlogBatch::new_from_raw_list(ctx, blobstore, d).await?;

        assert_eq!(
            vec![(ONES_CSID, vec![])],
            fetch_flattened(&batch, ctx, blobstore).await.unwrap()
        );

        let prepended = batch
            .prepend_child_with_single_parent(ctx, blobstore, TWOS_CSID)
            .await
            .unwrap();
        assert_eq!(
            vec![
                (TWOS_CSID, vec![FastlogParent::Known(ONES_CSID)]),
                (ONES_CSID, vec![])
            ],
            fetch_flattened(&prepended, ctx, blobstore).await.unwrap()
        );

        let prepended = prepended
            .prepend_child_with_single_parent(ctx, &blobstore, THREES_CSID)
            .await
            .unwrap();
        assert_eq!(
            vec![
                (THREES_CSID, vec![FastlogParent::Known(TWOS_CSID)]),
                (TWOS_CSID, vec![FastlogParent::Known(ONES_CSID)]),
                (ONES_CSID, vec![])
            ],
            fetch_flattened(&prepended, ctx, blobstore).await.unwrap()
        );

        Ok(())
    }

    #[test]
    fn test_create_merged_list() -> Result<(), Error> {
        assert_eq!(
            create_merged_list(ONES_CSID, vec![]),
            vec![(ONES_CSID, vec![])]
        );

        let first_parent = vec![(TWOS_CSID, vec![])];
        let second_parent = vec![(THREES_CSID, vec![])];
        assert_eq!(
            create_merged_list(ONES_CSID, vec![first_parent, second_parent]),
            vec![
                (
                    ONES_CSID,
                    vec![
                        FastlogParent::Known(TWOS_CSID),
                        FastlogParent::Known(THREES_CSID)
                    ]
                ),
                (TWOS_CSID, vec![]),
                (THREES_CSID, vec![]),
            ]
        );
        Ok(())
    }

    #[test]
    fn test_create_merged_list_same_commit() -> Result<(), Error> {
        assert_eq!(
            create_merged_list(ONES_CSID, vec![]),
            vec![(ONES_CSID, vec![])]
        );

        let first_parent = vec![(TWOS_CSID, vec![])];
        let second_parent = vec![(TWOS_CSID, vec![])];
        assert_eq!(
            create_merged_list(ONES_CSID, vec![first_parent, second_parent]),
            vec![
                (ONES_CSID, vec![FastlogParent::Known(TWOS_CSID),]),
                (TWOS_CSID, vec![]),
            ]
        );
        Ok(())
    }

    #[test]
    fn test_convert_to_raw_list_simple() -> Result<(), Error> {
        let list = vec![
            (
                ONES_CSID,
                vec![
                    FastlogParent::Known(TWOS_CSID),
                    FastlogParent::Known(THREES_CSID),
                ],
            ),
            (TWOS_CSID, vec![]),
            (THREES_CSID, vec![]),
        ];

        let raw_list = convert_to_raw_list(list.clone());
        let expected = vec![
            (ONES_CSID, vec![ParentOffset::new(1), ParentOffset::new(2)]),
            (TWOS_CSID, vec![]),
            (THREES_CSID, vec![]),
        ];
        assert_eq!(raw_list, expected);
        assert_eq!(flatten_raw_list(raw_list), list);

        let list = vec![
            (ONES_CSID, vec![FastlogParent::Known(TWOS_CSID)]),
            (TWOS_CSID, vec![FastlogParent::Known(THREES_CSID)]),
            (THREES_CSID, vec![]),
        ];

        let raw_list = convert_to_raw_list(list.clone());
        let expected = vec![
            (ONES_CSID, vec![ParentOffset::new(1)]),
            (TWOS_CSID, vec![ParentOffset::new(1)]),
            (THREES_CSID, vec![]),
        ];
        assert_eq!(raw_list, expected);
        assert_eq!(flatten_raw_list(raw_list), list);

        Ok(())
    }
}
