/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![allow(dead_code)]
#![allow(clippy::mutable_key_type)] // false positive: Bytes is not inner mutable

use std::collections::BTreeMap;
use std::fmt::Debug;

use anyhow::bail;
use anyhow::Context;
use anyhow::Ok;
use anyhow::Result;
use async_recursion::async_recursion;
use blobstore::Blobstore;
use blobstore::Loadable;
use blobstore::Storable;
use bounded_traversal::bounded_traversal_ordered_stream;
use bounded_traversal::OrderedTraversal;
use bytes::Bytes;
use context::CoreContext;
use derivative::Derivative;
use futures::stream;
use futures::FutureExt;
use futures::Stream;
use futures::StreamExt;
use futures::TryStreamExt;
use itertools::Itertools;
use nonzero_ext::nonzero;
use once_cell::sync::OnceCell;
use smallvec::SmallVec;
use sorted_vector_map::sorted_vector_map;
use sorted_vector_map::SortedVectorMap;

use crate::blob::Blob;
use crate::blob::BlobstoreValue;
use crate::thrift;
use crate::typed_hash::IdContext;
use crate::typed_hash::MononokeId;
use crate::ThriftConvert;

pub trait MapValue: ThriftConvert + Debug + Clone + Send + Sync + 'static {
    type Id: MononokeId<Thrift = thrift::ShardedMapNodeId, Value = ShardedMapNode<Self>>;
    type Context: IdContext<Id = Self::Id>;
}

type SmallBinary = SmallVec<[u8; 24]>;

