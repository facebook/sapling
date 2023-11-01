/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fmt::Debug;

use anyhow::bail;
use anyhow::Context;
use anyhow::Result;
use blobstore::Blobstore;
use blobstore::Loadable;
use context::CoreContext;
use smallvec::SmallVec;
use sorted_vector_map::SortedVectorMap;

use crate::blob::Blob;
use crate::blob::BlobstoreValue;
use crate::thrift;
use crate::typed_hash::IdContext;
use crate::typed_hash::MononokeId;
use crate::ThriftConvert;

// More detailed documentation about ShardedMapV2 can be found in mononoke_types_thrift.thrift

pub trait ShardedMapV2Value: ThriftConvert + Debug + Clone + Send + Sync + 'static {
    type NodeId: MononokeId<Thrift = thrift::ShardedMapV2NodeId, Value = ShardedMapV2Node<Self>>;
    type Context: IdContext<Id = Self::NodeId>;
}

type SmallBinary = SmallVec<[u8; 24]>;

#[derive(Clone, Eq, PartialEq, Debug, Default)]
pub struct ShardedMapV2Node<Value: ShardedMapV2Value> {
    prefix: SmallBinary,
    value: Option<Value>,
    children: SortedVectorMap<u8, ShardedMapV2Child<Value>>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum ShardedMapV2Child<Value: ShardedMapV2Value> {
    Inlined(ShardedMapV2Node<Value>),
    Stored(ShardedMapV2StoredNode<Value>),
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ShardedMapV2StoredNode<Value: ShardedMapV2Value> {
    id: Value::NodeId,
}

impl<Value: ShardedMapV2Value> ShardedMapV2StoredNode<Value> {
    fn from_thrift(t: thrift::ShardedMapV2StoredNode) -> Result<Self> {
        Ok(Self {
            id: Value::NodeId::from_thrift(t.id)?,
        })
    }

    fn into_thrift(self) -> thrift::ShardedMapV2StoredNode {
        thrift::ShardedMapV2StoredNode {
            id: self.id.into_thrift(),
        }
    }
}

impl<Value: ShardedMapV2Value> ShardedMapV2Child<Value> {
    pub async fn load(
        self,
        ctx: &CoreContext,
        blobstore: &impl Blobstore,
    ) -> Result<ShardedMapV2Node<Value>> {
        match self {
            Self::Inlined(inlined) => Ok(inlined),
            Self::Stored(stored) => stored
                .id
                .load(ctx, blobstore)
                .await
                .with_context(|| "Failed to load stored child"),
        }
    }

    fn from_thrift(t: thrift::ShardedMapV2Child) -> Result<Self> {
        Ok(match t {
            thrift::ShardedMapV2Child::inlined(inlined) => {
                Self::Inlined(ShardedMapV2Node::from_thrift(inlined)?)
            }
            thrift::ShardedMapV2Child::stored(stored) => {
                Self::Stored(ShardedMapV2StoredNode::from_thrift(stored)?)
            }
            thrift::ShardedMapV2Child::UnknownField(_) => bail!("Unknown variant"),
        })
    }

    fn into_thrift(self) -> thrift::ShardedMapV2Child {
        match self {
            Self::Inlined(inlined) => thrift::ShardedMapV2Child::inlined(inlined.into_thrift()),
            Self::Stored(stored) => thrift::ShardedMapV2Child::stored(stored.into_thrift()),
        }
    }
}

impl<Value: ShardedMapV2Value> ThriftConvert for ShardedMapV2Node<Value> {
    const NAME: &'static str = "ShardedMapV2Node";
    type Thrift = thrift::ShardedMapV2Node;

    fn from_thrift(t: thrift::ShardedMapV2Node) -> Result<Self> {
        Ok(Self {
            prefix: t.prefix.0,
            value: t
                .value
                .as_ref()
                .map(ThriftConvert::from_bytes)
                .transpose()?,
            children: t
                .children
                .into_iter()
                .map(|(key, child)| Ok((key as u8, ShardedMapV2Child::from_thrift(child)?)))
                .collect::<Result<_>>()?,
        })
    }

    fn into_thrift(self) -> thrift::ShardedMapV2Node {
        thrift::ShardedMapV2Node {
            prefix: thrift::small_binary(self.prefix),
            value: self.value.map(ThriftConvert::into_bytes),
            children: self
                .children
                .into_iter()
                .map(|(key, child)| (key as i8, child.into_thrift()))
                .collect(),
        }
    }
}

impl<Value: ShardedMapV2Value> BlobstoreValue for ShardedMapV2Node<Value> {
    type Key = Value::NodeId;

    fn into_blob(self) -> Blob<Self::Key> {
        let data = self.into_bytes();
        let id = Value::Context::id_from_data(&data);
        Blob::new(id, data)
    }

    fn from_blob(blob: Blob<Self::Key>) -> Result<Self> {
        Self::from_bytes(blob.data())
    }
}
