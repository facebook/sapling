/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use async_trait::async_trait;
use blobstore::KeyedBlobstore;
use context::CoreContext;
use futures::stream::BoxStream;
use futures::stream::StreamExt;
use mononoke_types::MPathElement;
use mononoke_types::history_manifest::HistoryManifestDirectory;
use mononoke_types::history_manifest::HistoryManifestEntry;
use mononoke_types::sharded_map_v2::LoadableShardedMapV2Node;
use mononoke_types::typed_hash::HistoryManifestDirectoryId;
use mononoke_types::typed_hash::HistoryManifestFileId;

use super::Entry;
use super::Manifest;

pub(crate) fn history_manifest_to_mf_entry(
    entry: HistoryManifestEntry,
) -> Option<Entry<HistoryManifestDirectoryId, HistoryManifestFileId>> {
    match entry {
        HistoryManifestEntry::File(id) => Some(Entry::Leaf(id)),
        HistoryManifestEntry::Directory(id) => Some(Entry::Tree(id)),
        // We exclude deleted nodes from the Manifest implementation.
        HistoryManifestEntry::DeletedNode(_) => None,
    }
}

#[async_trait]
impl<Store: KeyedBlobstore> Manifest<Store> for HistoryManifestDirectory {
    type TreeId = HistoryManifestDirectoryId;
    type Leaf = HistoryManifestFileId;
    type TrieMapType = LoadableShardedMapV2Node<HistoryManifestEntry>;

    async fn list(
        &self,
        ctx: &CoreContext,
        blobstore: &Store,
    ) -> Result<BoxStream<'async_trait, Result<(MPathElement, Entry<Self::TreeId, Self::Leaf>)>>>
    {
        Ok(self
            .clone()
            .into_subentries(ctx, blobstore)
            .filter_map(|result| async {
                match result {
                    Ok((path, entry)) => history_manifest_to_mf_entry(entry).map(|e| Ok((path, e))),
                    Err(e) => Some(Err(e)),
                }
            })
            .boxed())
    }

    async fn list_prefix(
        &self,
        ctx: &CoreContext,
        blobstore: &Store,
        prefix: &[u8],
    ) -> Result<BoxStream<'async_trait, Result<(MPathElement, Entry<Self::TreeId, Self::Leaf>)>>>
    {
        Ok(self
            .clone()
            .into_prefix_subentries(ctx, blobstore, prefix)
            .filter_map(|result| async {
                match result {
                    Ok((path, entry)) => history_manifest_to_mf_entry(entry).map(|e| Ok((path, e))),
                    Err(e) => Some(Err(e)),
                }
            })
            .boxed())
    }

    async fn list_prefix_after(
        &self,
        ctx: &CoreContext,
        blobstore: &Store,
        prefix: &[u8],
        after: &[u8],
    ) -> Result<BoxStream<'async_trait, Result<(MPathElement, Entry<Self::TreeId, Self::Leaf>)>>>
    {
        Ok(self
            .clone()
            .into_prefix_subentries_after(ctx, blobstore, prefix, after)
            .filter_map(|result| async {
                match result {
                    Ok((path, entry)) => history_manifest_to_mf_entry(entry).map(|e| Ok((path, e))),
                    Err(e) => Some(Err(e)),
                }
            })
            .boxed())
    }

    async fn lookup(
        &self,
        ctx: &CoreContext,
        blobstore: &Store,
        name: &MPathElement,
    ) -> Result<Option<Entry<Self::TreeId, Self::Leaf>>> {
        Ok(self
            .lookup(ctx, blobstore, name)
            .await?
            .and_then(history_manifest_to_mf_entry))
    }

    async fn into_trie_map(
        self,
        _ctx: &CoreContext,
        _blobstore: &Store,
    ) -> Result<Self::TrieMapType> {
        Ok(LoadableShardedMapV2Node::Inlined(self.subentries))
    }
}

#[cfg(test)]
mod tests {
    use fbinit::FacebookInit;
    use futures::stream::TryStreamExt;
    use memblob::KeyedMemblob;
    use mononoke_macros::mononoke;
    use mononoke_types::NonRootMPath;
    use mononoke_types::blob::BlobstoreValue;
    use mononoke_types::history_manifest::HistoryManifestDeletedNode;
    use mononoke_types::history_manifest::HistoryManifestFile;
    use mononoke_types::sharded_map_v2::ShardedMapV2Node;
    use mononoke_types::typed_hash::HistoryManifestDeletedNodeId;
    use mononoke_types_mocks::changesetid::ONES_CSID;
    use mononoke_types_mocks::changesetid::THREES_CSID;
    use mononoke_types_mocks::changesetid::TWOS_CSID;
    use mononoke_types_mocks::contentid::ONES_CTID;
    use mononoke_types_mocks::contentid::TWOS_CTID;

    use super::*;

    fn make_file_entry(discriminant: u8) -> (HistoryManifestFileId, HistoryManifestEntry) {
        let path_hash = NonRootMPath::new(format!("path_{discriminant}"))
            .unwrap()
            .get_path_hash();
        let file = HistoryManifestFile {
            parents: vec![],
            content_id: if discriminant.is_multiple_of(2) {
                ONES_CTID
            } else {
                TWOS_CTID
            },
            file_type: mononoke_types::FileType::Regular,
            path_hash,
            linknode: match discriminant {
                0 => ONES_CSID,
                1 => TWOS_CSID,
                _ => THREES_CSID,
            },
            subentries: Default::default(),
        };
        let id = file.get_file_id();
        (id, HistoryManifestEntry::File(id))
    }

