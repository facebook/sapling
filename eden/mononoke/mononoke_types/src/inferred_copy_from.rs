/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use blobstore::Blobstore;
use context::CoreContext;
use futures::stream::BoxStream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;

use crate::Blob;
use crate::BlobstoreValue;
use crate::ThriftConvert;
use crate::blob::InferredCopyFromBlob;
use crate::path::MPath;
use crate::sharded_map_v2::ShardedMapV2Node;
use crate::sharded_map_v2::ShardedMapV2Value;
use crate::thrift;
use crate::typed_hash::ChangesetId;
use crate::typed_hash::IdContext;
use crate::typed_hash::InferredCopyFromContext;
use crate::typed_hash::InferredCopyFromId;
use crate::typed_hash::ShardedMapV2NodeInferredCopyFromContext;
use crate::typed_hash::ShardedMapV2NodeInferredCopyFromId;

#[derive(ThriftConvert, Debug, Clone, PartialEq, Eq, Hash)]
#[thrift(thrift::inferred_copy_from::InferredCopyFrom)]
pub struct InferredCopyFrom {
    pub subentries: ShardedMapV2Node<InferredCopyFromEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct InferredCopyFromEntry {
    pub from_csid: ChangesetId,
    pub from_path: MPath,
}

impl ThriftConvert for InferredCopyFromEntry {
    const NAME: &'static str = "InferredCopyFromEntry";
    type Thrift = thrift::inferred_copy_from::InferredCopyFromEntry;

    fn from_thrift(t: Self::Thrift) -> Result<Self> {
        Ok(Self {
            from_csid: ChangesetId::from_thrift(t.from_csid)?,
            from_path: MPath::from_thrift(t.from_path)?,
        })
    }

    fn into_thrift(self) -> Self::Thrift {
        Self::Thrift {
            from_csid: self.from_csid.into_thrift(),
            from_path: self.from_path.into_thrift(),
        }
    }
}

impl ShardedMapV2Value for InferredCopyFromEntry {
    type NodeId = ShardedMapV2NodeInferredCopyFromId;
    type Context = ShardedMapV2NodeInferredCopyFromContext;
    type RollupData = ();

    const WEIGHT_LIMIT: usize = 2000;
}

impl InferredCopyFrom {
    pub fn empty() -> Self {
        Self {
            subentries: ShardedMapV2Node::default(),
        }
    }

    pub async fn lookup(
        &self,
        ctx: &CoreContext,
        blobstore: &impl Blobstore,
        path: &MPath,
    ) -> Result<Option<InferredCopyFromEntry>> {
        self.subentries
            .lookup(ctx, blobstore, &path.to_null_separated_bytes())
            .await
    }

    pub async fn from_subentries(
        ctx: &CoreContext,
        blobstore: &impl Blobstore,
        subentries: impl IntoIterator<Item = (MPath, InferredCopyFromEntry)>,
    ) -> Result<Self> {
        Ok(Self {
            subentries: ShardedMapV2Node::from_entries(
                ctx,
                blobstore,
                subentries
                    .into_iter()
                    .map(|(path, entry)| (path.to_null_separated_bytes(), entry)),
            )
            .await?,
        })
    }

    pub fn into_subentries<'a>(
        self,
        ctx: &'a CoreContext,
        blobstore: &'a impl Blobstore,
    ) -> BoxStream<'a, Result<(MPath, InferredCopyFromEntry)>> {
        self.subentries
            .into_entries(ctx, blobstore)
            .and_then(
                |(k, v)| async move { Ok((MPath::from_null_separated_bytes(k.to_vec())?, v)) },
            )
            .boxed()
    }

    pub fn into_prefix_subentries<'a>(
        self,
        ctx: &'a CoreContext,
        blobstore: &'a impl Blobstore,
        prefix: &'a [u8],
    ) -> BoxStream<'a, Result<(MPath, InferredCopyFromEntry)>> {
        self.subentries
            .into_prefix_entries(ctx, blobstore, prefix)
            .map(|res| {
                res.and_then(|(k, v)| Ok((MPath::from_null_separated_bytes(k.to_vec())?, v)))
            })
            .boxed()
    }
}

impl BlobstoreValue for InferredCopyFrom {
    type Key = InferredCopyFromId;

    fn into_blob(self) -> InferredCopyFromBlob {
        let data = self.into_bytes();
        let id = InferredCopyFromContext::id_from_data(&data);
        Blob::new(id, data)
    }

    fn from_blob(blob: Blob<Self::Key>) -> Result<Self> {
        Self::from_bytes(blob.data())
    }
}
