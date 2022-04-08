/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![allow(dead_code)]

use anyhow::{anyhow, bail, Context, Error, Result};
use async_recursion::async_recursion;
use blobstore::Blobstore;
use bounded_traversal::{bounded_traversal_ordered_stream, OrderedTraversal};
use bytes::Bytes;
use context::CoreContext;
use fbthrift::compact_protocol;
use futures::{FutureExt, Stream};
use nonzero_ext::nonzero;
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

    /// Given a key, what's the value for that key, if any?
    // See the detailed description of the logic in https://fburl.com/tlda3tzk
    #[async_recursion]
    pub async fn lookup(
        &self,
        ctx: &CoreContext,
        blobstore: &impl Blobstore,
        key: &[u8],
    ) -> Result<Option<Value>> {
        Ok(match self {
            // Case 1: Do lookup directly on the inlined map
            Self::Terminal { values } => values.get(key).cloned(),
            Self::Intermediate {
                prefix,
                value,
                children,
                ..
            } => {
                if let Some(key) = key.strip_prefix(prefix.as_slice()) {
                    if let Some((first, rest)) = key.split_first() {
                        if let Some(child) = children.get(first) {
                            // Case 2: Recurse, either inlined or first fetching from the blobstore
                            match child {
                                MapChild::Inlined(node) => {
                                    node.lookup(ctx, blobstore, rest).await?
                                }
                                MapChild::Id(id) => {
                                    Self::load(ctx, blobstore, id)
                                        .await?
                                        .lookup(ctx, blobstore, rest)
                                        .await?
                                }
                            }
                        } else {
                            // Case 3: No edge from this node to the next byte of the key
                            None
                        }
                    } else {
                        // Case 4: The node for this key is this intermediate node, not a terminal node
                        value.clone()
                    }
                } else {
                    // Case 5: Key doesn't match prefix
                    None
                }
            }
        })
    }

    /// Iterates through all values in the map, asynchronously and only loading
    /// blobs as needed.
    // See the detailed description of the logic in https://fburl.com/53iumd6p
    pub fn into_entries<'a>(
        self,
        ctx: &'a CoreContext,
        blobstore: &'a impl Blobstore,
    ) -> impl Stream<Item = Result<(SmallBinary, Value)>> + 'a {
        bounded_traversal_ordered_stream(
            nonzero!(256usize),
            nonzero!(256usize),
            vec![(self.size(), (SmallBinary::new(), MapChild::Inlined(self)))],
            move |(mut cur_prefix, id_or_inlined): (SmallBinary, MapChild<Value>)| {
                async move {
                    Ok(match id_or_inlined.load(ctx, blobstore).await? {
                        // Case 1. Prepend all keys with cur_prefix and output elements
                        Self::Terminal { values } => values
                            .into_iter()
                            .map(|(key, value)| {
                                let mut full_key = cur_prefix.clone();
                                full_key.extend(key);
                                OrderedTraversal::Output((full_key, value))
                            })
                            .collect::<Vec<_>>(),
                        // Case 2. Recurse
                        Self::Intermediate {
                            prefix: new_prefix,
                            value,
                            value_count,
                            children,
                        } => {
                            // Step 2-a. Extend cur_prefix
                            cur_prefix.extend(new_prefix);
                            let cur_prefix = &cur_prefix;
                            value
                                // Step 2-b. If value is present, output (cur_prefix, value)
                                .map(|value| OrderedTraversal::Output((cur_prefix.clone(), value)))
                                .into_iter()
                                // Step 2-c. Copy prefix, append byte, and recurse.
                                .chain(children.into_iter().map(|(byte, id_or_inlined)| {
                                    let mut new_prefix = cur_prefix.clone();
                                    new_prefix.push(byte);
                                    // We have a tradeoff to decide on here:
                                    // (1) If we don't load ids, we can't really know their size.
                                    // (2) If we do load ids, we know their size, but we might load
                                    // a lot of map nodes before they actually need to be used.
                                    // (3) The other possible solution is putting value_count on the
                                    // edge, not on the node.
                                    // The choice was (1), as we use scheduled_max = queued_max = 1
                                    // here, the size predictions shouldn't make any difference.
                                    let size_prediction = match &id_or_inlined {
                                        MapChild::Inlined(inlined) => inlined.size(),
                                        MapChild::Id(_) => value_count,
                                    };
                                    OrderedTraversal::Recurse(
                                        size_prediction,
                                        (new_prefix, id_or_inlined),
                                    )
                                }))
                                .collect::<Vec<_>>()
                        }
                    })
                }
                .boxed()
            },
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
    use context::CoreContext;
    use fbinit::FacebookInit;
    use futures::TryStreamExt;
    use memblob::Memblob;
    use pretty_assertions::assert_eq;

    #[derive(Debug, Clone, Copy, Eq, PartialEq)]
    struct MyType(i32);

    type TestShardedMap = ShardedMapNode<MyType>;

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

    fn terminal(values: Vec<(&str, i32)>) -> TestShardedMap {
        ShardedMapNode::Terminal {
            values: values
                .into_iter()
                .map(|(k, v)| (SmallVec::from_slice(k.as_bytes()), MyType(v)))
                .collect(),
        }
    }

    fn intermediate(
        prefix: &str,
        value: Option<i32>,
        children: Vec<(char, TestShardedMap)>,
    ) -> TestShardedMap {
        let value_count =
            children.iter().map(|(_, v)| v.size()).sum::<usize>() + value.iter().len();
        ShardedMapNode::Intermediate {
            prefix: SmallVec::from_slice(prefix.as_bytes()),
            value: value.map(MyType),
            value_count,
            children: children
                .into_iter()
                .map(|(c, v)| (c as u32 as u8, MapChild::Inlined(v)))
                .collect(),
        }
    }

    /// Returns an example map based on the picture on https://fburl.com/2fqtp2rk
    fn example_map() -> TestShardedMap {
        let abac = terminal(vec![
            ("ab", 7),
            ("aba", 8),
            ("akkk", 9),
            ("ate", 10),
            ("axi", 11),
        ]);
        let abal = terminal(vec![("aba", 5), ("ada", 6)]);
        let a = intermediate("ba", Some(12), vec![('c', abac), ('l', abal)]);
        let o = terminal(vec![("miojo", 1), ("miux", 2), ("mundo", 3), ("mungal", 4)]);
        // root
        intermediate("", None, vec![('a', a), ('o', o)])
    }

    fn assert_round_trip(map: TestShardedMap) {
        let map_t = map.clone().into_thrift();
        // This is not deep equality through blobstore
        assert_eq!(ShardedMapNode::from_thrift(map_t).unwrap(), map);
    }

    struct MapHelper(TestShardedMap, CoreContext, Memblob);
    impl MapHelper {
        fn size(&self) -> usize {
            self.0.size()
        }

        async fn lookup(&self, key: &str) -> Result<Option<i32>> {
            let v = self.0.lookup(&self.1, &self.2, key.as_bytes()).await?;
            Ok(v.map(|my_type| my_type.0))
        }

        fn entries(&self) -> impl Stream<Item = Result<(String, i32)>> + '_ {
            self.0
                .clone()
                .into_entries(&self.1, &self.2)
                .and_then(|(k, v)| async move { Ok((String::from_utf8(k.to_vec())?, v.0)) })
        }

        async fn assert_entries(&self, entries: Vec<(&str, i32)>) -> Result<()> {
            assert_eq!(
                self.entries().try_collect::<Vec<_>>().await?,
                entries
                    .into_iter()
                    .map(|(k, v)| (String::from(k), v))
                    .collect::<Vec<_>>()
            );
            Ok(())
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

        let map = terminal(vec![("ab", 3), ("cd", 5)]);
        assert!(!map.is_empty());
        assert_round_trip(map);

        let map = example_map();
        assert!(!map.is_empty());
        assert_eq!(map.size(), 12);
        assert_round_trip(map);
    }

    #[fbinit::test]
    async fn lookup_test(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let blobstore = Memblob::default();

        let map = MapHelper(example_map(), ctx, blobstore);
        // Case 2 > Case 1
        assert_eq!(map.lookup("omiux").await?, Some(2));
        // Case 3
        assert_eq!(map.lookup("inexistent").await?, None);
        // Case 2 > Case 5
        assert_eq!(map.lookup("abxio").await?, None);
        // Case 2 > Case 4
        assert_eq!(map.lookup("aba").await?, Some(12));
        // Case 2 > Case 2 > Case 1
        assert_eq!(map.lookup("abacakkk").await?, Some(9));
        assert_eq!(map.lookup("abacakk").await?, None);
        // Case 4
        assert_eq!(map.lookup("").await?, None);
        Ok(())
    }

    #[fbinit::test]
    async fn into_entries_test(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let blobstore = Memblob::default();

        let map = MapHelper(example_map(), ctx, blobstore);
        map.assert_entries(vec![
            ("aba", 12),
            ("abacab", 7),
            ("abacaba", 8),
            ("abacakkk", 9),
            ("abacate", 10),
            ("abacaxi", 11),
            ("abalaba", 5),
            ("abalada", 6),
            ("omiojo", 1),
            ("omiux", 2),
            ("omundo", 3),
            ("omungal", 4),
        ])
        .await?;
        Ok(())
    }
}