    fn make_dir_entry() -> (HistoryManifestDirectoryId, HistoryManifestEntry) {
        let dir = HistoryManifestDirectory::empty(vec![], TWOS_CSID);
        let id = dir.get_directory_id();
        (id, HistoryManifestEntry::Directory(id))
    }

    fn make_deleted_entry(
        discriminant: u8,
    ) -> (HistoryManifestDeletedNodeId, HistoryManifestEntry) {
        let node = HistoryManifestDeletedNode {
            parents: vec![],
            subentries: Default::default(),
            linknode: match discriminant {
                0 => ONES_CSID,
                _ => TWOS_CSID,
            },
        };
        let id = *node.into_blob().id();
        (id, HistoryManifestEntry::DeletedNode(id))
    }

    /// Build a HistoryManifestDirectory with the given named entries.
    async fn make_directory(
        ctx: &CoreContext,
        blobstore: &KeyedMemblob,
        entries: Vec<(&str, HistoryManifestEntry)>,
    ) -> Result<HistoryManifestDirectory> {
        let subentries = ShardedMapV2Node::from_entries(
            ctx,
            blobstore,
            entries
                .into_iter()
                .map(|(name, entry)| (name.as_bytes().to_vec(), entry)),
        )
        .await?;
        Ok(HistoryManifestDirectory {
            parents: vec![],
            subentries,
            linknode: ONES_CSID,
        })
    }

    #[mononoke::fbinit_test]
    async fn test_lookup_filters_deleted_nodes(fb: FacebookInit) -> Result<()> {
        let blobstore = KeyedMemblob::default();
        let ctx = CoreContext::test_mock(fb);

        let (fid, file_entry) = make_file_entry(0);
        let (did, dir_entry) = make_dir_entry();
        let (_del_id, deleted_entry) = make_deleted_entry(0);

        let dir = make_directory(
            &ctx,
            &blobstore,
            vec![
                ("file_a", file_entry),
                ("dir_b", dir_entry),
                ("deleted_c", deleted_entry),
            ],
        )
        .await?;

        // File entry returns Leaf
        let result = Manifest::lookup(
            &dir,
            &ctx,
            &blobstore,
            &MPathElement::new_from_slice(b"file_a")?,
        )
        .await?;
        assert_eq!(result, Some(Entry::Leaf(fid)));

        // Directory entry returns Tree
        let result = Manifest::lookup(
            &dir,
            &ctx,
            &blobstore,
            &MPathElement::new_from_slice(b"dir_b")?,
        )
        .await?;
        assert_eq!(result, Some(Entry::Tree(did)));

        // Deleted node returns None
        let result = Manifest::lookup(
            &dir,
            &ctx,
            &blobstore,
            &MPathElement::new_from_slice(b"deleted_c")?,
        )
        .await?;
        assert_eq!(result, None);

        // Non-existent entry returns None
        let result = Manifest::lookup(
            &dir,
            &ctx,
            &blobstore,
            &MPathElement::new_from_slice(b"missing")?,
        )
        .await?;
        assert_eq!(result, None);

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_list_filters_deleted_nodes(fb: FacebookInit) -> Result<()> {
        let blobstore = KeyedMemblob::default();
        let ctx = CoreContext::test_mock(fb);

        let (fid1, file_entry1) = make_file_entry(0);
        let (did, dir_entry) = make_dir_entry();
        let (_del_id1, deleted_entry1) = make_deleted_entry(0);
        let (_del_id2, deleted_entry2) = make_deleted_entry(1);
        let (fid2, file_entry2) = make_file_entry(1);

        let dir = make_directory(
            &ctx,
            &blobstore,
            vec![
                ("file_a", file_entry1),
                ("dir_b", dir_entry),
                ("deleted_c", deleted_entry1),
                ("deleted_d", deleted_entry2),
                ("file_e", file_entry2),
            ],
        )
        .await?;

        let entries: Vec<_> = dir.list(&ctx, &blobstore).await?.try_collect().await?;

        // Should only contain the 3 non-deleted entries
        assert_eq!(entries.len(), 3);

        let names: Vec<String> = entries
            .iter()
            .map(|(path, _)| String::from_utf8_lossy(path.as_ref()).into_owned())
            .collect();
        assert_eq!(names, vec!["dir_b", "file_a", "file_e"]);

        assert_eq!(entries[0].1, Entry::Tree(did));
        assert_eq!(entries[1].1, Entry::Leaf(fid1));
        assert_eq!(entries[2].1, Entry::Leaf(fid2));

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_list_prefix_filters_deleted_nodes(fb: FacebookInit) -> Result<()> {
        let blobstore = KeyedMemblob::default();
        let ctx = CoreContext::test_mock(fb);

        let (fid1, file_entry1) = make_file_entry(0);
        let (_del_id, deleted_entry) = make_deleted_entry(0);
        let (fid2, file_entry2) = make_file_entry(1);
        let (_fid3, other_entry) = make_file_entry(2);

        let dir = make_directory(
            &ctx,
            &blobstore,
            vec![
                ("file_a", file_entry1),
                ("file_b", deleted_entry),
                ("file_c", file_entry2),
                ("other", other_entry),
            ],
        )
        .await?;

        let entries: Vec<_> = dir
            .list_prefix(&ctx, &blobstore, b"file")
            .await?
            .try_collect()
            .await?;

        // "file_b" is deleted and should be filtered out, "other" doesn't match prefix
        assert_eq!(entries.len(), 2);

        let names: Vec<String> = entries
            .iter()
            .map(|(path, _)| String::from_utf8_lossy(path.as_ref()).into_owned())
            .collect();
        assert_eq!(names, vec!["file_a", "file_c"]);
        assert_eq!(entries[0].1, Entry::Leaf(fid1));
        assert_eq!(entries[1].1, Entry::Leaf(fid2));

        Ok(())
    }
}
