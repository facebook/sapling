/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![allow(dead_code)]

use anyhow::{anyhow, bail, Context, Error, Result};
use blobstore::Blobstore;
use bytes::Bytes;
use context::CoreContext;
use fbthrift::compact_protocol;
use smallvec::SmallVec;
use sorted_vector_map::SortedVectorMap;

use crate::blob::{Blob, BlobstoreValue, ShardedMapNodeBlob};
use crate::errors::ErrorKind;
use crate::thrift;
use crate::typed_hash::{BlobstoreKey, ShardedMapNodeContext, ShardedMapNodeId};

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub enum MapChild<Value: MapValue> {
    Id(ShardedMapNodeId),
    Inlined(ShardedMapNode<Value>),
}

#[trait_alias::trait_alias]
pub trait MapValue =
    TryFrom<Bytes, Error = Error> + Into<Bytes> + std::fmt::Debug + Clone + Send + Sync + 'static;

type SmallBinary = SmallVec<[u8; 24]>;

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub enum ShardedMapNode<Value: MapValue> {
    Intermediate {
        prefix: SmallBinary,
        value: Option<Value>,
        value_count: usize,
        children: SortedVectorMap<u8, MapChild<Value>>,
    },
    Terminal {
        // The key is the original map key minus the prefixes and edges from all
        // intermediate nodes in the path to this node.
        values: SortedVectorMap<SmallBinary, Value>,
    },
}

impl<Value: MapValue> MapChild<Value> {
    async fn load(
        self,
        ctx: &CoreContext,
        blobstore: &impl Blobstore,
    ) -> Result<ShardedMapNode<Value>> {
        match self {
            Self::Inlined(inlined) => Ok(inlined),
            Self::Id(id) => ShardedMapNode::load(ctx, blobstore, &id).await,
        }
    }

    fn from_thrift(t: thrift::MapChild) -> Result<Self> {
        Ok(match t {
            thrift::MapChild::inlined(inlined) => {
                Self::Inlined(ShardedMapNode::from_thrift(inlined)?)
            }
            thrift::MapChild::id(id) => Self::Id(ShardedMapNodeId::from_thrift(id)?),
            thrift::MapChild::UnknownField(_) => bail!("Unknown variant"),
        })
    }

    fn into_thrift(self) -> thrift::MapChild {
        match self {
            Self::Inlined(inlined) => thrift::MapChild::inlined(inlined.into_thrift()),
            Self::Id(id) => thrift::MapChild::id(id.into_thrift()),
        }
    }
}

impl<Value: MapValue> ShardedMapNode<Value> {
    async fn load(
        ctx: &CoreContext,
        blobstore: &impl Blobstore,
        id: &ShardedMapNodeId,
    ) -> Result<Self> {
        let key = id.blobstore_key();
        Self::from_bytes(
            blobstore
                .get(ctx, &key)
                .await?
                .with_context(|| anyhow!("Blob is missing: {}", key))?
                .into_raw_bytes()
                .as_ref(),
        )
    }

    pub fn is_empty(&self) -> bool {
        match self {
            Self::Terminal { values } => values.is_empty(),
            Self::Intermediate { value_count, .. } => *value_count == 0,
        }
    }

    fn size(&self) -> usize {
        match self {
            Self::Terminal { values } => values.len(),
            Self::Intermediate { value_count, .. } => *value_count,
        }
    }

    pub(crate) fn from_thrift(t: thrift::ShardedMapNode) -> Result<Self> {
        Ok(match t {
            thrift::ShardedMapNode::intermediate(intermediate) => Self::Intermediate {
                prefix: intermediate.prefix.0,
                value: intermediate.value.map(Value::try_from).transpose()?,
                value_count: intermediate.value_count as usize,
                children: intermediate
                    .children
                    .into_iter()
                    .map(|(k, v)| Ok((k as u8, MapChild::from_thrift(v)?)))
                    .collect::<Result<_>>()?,
            },
            thrift::ShardedMapNode::terminal(terminal) => Self::Terminal {
                values: terminal
                    .values
                    .into_iter()
                    .map(|(k, v)| Ok((k.0, Value::try_from(v)?)))
                    .collect::<Result<_>>()?,
            },
            thrift::ShardedMapNode::UnknownField(_) => bail!("Unknown map node variant"),
        })
    }

    pub(crate) fn into_thrift(self) -> thrift::ShardedMapNode {
        match self {
            Self::Intermediate {
                prefix,
                value,
                value_count,
                children,
            } => thrift::ShardedMapNode::intermediate(thrift::ShardedMapIntermediateNode {
                prefix: thrift::small_binary(prefix),
                value: value.map(Into::into),
                value_count: value_count as i64,
                children: children
                    .into_iter()
                    .map(|(k, v)| (k as i8, v.into_thrift()))
                    .collect(),
            }),
            Self::Terminal { values } => {
                thrift::ShardedMapNode::terminal(thrift::ShardedMapTerminalNode {
                    values: values
                        .into_iter()
                        .map(|(k, v)| (thrift::small_binary(k), v.into()))
                        .collect(),
                })
            }
        }
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        let thrift_tc = compact_protocol::deserialize(bytes)
            .with_context(|| ErrorKind::BlobDeserializeError("ShardedMapNode".into()))?;
        Self::from_thrift(thrift_tc)
    }
}

impl<Value: MapValue> BlobstoreValue for ShardedMapNode<Value> {
    type Key = ShardedMapNodeId;

    fn into_blob(self) -> ShardedMapNodeBlob {
        let thrift = self.into_thrift();
        let data = compact_protocol::serialize(&thrift);
        let mut context = ShardedMapNodeContext::new();
        context.update(&data);
        let id = context.finish();
        Blob::new(id, data)
    }

    fn from_blob(blob: Blob<Self::Key>) -> Result<Self> {
        Self::from_bytes(blob.data().as_ref())
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use bytes::{Buf, BufMut, BytesMut};

    #[derive(Debug, Clone, Copy, Eq, PartialEq)]
    struct MyType(i32);

    impl TryFrom<Bytes> for MyType {
        type Error = anyhow::Error;
        fn try_from(mut b: Bytes) -> Result<Self> {
            Ok(Self(b.get_i32()))
        }
    }

    impl From<MyType> for Bytes {
        fn from(t: MyType) -> Bytes {
            let mut b = BytesMut::new();
            b.put_i32(t.0);
            b.freeze()
        }
    }

    #[test]
    fn basic_test() {
        let empty = ShardedMapNode::<MyType>::Terminal {
            values: Default::default(),
        };
        assert!(empty.is_empty());
        assert_eq!(empty.size(), 0);
        let empty = ShardedMapNode::<MyType>::Intermediate {
            value: None,
            value_count: 0,
            children: Default::default(),
            prefix: Default::default(),
        };
        assert!(empty.is_empty());
        assert_eq!(empty.size(), 0);

        let map = ShardedMapNode::Terminal {
            values: vec![
                (SmallBinary::from_vec(vec![1, 3]), MyType(3)),
                (SmallBinary::from_vec(vec![5, 7]), MyType(5)),
            ]
            .into_iter()
            .collect(),
        };
        let map_t = map.clone().into_thrift();
        assert_eq!(ShardedMapNode::from_thrift(map_t).unwrap(), map);
    }
}