#[derive(Derivative)]
#[derivative(PartialEq, Debug)]
#[derive(Clone, Eq)]
pub enum ShardedMapNode<Value: MapValue> {
    Intermediate {
        prefix: SmallBinary,
        value: Option<Value>,
        edges: SortedVectorMap<u8, ShardedMapEdge<Value>>,
        #[derivative(PartialEq = "ignore", Debug = "ignore")]
        size: OnceCell<usize>,
    },
    Terminal {
        // The key is the original map key minus the prefixes and edges from all
        // intermediate nodes in the path to this node.
        values: SortedVectorMap<SmallBinary, Value>,
    },
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ShardedMapEdge<Value: MapValue> {
    size: usize,
    child: ShardedMapChild<Value>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum ShardedMapChild<Value: MapValue> {
    Id(Value::Id),
    Inlined(ShardedMapNode<Value>),
}

impl<Value: MapValue> ShardedMapEdge<Value> {
    async fn load_child(
        self,
        ctx: &CoreContext,
        blobstore: &impl Blobstore,
    ) -> Result<ShardedMapNode<Value>> {
        self.child.load(ctx, blobstore).await
    }

    fn from_thrift(t: thrift::ShardedMapEdge) -> Result<Self> {
        Ok(Self {
            size: t.size.try_into().context("Failed to parse size to usize")?,
            child: ShardedMapChild::from_thrift(t.child)?,
        })
    }

    fn into_thrift(self) -> thrift::ShardedMapEdge {
        thrift::ShardedMapEdge {
            size: self.size as i64,
            child: self.child.into_thrift(),
        }
    }
}

impl<Value: MapValue> ShardedMapChild<Value> {
    async fn load(
        self,
        ctx: &CoreContext,
        blobstore: &impl Blobstore,
    ) -> Result<ShardedMapNode<Value>> {
        match self {
            Self::Inlined(inlined) => Ok(inlined),
            Self::Id(id) => id.load(ctx, blobstore).await.map_err(Into::into),
        }
    }

    fn from_thrift(t: thrift::ShardedMapChild) -> Result<Self> {
        Ok(match t {
            thrift::ShardedMapChild::inlined(inlined) => {
                Self::Inlined(ShardedMapNode::from_thrift(inlined)?)
            }
            thrift::ShardedMapChild::id(id) => Self::Id(Value::Id::from_thrift(id)?),
            thrift::ShardedMapChild::UnknownField(_) => bail!("Unknown variant"),
        })
    }

    fn into_thrift(self) -> thrift::ShardedMapChild {
        match self {
            Self::Inlined(inlined) => thrift::ShardedMapChild::inlined(inlined.into_thrift()),
            Self::Id(id) => thrift::ShardedMapChild::id(id.into_thrift()),
        }
    }
}

impl<Value: MapValue> Default for ShardedMapChild<Value> {
    fn default() -> Self {
        Self::Inlined(Default::default())
    }
}

impl<Value: MapValue> Default for ShardedMapNode<Value> {
    fn default() -> Self {
        Self::Terminal {
            values: SortedVectorMap::new(),
        }
    }
}

impl<Value: MapValue> Default for ShardedMapEdge<Value> {
    fn default() -> Self {
        Self {
            size: 0,
            child: Default::default(),
        }
    }
}

/// Returns longest common prefix of a and b.
fn common_prefix<'a>(a: &'a [u8], b: &'a [u8]) -> &'a [u8] {
    let lcp = a.iter().zip(b.iter()).take_while(|(a, b)| a == b).count();
    // Panic safety: lcp is at most a.len()
    &a[..lcp]
}

impl<Value: MapValue> ShardedMapNode<Value> {
    fn intermediate(
        prefix: SmallBinary,
        value: Option<Value>,
        edges: SortedVectorMap<u8, ShardedMapEdge<Value>>,
    ) -> Self {
        Self::Intermediate {
            prefix,
            value,
            edges,
            size: Default::default(),
        }
    }

    /// Given a key, what's the value for that key, if any?
    // See the detailed description of the logic in docs/sharded_map.md
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
                edges,
                ..
            } => {
                if let Some(key) = key.strip_prefix(prefix.as_slice()) {
                    if let Some((first, rest)) = key.split_first() {
                        if let Some(edge) = edges.get(first) {
                            // Case 2: Recurse, either inlined or first fetching from the blobstore
                            match &edge.child {
                                ShardedMapChild::Inlined(node) => {
                                    node.lookup(ctx, blobstore, rest).await?
                                }
                                ShardedMapChild::Id(id) => {
                                    id.load(ctx, blobstore)
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

    /// Given a map and replacements, return the map with the replacements.
    fn update_map(
        mut map: BTreeMap<SmallBinary, Value>,
        replacements: impl IntoIterator<Item = (Bytes, Option<Value>)>,
        deleter: impl Fn(Value),
    ) -> Result<SortedVectorMap<SmallBinary, Value>> {
        for (key, value) in replacements {
            let key = SmallVec::from_iter(key);
            match value {
                Some(value) => map.insert(key, value),
                None => map.remove(&key),
            }
            .map(&deleter);
        }
        Ok(map.into())
    }

    /// Prepend all keys in this node with the given prefix.
    fn prepend(&mut self, prefix: SmallBinary) {
        match self {
            Self::Terminal { values } => {
                *values = std::mem::take(values)
                    .into_iter()
                    .update(|(k, _)| {
                        k.insert_from_slice(0, &prefix);
                    })
                    .collect()
            }
            Self::Intermediate {
                prefix: cur_prefix, ..
            } => {
                cur_prefix.insert_from_slice(0, &prefix);
            }
        }
    }

    fn shard_size() -> Result<usize> {
        if cfg!(test) {
            Ok(5)
        } else {
            thrift::MAP_SHARD_SIZE
                .try_into()
                .context("Failed to parse shard size")
        }
    }

    /// Create a new map from this map with given replacements. It is a generalization of
    /// adding and removing, and should be faster than doing all operations separately.
    /// It does not rely on the added keys not existing or the removed keys existing.
    // See the detailed description of the logic in docs/sharded_map.md
    #[async_recursion]
    pub async fn update(
        self,
        ctx: &CoreContext,
        blobstore: &impl Blobstore,
        replacements: BTreeMap<Bytes, Option<Value>>,
        // Called for all deletions
        deleter: impl Fn(Value) + Send + Copy + 'async_recursion,
    ) -> Result<Self> {
        let shard_size = Self::shard_size()?;
        match self {
            Self::Terminal { values } => {
                let values = Self::update_map(values.into_iter().collect(), replacements, deleter)?;
                if values.len() <= shard_size {
                    // Case 1: values is small enough, return a terminal node
                    Ok(Self::Terminal { values })
                } else {
                    // Case 2: This will become a intermediate node
                    // Let's reuse the logic to add values to a intermediate node by creating
                    // an empty one.
                    let lcp = values
                        .keys()
                        .map(|k| k.as_slice())
                        .reduce(common_prefix)
                        .unwrap_or(b"");
                    // Setting the correct prefix is not necessary for correctness, but it avoids
                    // having Case 3 + Case 4.3.2 + compression unnecessarily.
                    Self::intermediate(SmallBinary::from_slice(lcp), None, Default::default())
                        .update(
                            ctx,
                            blobstore,
                            values
                                .into_iter()
                                .map(|(k, v)| (Bytes::copy_from_slice(k.as_ref()), Some(v)))
                                .collect(),
                            deleter,
                        )
                        .await
                }
            }
            Self::Intermediate {
                mut prefix,
                mut value,
                mut edges,
                ..
            } => {
                // LCP only considered added keys
                let lcp = replacements
                    .iter()
                    .filter_map(|(k, v)| v.as_ref().map(|_| k))
                    .fold(prefix.as_slice(), |lcp, key| common_prefix(lcp, key))
                    .len();
                if lcp < prefix.len() {
                    // Case 3: The prefix of all keys is smaller than `prefix`
                    // Let's create two new nodes and recursively update them.
                    // Right: Bytes lcp + 1 .. size
                    let prefix_right = prefix.drain(lcp + 1..).collect();
                    // Middle: Byte lcp
                    // unwrap safety: lcp + 1 > 0, so split_off leaves at least one element at prefix
                    let mid_byte = prefix.pop().unwrap();
                    // Left: Bytes 0 .. lcp
                    let prefix_left = prefix;
                    let right_node = Self::intermediate(prefix_right, value, edges);
                    let left_node = Self::intermediate(
                        prefix_left,
                        None,
                        // Design decision: all intermediate nodes are inlined
                        sorted_vector_map! {
                            mid_byte =>
                            ShardedMapEdge {
                                size: right_node.size(),
                                child: ShardedMapChild::Inlined(right_node),
                            },
                        },
                    );
                    left_node
                        .update(ctx, blobstore, replacements, deleter)
                        .await
                } else {
                    // Case 4: All added keys traverse the long edge (have the prefix `prefix`)
                    let mut partitioned = BTreeMap::<u8, BTreeMap<Bytes, Option<Value>>>::new();
                    // Step 4.1: Strip prefixes, and partition replacements.
                    replacements.into_iter().for_each(|(k, v)| {
                        match k.strip_prefix(prefix.as_slice()) {
                            None => {
                                // Only deletions might not have the correct prefix
                                debug_assert!(v.is_none());
                            }
                            Some(rest) => {
                                if let Some((first, rest)) = rest.split_first() {
                                    partitioned
                                        .entry(*first)
                                        .or_default()
                                        // Panic safety: rest was produced from k
                                        .insert(k.slice_ref(rest), v);
                                } else {
                                    std::mem::replace(&mut value, v).map(deleter);
                                }
                            }
                        }
                    });

                    // Step 4.2: Recursively update partitioned children
                    let replaced_futures = partitioned
                        .into_iter()
                        .map(|(next_byte, replacements)| {
                            let edge = edges.remove(&next_byte).unwrap_or_default();
                            async move {
                                let node = edge.load_child(ctx, blobstore).await?;
                                let replaced_node =
                                    node.update(ctx, blobstore, replacements, deleter).await?;
                                Ok((next_byte, replaced_node))
                            }
                        })
                        .collect::<Vec<_>>();
                    let replaced = stream::iter(replaced_futures)
                        .buffer_unordered(100)
                        .try_collect::<Vec<_>>()
                        .await?;
                    let mut new_children = BTreeMap::new();
                    for (next_byte, replaced_node) in replaced {
                        if !replaced_node.is_empty() {
                            let previous = new_children.insert(next_byte, replaced_node);
                            debug_assert!(previous.is_none());
                        }
                    }

                    let new_size: usize = edges.values().map(|edge| edge.size).sum::<usize>()
                        + new_children.values().map(|v| v.size()).sum::<usize>()
                        + value.iter().len();

                    if new_size <= shard_size {
                        // Case 4.3.1: Compress node into terminal node.
                        // For simplicity, reuse into_entries.
                        // In practice, all children will be terminal nodes, so nothing extra
                        // will be unecessarily persisted into the blobstore.
                        for (byte, node) in new_children {
                            debug_assert!(matches!(node, Self::Terminal { .. }));
                            let previous = edges.insert(
                                byte,
                                ShardedMapEdge {
                                    size: node.size(),
                                    child: ShardedMapChild::Inlined(node),
                                },
                            );
                            debug_assert!(previous.is_none());
                        }
                        let values = Self::intermediate(prefix, value, edges)
                            .into_entries(ctx, blobstore)
                            // Extending SortedVectorMap 1 by 1 will be fast because into_entries
                            // returns elements in order
                            .try_collect()
                            .await?;

                        Ok(Self::Terminal { values })
                    } else {
                        // Case 4.3.2: This will continue being a intermediate node, let's
                        // inline what's necessary and store everything
                        let new_edges = stream::iter(new_children)
                            .map(|(byte, node)| async move {
                                let size = node.size();
                                let child = match &node {
                                    // Design decision: Inline all intermediate nodes and store
                                    // terminal nodes separated
                                    Self::Intermediate { .. } => ShardedMapChild::Inlined(node),
                                    Self::Terminal { .. } => ShardedMapChild::Id(
                                        node.into_blob().store(ctx, blobstore).await?,
                                    ),
                                };
                                Ok((byte, ShardedMapEdge { size, child }))
                            })
                            .buffer_unordered(100)
                            .try_collect::<Vec<_>>()
                            .await?;
                        for (byte, edge) in new_edges {
                            let previous = edges.insert(byte, edge);
                            debug_assert!(previous.is_none());
                        }
                        debug_assert!(!edges.is_empty());
                        if edges.len() == 1 && value.is_none() {
                            // Unwrap safety: edges.len() == 1 above
                            let (byte, edge) = edges.into_iter().next().unwrap();
                            let mut child = edge.load_child(ctx, blobstore).await?;
                            prefix.push(byte);
                            child.prepend(prefix);
                            Ok(child)
                        } else {
                            Ok(Self::intermediate(prefix, value, edges))
                        }
                    }
                }
            }
        }
    }

    /// Iterates through all values in the map, asynchronously and only loading
    /// blobs as needed.
    // See the detailed description of the logic in docs/sharded_map.md
    pub fn into_entries<'a>(
        self,
        ctx: &'a CoreContext,
        blobstore: &'a impl Blobstore,
    ) -> impl Stream<Item = Result<(SmallBinary, Value)>> + 'a {
        self.into_prefix_entries(ctx, blobstore, &[])
    }

    pub fn into_prefix_entries<'a>(
        self,
        ctx: &'a CoreContext,
        blobstore: &'a impl Blobstore,
        prefix: &'a [u8],
    ) -> impl Stream<Item = Result<(SmallBinary, Value)>> + 'a {
        // TODO: prefix
        bounded_traversal_ordered_stream(
            nonzero!(256usize),
            nonzero!(256usize),
            vec![(
                self.size(),
                (SmallBinary::new(), prefix, ShardedMapChild::Inlined(self)),
            )],
            move |(mut cur_prefix, remaining_prefix, child): (
                SmallBinary,
                &[u8],
                ShardedMapChild<Value>,
            )| {
                async move {
                    Ok(match child.load(ctx, blobstore).await? {
                        // Case 1. Prepend all keys with cur_prefix and output elements
                        Self::Terminal { values } => values
                            .into_iter()
                            .filter(|(k, _)| k.starts_with(remaining_prefix))
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
                            edges,
                            ..
                        } => {
                            let remaining_prefix = if remaining_prefix.len() >= new_prefix.len() {
                                if let Some(new_remaining) =
                                    remaining_prefix.strip_prefix(new_prefix.as_slice())
                                {
                                    new_remaining
                                } else {
                                    // prefix doesn't match
                                    return Ok(vec![]);
                                }
                            } else {
                                if new_prefix.starts_with(remaining_prefix) {
                                    &[]
                                } else {
                                    // prefix doesn't match
                                    return Ok(vec![]);
                                }
                            };
                            // Step 2-a. Extend cur_prefix
                            cur_prefix.extend(new_prefix);
                            let cur_prefix = &cur_prefix;
                            remaining_prefix
                                .is_empty()
                                .then_some(value)
                                .flatten()
                                // Step 2-b. If value is present (and prefix empty), output (cur_prefix, value)
                                .map(|value| OrderedTraversal::Output((cur_prefix.clone(), value)))
                                .into_iter()
                                // Step 2-c. Copy prefix, append byte, and recurse.
                                .chain(edges.into_iter().filter_map(|(byte, edge)| {
                                    let (first, rest) =
                                        remaining_prefix.split_first().unwrap_or((&byte, &[]));
                                    if *first == byte {
                                        let mut new_prefix = cur_prefix.clone();
                                        new_prefix.push(byte);
                                        let size_prediction = edge.size;
                                        Some(OrderedTraversal::Recurse(
                                            size_prediction,
                                            (new_prefix, rest, edge.child),
                                        ))
                                    } else {
                                        // Byte didn't match prefix
                                        None
                                    }
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
            Self::Intermediate { .. } => self.size() == 0,
        }
    }

    fn size(&self) -> usize {
        match self {
            Self::Terminal { values } => values.len(),
            Self::Intermediate {
                value, edges, size, ..
            } => *size.get_or_init(|| {
                value.iter().len() + edges.values().map(|edge| edge.size).sum::<usize>()
            }),
        }
    }
}

impl<Value: MapValue> ThriftConvert for ShardedMapNode<Value> {
    const NAME: &'static str = "ShardedMapNode";
    type Thrift = thrift::ShardedMapNode;

    fn from_thrift(t: thrift::ShardedMapNode) -> Result<Self> {
        Ok(match t {
            thrift::ShardedMapNode::intermediate(intermediate) => Self::Intermediate {
                prefix: intermediate.prefix.0,
                value: intermediate
                    .value
                    .as_ref()
                    .map(ThriftConvert::from_bytes)
                    .transpose()?,
                edges: intermediate
                    .edges
                    .into_iter()
                    .map(|(k, e)| Ok((k as u8, ShardedMapEdge::from_thrift(e)?)))
                    .collect::<Result<_>>()?,
                size: Default::default(),
            },
            thrift::ShardedMapNode::terminal(terminal) => Self::Terminal {
                values: terminal
                    .values
                    .into_iter()
                    .map(|(k, v)| Ok((k.0, Value::from_bytes(&v)?)))
                    .collect::<Result<_>>()?,
            },
            thrift::ShardedMapNode::UnknownField(_) => bail!("Unknown map node variant"),
        })
    }

    fn into_thrift(self) -> thrift::ShardedMapNode {
        match self {
            Self::Intermediate {
                prefix,
                value,
                edges,
                ..
            } => thrift::ShardedMapNode::intermediate(thrift::ShardedMapIntermediateNode {
                prefix: thrift::small_binary(prefix),
                value: value.map(ThriftConvert::into_bytes),
                edges: edges
                    .into_iter()
                    .map(|(k, e)| (k as i8, e.into_thrift()))
                    .collect(),
            }),
            Self::Terminal { values } => {
                thrift::ShardedMapNode::terminal(thrift::ShardedMapTerminalNode {
                    values: values
                        .into_iter()
                        .map(|(k, v)| (thrift::small_binary(k), v.into_bytes()))
                        .collect(),
                })
            }
        }
    }
}

impl<Value: MapValue> BlobstoreValue for ShardedMapNode<Value> {
    type Key = Value::Id;

    fn into_blob(self) -> Blob<Self::Key> {
        let data = self.into_bytes();
        let id = Value::Context::id_from_data(&data);
        Blob::new(id, data)
    }

    fn from_blob(blob: Blob<Self::Key>) -> Result<Self> {
        Self::from_bytes(blob.data())
    }
}

#[cfg(test)]
mod test {
    use std::str::FromStr;

    use async_trait::async_trait;
    use blobstore::BlobstoreKeyParam;
    use blobstore::BlobstoreKeyRange;
    use blobstore::BlobstoreKeySource;
    use blobstore::LoadableError;
    use context::CoreContext;
    use fbinit::FacebookInit;
    use futures::TryStreamExt;
    use memblob::Memblob;
    use pretty_assertions::assert_eq;
    use quickcheck::Arbitrary;
    use quickcheck::Gen;
    use quickcheck::QuickCheck;
    use quickcheck::TestResult;
    use quickcheck::Testable;
    use ShardedMapNode::*;

    use super::*;
    use crate::impl_typed_hash;
    use crate::private::Blake2;
    use crate::typed_hash::BlobstoreKey;

    #[derive(Debug, Clone, Copy, Eq, PartialEq)]
    pub struct MyType(i32);

    #[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Debug, Hash)]
    pub struct ShardedMapNodeMyId(Blake2);

    impl_typed_hash! {
        hash_type => ShardedMapNodeMyId,
        thrift_hash_type => thrift::ShardedMapNodeId,
        value_type => ShardedMapNode<MyType>,
        context_type => ShardedMapNodeMyContext,
        context_key => "mytype.mapnode",
    }

    impl MapValue for MyType {
        type Id = ShardedMapNodeMyId;
        type Context = ShardedMapNodeMyContext;
    }

    impl ThriftConvert for MyType {
        const NAME: &'static str = "MyType";
        type Thrift = i32;
        fn into_thrift(self) -> Self::Thrift {
            self.0
        }
        fn from_thrift(t: Self::Thrift) -> Result<Self> {
            Ok(MyType(t))
        }
    }

    type TestShardedMap = ShardedMapNode<MyType>;

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
        ShardedMapNode::Intermediate {
            prefix: SmallVec::from_slice(prefix.as_bytes()),
            value: value.map(MyType),
            edges: children
                .into_iter()
                .map(|(c, v)| {
                    (
                        c as u32 as u8,
                        ShardedMapEdge {
                            size: v.size(),
                            child: ShardedMapChild::Inlined(v),
                        },
                    )
                })
                .collect(),
            size: Default::default(),
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

    const EXAMPLE_ENTRIES: &[(&str, i32)] = &[
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
    ];

    fn assert_round_trip(map: TestShardedMap) {
        let map_t = map.clone().into_thrift();
        // This is not deep equality through blobstore
        assert_eq!(ShardedMapNode::from_thrift(map_t).unwrap(), map);
    }

    #[derive(Clone)]
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

        fn prefix_entries<'a>(
            &'a self,
            prefix: &'a str,
        ) -> impl Stream<Item = Result<(String, i32)>> + 'a {
            self.0
                .clone()
                .into_prefix_entries(&self.1, &self.2, prefix.as_bytes())
                .and_then(|(k, v)| async move { Ok((String::from_utf8(k.to_vec())?, v.0)) })
        }

        async fn assert_entries(&self, entries: &[(&str, i32)]) -> Result<()> {
            assert_eq!(
                self.entries().try_collect::<Vec<_>>().await?,
                entries
                    .iter()
                    .map(|(k, v)| (String::from(*k), *v))
                    .collect::<Vec<_>>()
            );
            Ok(())
        }

        async fn assert_prefix_entries(&self, prefix: &str, entries: &[(&str, i32)]) -> Result<()> {
            assert_eq!(
                self.prefix_entries(prefix).try_collect::<Vec<_>>().await?,
                entries
                    .iter()
                    .map(|(k, v)| (String::from(*k), *v))
                    .collect::<Vec<_>>()
            );
            Ok(())
        }

        async fn add_remove(
            &mut self,
            to_add: &[(&str, i32)],
            to_remove: &[&str],
        ) -> Result<Vec<i32>> {
            let map = std::mem::take(&mut self.0);
            let (send, recv) = crossbeam::channel::unbounded();
            self.0 = map
                .update(
                    &self.1,
                    &self.2,
                    to_add
                        .iter()
                        .map(|(k, v)| (Bytes::copy_from_slice(k.as_bytes()), Some(MyType(*v))))
                        .chain(
                            to_remove
                                .iter()
                                .map(|k| (Bytes::copy_from_slice(k.as_bytes()), None)),
                        )
                        .collect(),
                    |x| send.send(x.0).unwrap(),
                )
                .await?;
            self.validate().await?;
            Ok(recv.try_iter().collect())
        }

        #[async_recursion]
        async fn validate(&self) -> Result<()> {
            let size = self.size();
            assert_eq!(self.0.is_empty(), size == 0);
            match &self.0 {
                Terminal { values } => assert!(values.len() <= 5),
                Intermediate {
                    prefix: _,
                    value,
                    edges,
                    size: _,
                } => {
                    let children_size: usize = stream::iter(
                        edges
                            .into_iter()
                            .map(|(_, e)| async move {
                                anyhow::Ok((e.size, e.child.clone().load(&self.1, &self.2).await?))
                            })
                            // prevent compiler bug
                            .collect::<Vec<_>>(),
                    )
                    .buffer_unordered(100)
                    .and_then(|(size, child)| async move {
                        let child = Self(child, self.1.clone(), self.2.clone());
                        child.validate().await?;
                        assert_eq!(child.size(), size);
                        Ok(size)
                    })
                    .try_collect::<Vec<_>>()
                    .await?
                    .into_iter()
                    .sum();
                    assert_eq!(children_size + value.iter().len(), size);
                }
            }
            Ok(())
        }

        #[async_recursion]
        async fn inner_inline_all(
            &self,
            mut map: ShardedMapNode<MyType>,
        ) -> Result<ShardedMapNode<MyType>> {
            match &mut map {
                Terminal { .. } => {}
                Intermediate { edges, .. } => {
                    for (_, edge) in edges {
                        let node = std::mem::take(&mut edge.child)
                            .load(&self.1, &self.2)
                            .await?;
                        edge.child = ShardedMapChild::Inlined(self.inner_inline_all(node).await?);
                    }
                }
            }
            Ok(map)
        }

        async fn inline_all(&mut self) -> Result<()> {
            let map = std::mem::take(&mut self.0);
            self.0 = self.inner_inline_all(map).await?;
            Ok(())
        }

        async fn child(&self, key: char) -> Result<Self> {
            let child = match &self.0 {
                Terminal { .. } => bail!("terminal"),
                Intermediate { edges, .. } => {
                    edges
                        .get(&(key as u8))
                        .unwrap()
                        .child
                        .clone()
                        .load(&self.1, &self.2)
                        .await?
                }
            };
            Ok(Self(child, self.1.clone(), self.2.clone()))
        }

        fn assert_terminal(&self, values_len: usize) {
            match &self.0 {
                Intermediate { .. } => panic!("not terminal"),
                Terminal { values } => assert_eq!(values.len(), values_len),
            }
        }
        fn assert_intermediate(&self, child_count: usize) {
            match &self.0 {
                Terminal { .. } => panic!("not intermediate"),
                Intermediate { edges, .. } => assert_eq!(edges.len(), child_count),
            }
        }
        fn assert_prefix(&self, prefix: &str) {
            match &self.0 {
                Terminal { .. } => panic!("not intermediate"),
                Intermediate {
                    prefix: my_prefix, ..
                } => assert_eq!(my_prefix.as_slice(), prefix.as_bytes()),
            }
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
            edges: Default::default(),
            prefix: Default::default(),
            size: Default::default(),
        };
        assert!(empty.is_empty());
        assert_eq!(empty.size(), 0);
        assert!(ShardedMapNode::<MyType>::default().is_empty());

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
        map.assert_entries(EXAMPLE_ENTRIES).await?;
        map.assert_prefix_entries("", EXAMPLE_ENTRIES).await?;
        map.assert_prefix_entries("aba", &EXAMPLE_ENTRIES[0..8])
            .await?;
        map.assert_prefix_entries("abaca", &EXAMPLE_ENTRIES[1..6])
            .await?;
        map.assert_prefix_entries("omi", &EXAMPLE_ENTRIES[8..10])
            .await?;
        map.assert_prefix_entries("om", &EXAMPLE_ENTRIES[8..])
            .await?;
        map.assert_prefix_entries("o", &EXAMPLE_ENTRIES[8..])
            .await?;
        map.assert_prefix_entries("ban", &[]).await?;
        Ok(())
    }

    async fn get_all_keys(
        ctx: &CoreContext,
        blobstore: &impl BlobstoreKeySource,
    ) -> Result<impl Iterator<Item = String>> {
        let data = blobstore
            .enumerate(
                ctx,
                &BlobstoreKeyParam::Start(BlobstoreKeyRange {
                    begin_key: String::new(),
                    end_key: String::new(),
                }),
            )
            .await?;
        if data.next_token.is_some() {
            unimplemented!();
        }
        let mut data: Vec<_> = data.keys.into_iter().collect();
        data.sort();
        Ok(data.into_iter())
    }

    async fn assert_all_keys(
        ctx: &CoreContext,
        blobstore: &impl BlobstoreKeySource,
        keys: Vec<&str>,
    ) -> Result<()> {
        assert_eq!(
            get_all_keys(ctx, blobstore).await?.collect::<Vec<_>>(),
            keys.into_iter().map(String::from).collect::<Vec<_>>()
        );
        Ok(())
    }

    async fn assert_key_count(
        ctx: &CoreContext,
        blobstore: &impl BlobstoreKeySource,
        count: usize,
    ) -> Result<()> {
        assert_eq!(get_all_keys(ctx, blobstore).await?.count(), count);
        Ok(())
    }

    #[fbinit::test]
    async fn store_test(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let blobstore = Memblob::default();
        let map = example_map();
        map.into_blob().store(&ctx, &blobstore).await?;
        assert_all_keys(&ctx, &blobstore, vec!["mytype.mapnode.blake2.9239707907ceb346d7146c601f131ab7c598a8e98441c2934817c964f0a2c270"]).await?;
        Ok(())
    }

    #[fbinit::test]
    async fn update_basic_test(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let blobstore = Memblob::default();
        let mut map = MapHelper(Default::default(), ctx.clone(), blobstore.clone());
        map.assert_entries(&[]).await?;
        map.add_remove(EXAMPLE_ENTRIES, &[]).await?;
        map.assert_entries(EXAMPLE_ENTRIES).await?;
        assert_all_keys(
            &ctx,
            &blobstore,
            vec!["mytype.mapnode.blake2.8c8f56e9612f7cc94187729e4eff067bf56bb239019e25c8421243a60e4d1fb9",
                "mytype.mapnode.blake2.e808435da65aa2e0f61db333fba1904e57b1b46dff2d3a5c263d0016750f1f0d",
                "mytype.mapnode.blake2.f8728f3aabc9b8083db7150e9c2636c2971dac0784fac59cee3b7b6908165476"],
        )
        .await?;
        {
            // Let's compare it to our hand-written map
            let mut map = map.clone();
            map.inline_all().await?;
            map.assert_entries(EXAMPLE_ENTRIES).await?;
            assert_eq!(map.0, example_map());
        }
        let deleted = map.add_remove(&[], &["abalaba", "non_existing"]).await?;
        assert_eq!(deleted, vec![5]);
        assert_eq!(map.0.size(), EXAMPLE_ENTRIES.len() - 1);
        assert_key_count(&ctx, &blobstore, 4).await?;
        let deleted = map.add_remove(&[], &["abalada"]).await?;
        assert_eq!(deleted, vec![6]);
        // Intermeditate node should now have 1 child, but also a value
        assert_key_count(&ctx, &blobstore, 4).await?;
        let child = map.child('a').await?;
        match child.0 {
            Terminal { .. } => bail!("not intermediate"),
            Intermediate { value, edges, .. } => {
                assert!(value.is_some());
                assert_eq!(edges.len(), 1);
            }
        }
        let deleted = map.add_remove(&[], &["aba"]).await?;
        assert_eq!(deleted, vec![12]);
        // Intermediate node without a value should be merged
        assert_key_count(&ctx, &blobstore, 5).await?;
        map.assert_entries(&[
            ("abacab", 7),
            ("abacaba", 8),
            ("abacakkk", 9),
            ("abacate", 10),
            ("abacaxi", 11),
            ("omiojo", 1),
            ("omiux", 2),
            ("omundo", 3),
            ("omungal", 4),
        ])
        .await?;
        map.child('a').await?.assert_terminal(5);
        assert!(
            map.add_remove(&[], &["abalada", "abalaba", "aba"])
                .await?
                .is_empty()
        );
        assert_eq!(
            map.add_remove(&[("potato", 1000), ("abacaxi", 1001)], &[])
                .await?,
            vec![11]
        );
        Ok(())
    }

    #[fbinit::test]
    async fn update_tricky_test(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let blobstore = Memblob::default();
        let mut map = MapHelper(Default::default(), ctx.clone(), blobstore.clone());
        map.add_remove(
            &[
                ("A11", 1),
                ("A12", 2),
                ("A13", 3),
                ("A21", 1),
                ("A22", 2),
                ("A23", 3),
            ],
            &[],
        )
        .await?;
        map.assert_intermediate(2);
        map.child('1').await?.assert_terminal(3);
        map.child('2').await?.assert_terminal(3);
        // LCP of keys is smaller than prefix only due to removals.
        map.add_remove(&[("A14", 4)], &["cz", "A", "A31"]).await?;
        map.assert_intermediate(2);
        map.child('1').await?.assert_terminal(4);
        map.child('2').await?.assert_terminal(3);
        map.add_remove(
            &[
                ("B11", 1),
                ("B21", 1),
                ("B22", 2),
                ("B23", 3),
                ("B24", 4),
                ("B31", 1),
            ],
            &["A11", "A12", "A13", "A14", "A21", "A22", "A23"],
        )
        .await?;
        map.assert_intermediate(3);
        map.child('1').await?.assert_terminal(1);
        map.child('2').await?.assert_terminal(4);
        map.child('3').await?.assert_terminal(1);
        map.add_remove(&[], &[""]).await?;
        Ok(())
    }

    #[fbinit::test]
    async fn update_tricky_deletes_test(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let blobstore = Memblob::default();
        let mut map = MapHelper(Default::default(), ctx.clone(), blobstore.clone());
        map.add_remove(EXAMPLE_ENTRIES, &[]).await?;
        // Removing something that is not a prefix of the intermediate node
        // can panic if not done correctly
        map.add_remove(&[], &[""]).await?;
        map.add_remove(&[], &["a"]).await?;
        map.add_remove(&[], &["ab"]).await?;
        // Bug where we might mismatch prefix of deleted keys
        map.add_remove(&[], &["abx"]).await?;
        assert_eq!(map.size(), 12);
        map.add_remove(&[], &["abxlada"]).await?;
        assert_eq!(map.size(), 12);
        // Let's play with the value of an intermediate node and assert all is still good:
        map.add_remove(&[], &["aba"]).await?;
        assert_eq!(map.size(), 11);
        let child = map.child('a').await?;
        assert_eq!(child.size(), 7);
        assert_eq!(map.lookup("aba").await?, None);
        map.add_remove(&[("aba", 0)], &[]).await?;
        assert_eq!(map.size(), 12);
        assert_eq!(map.lookup("aba").await?, Some(0));
        map.add_remove(&[("aba", -1)], &[]).await?;
        assert_eq!(map.size(), 12);
        assert_eq!(map.lookup("aba").await?, Some(-1));
        Ok(())
    }

    #[fbinit::test]
    async fn update_cases_test(fb: FacebookInit) -> Result<()> {
        // Let's try to do updates that cause different cases and assert it all works out
        let ctx = CoreContext::test_mock(fb);
        let blobstore = Memblob::default();
        let mut map = MapHelper(Default::default(), ctx.clone(), blobstore.clone());
        // Case 1
        map.add_remove(&[("_a", 1), ("_b", 2), ("_c", 3)], &[])
            .await?;
        map.assert_terminal(3);
        // Case 2 > Case 4 > (Recursive Case 1's) > Case 4.3.2
        map.add_remove(&[("_d", 4), ("_e", 5), ("_f", 6)], &[])
            .await?;
        map.assert_intermediate(6);
        map.child('d').await?.assert_terminal(1);
        // Case 4 > (Recursive Case 1) > case 4.3.1
        map.add_remove(&[], &["_b"]).await?;
        map.assert_terminal(5);
        // Case 2 > Case 3 > Case 4 > (Recursive Case 1, Case 2>...) > Case 4.3.2
        map.add_remove(&[("_b", 2), ("z", -1)], &[]).await?;
        map.assert_intermediate(2);
        map.assert_prefix("");
        map.child('_').await?.assert_intermediate(6);
        map.child('z').await?.assert_terminal(1);
        // Case 4 > (Rec Case 1) > Case 4.3.2 + merge
        map.add_remove(&[], &["z"]).await?;
        map.assert_intermediate(6);
        map.assert_prefix("_");
        // Case 3 > Case 4 > Rec Case 1 > Case 4.3.2
        map.add_remove(&[("y", -2)], &[]).await?;
        map.assert_intermediate(2);
        map.assert_prefix("");
        // Case 4 > (Rec Case 1, Case 4>Rec Case1>Case 4.3.1) > Case 4.3.1
        map.add_remove(&[], &["_b", "y"]).await?;
        map.assert_terminal(5);
        map.assert_entries(&[("_a", 1), ("_c", 3), ("_d", 4), ("_e", 5), ("_f", 6)])
            .await?;
        Ok(())
    }

    #[fbinit::test]
    async fn update_case_3_test(fb: FacebookInit) -> Result<()> {
        // Let's try an update that causes case 3 and do detailed asserting
        let ctx = CoreContext::test_mock(fb);
        let blobstore = Memblob::default();
        let mut map = MapHelper(Default::default(), ctx.clone(), blobstore.clone());
        map.add_remove(
            &[
                ("abc1", 1),
                ("abc2", 2),
                ("abc3", 3),
                ("abc4", 4),
                ("abc5", 5),
                ("abc6", 6),
            ],
            &[],
        )
        .await?;
        map.assert_intermediate(6);
        map.assert_prefix("abc");
        map.add_remove(&[("a1", 1)], &[]).await?;
        map.assert_intermediate(2);
        map.assert_prefix("a");
        let childb = map.child('b').await?;
        childb.assert_prefix("c");
        childb.assert_intermediate(6);
        let child1 = map.child('1').await?;
        child1.assert_terminal(1);
        map.assert_entries(&[
            ("a1", 1),
            ("abc1", 1),
            ("abc2", 2),
            ("abc3", 3),
            ("abc4", 4),
            ("abc5", 5),
            ("abc6", 6),
        ])
        .await?;
        Ok(())
    }

    #[fbinit::test]
    fn round_trip_quickcheck(fb: FacebookInit) {
        let ctx = CoreContext::test_mock(fb);
        let blobstore = Memblob::default();
        use tokio::runtime::Runtime;

        struct Roundtrip(Runtime, CoreContext, Memblob);
        impl Testable for Roundtrip {
            fn result(&self, gen: &mut Gen) -> TestResult {
                let res = self.0.block_on(async {
                    let values: BTreeMap<String, i32> = Arbitrary::arbitrary(gen);
                    let mut queries: Vec<String> = Arbitrary::arbitrary(gen);
                    let keys: Vec<&String> = values.keys().collect();
                    for _ in 0..values.len() / 2 {
                        queries.push(gen.choose(&keys).unwrap().to_string());
                    }
                    let mut map = MapHelper(Default::default(), self.1.clone(), self.2.clone());
                    let adds = values
                        .iter()
                        .map(|(k, v)| (k.as_str(), *v))
                        .collect::<Vec<_>>();
                    map.add_remove(&adds, &[]).await?;
                    if map.size() != values.len() {
                        return Ok(false);
                    }
                    for k in queries {
                        let correct_v = values.get(&k);
                        let test_v = map.lookup(&k).await?;
                        if correct_v.cloned() != test_v {
                            return Ok(false);
                        }
                    }
                    let test_roundtrip = map.entries().try_collect::<BTreeMap<_, _>>().await?;
                    Ok(values == test_roundtrip)
                });
                TestResult::from_bool(matches!(res, Result::Ok(true)))
            }
        }

        QuickCheck::new().quickcheck(Roundtrip(Runtime::new().unwrap(), ctx, blobstore));
    }
}
