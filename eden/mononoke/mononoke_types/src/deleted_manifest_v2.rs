/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{Context, Result};
use blobstore::{Blobstore, Storable};
use bytes::Bytes;
use context::CoreContext;
use fbthrift::compact_protocol;
use futures::{stream::BoxStream, StreamExt, TryStreamExt};
use std::collections::BTreeMap;

use crate::blob::{Blob, BlobstoreValue, DeletedManifestV2Blob};
use crate::deleted_manifest_common::DeletedManifestCommon;
use crate::errors::ErrorKind;
use crate::sharded_map::ShardedMapNode;
use crate::thrift;
use crate::typed_hash::{BlobstoreKey, ChangesetId, DeletedManifestV2Context, DeletedManifestV2Id};
use crate::MPathElement;

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct DeletedManifestV2 {
    linknode: Option<ChangesetId>,
    subentries: ShardedMapNode<DeletedManifestV2Id>,
}

#[async_trait::async_trait]
impl DeletedManifestCommon for DeletedManifestV2 {
    type Id = DeletedManifestV2Id;

    async fn copy_and_update_subentries(
        ctx: &CoreContext,
        blobstore: &impl Blobstore,
        current: Option<Self>,
        linknode: Option<ChangesetId>,
        subentries_to_update: BTreeMap<MPathElement, Option<Self::Id>>,
    ) -> Result<Self> {
        let subentries = current.map(|mf| mf.subentries).unwrap_or_default();
        let subentries = subentries
            .update(
                ctx,
                blobstore,
                subentries_to_update
                    .into_iter()
                    .map(|(k, v)| (Bytes::copy_from_slice(k.as_ref()), v))
                    .collect(),
            )
            .await?;
        Ok(Self::new(linknode, subentries))
    }

    fn linknode(&self) -> Option<&ChangesetId> {
        self.linknode.as_ref()
    }

    fn is_empty(&self) -> bool {
        self.subentries.is_empty()
    }

    fn id(&self) -> Self::Id {
        *self.clone().into_blob().id()
    }

    async fn lookup(
        &self,
        ctx: &CoreContext,
        blobstore: &impl Blobstore,
        basename: &MPathElement,
    ) -> Result<Option<Self::Id>> {
        self.subentries
            .lookup(ctx, blobstore, basename.as_ref())
            .await
    }

    fn into_subentries<'a>(
        self,
        ctx: &'a CoreContext,
        blobstore: &'a impl Blobstore,
    ) -> BoxStream<'a, Result<(MPathElement, Self::Id)>> {
        self.subentries
            .into_entries(ctx, blobstore)
            .and_then(|(k, v)| async move { anyhow::Ok((MPathElement::from_smallvec(k)?, v)) })
            .boxed()
    }
}

impl DeletedManifestV2 {
    pub fn new(
        linknode: Option<ChangesetId>,
        subentries: ShardedMapNode<DeletedManifestV2Id>,
    ) -> Self {
        Self {
            linknode,
            subentries,
        }
    }

    pub(crate) fn from_thrift(t: thrift::DeletedManifestV2) -> Result<DeletedManifestV2> {
        Ok(Self {
            linknode: t.linknode.map(ChangesetId::from_thrift).transpose()?,
            subentries: ShardedMapNode::from_thrift(t.subentries)?,
        })
    }

    pub(crate) fn into_thrift(self) -> thrift::DeletedManifestV2 {
        thrift::DeletedManifestV2 {
            linknode: self.linknode.map(ChangesetId::into_thrift),
            subentries: self.subentries.into_thrift(),
        }
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        let thrift_tc = compact_protocol::deserialize(bytes)
            .with_context(|| ErrorKind::BlobDeserializeError("DeletedManifestV2".into()))?;
        Self::from_thrift(thrift_tc)
    }
}

impl BlobstoreValue for DeletedManifestV2 {
    type Key = DeletedManifestV2Id;

    fn into_blob(self) -> DeletedManifestV2Blob {
        let thrift = self.into_thrift();
        let data = compact_protocol::serialize(&thrift);
        let mut context = DeletedManifestV2Context::new();
        context.update(&data);
        let id = context.finish();
        Blob::new(id, data)
    }

    fn from_blob(blob: Blob<Self::Key>) -> Result<Self> {
        Self::from_bytes(blob.data().as_ref())
    }
}

#[async_trait::async_trait]
impl Storable for DeletedManifestV2 {
    type Key = DeletedManifestV2Id;

    async fn store<'a, B: Blobstore>(
        self,
        ctx: &'a CoreContext,
        blobstore: &'a B,
    ) -> Result<Self::Key> {
        let blob = self.into_blob();
        let id = blob.id().clone();
        blobstore.put(ctx, id.blobstore_key(), blob.into()).await?;
        Ok(id)
    }
}
