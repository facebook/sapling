/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fmt::Debug;
use std::sync::OnceLock;

use anyhow::bail;
use anyhow::Context;
use anyhow::Result;
use async_recursion::async_recursion;
use blobstore::Blobstore;
use blobstore::Loadable;
use blobstore::Storable;
use context::CoreContext;
use derivative::Derivative;
use futures::stream;
use futures::StreamExt;
use futures::TryStreamExt;
use itertools::Either;
use smallvec::SmallVec;
use sorted_vector_map::SortedVectorMap;

use crate::blob::Blob;
use crate::blob::BlobstoreValue;
use crate::thrift;
use crate::typed_hash::IdContext;
use crate::typed_hash::MononokeId;
use crate::ThriftConvert;
use crate::TrieMap;

// More detailed documentation about ShardedMapV2 can be found in mononoke_types_thrift.thrift

pub trait ShardedMapV2Value: ThriftConvert + Debug + Clone + Send + Sync + 'static {
    type NodeId: MononokeId<Thrift = thrift::ShardedMapV2NodeId, Value = ShardedMapV2Node<Self>>;
    type Context: IdContext<Id = Self::NodeId>;
}

type SmallBinary = SmallVec<[u8; 24]>;

#[derive(Derivative)]
#[derivative(PartialEq, Debug, Default(bound = ""))]
#[derive(Clone, Eq)]
pub struct ShardedMapV2Node<Value: ShardedMapV2Value> {
    prefix: SmallBinary,
    value: Option<Value>,
    children: SortedVectorMap<u8, LoadableShardedMapV2Node<Value>>,
    #[derivative(PartialEq = "ignore", Debug = "ignore")]
    weight: OnceLock<usize>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum LoadableShardedMapV2Node<Value: ShardedMapV2Value> {
    Inlined(ShardedMapV2Node<Value>),
    Stored(ShardedMapV2StoredNode<Value>),
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ShardedMapV2StoredNode<Value: ShardedMapV2Value> {
    id: Value::NodeId,
    weight: usize,
}

impl<Value: ShardedMapV2Value> ShardedMapV2StoredNode<Value> {
    fn from_thrift(t: thrift::ShardedMapV2StoredNode) -> Result<Self> {
        Ok(Self {
            id: Value::NodeId::from_thrift(t.id)?,
            weight: t.weight as usize,
        })
    }

    fn into_thrift(self) -> thrift::ShardedMapV2StoredNode {
        thrift::ShardedMapV2StoredNode {
            id: self.id.into_thrift(),
            weight: self.weight as i64,
        }
    }
}

impl<Value: ShardedMapV2Value> LoadableShardedMapV2Node<Value> {
    /// Returns the underlying node, fetching from the blobstore
    /// if it's not inlined.
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
                .with_context(|| "Failed to load stored sharded map node"),
        }
    }

    /// Returns an inlined variant of self.
    pub async fn inlined(
        self,
        ctx: &CoreContext,
        blobstore: &impl Blobstore,
    ) -> Result<LoadableShardedMapV2Node<Value>> {
        match self {
            inlined @ Self::Inlined(_) => Ok(inlined),
            Self::Stored(stored) => Ok(Self::Inlined(stored.id.load(ctx, blobstore).await?)),
        }
    }

    /// Returns a stored variant of self.
    pub async fn stored(
        self,
        ctx: &CoreContext,
        blobstore: &impl Blobstore,
    ) -> Result<LoadableShardedMapV2Node<Value>> {
        match self {
            Self::Inlined(inlined) => Ok(Self::Stored(ShardedMapV2StoredNode {
                weight: inlined.weight(),
                id: inlined.into_blob().store(ctx, blobstore).await?,
            })),
            stored @ Self::Stored(_) => Ok(stored),
        }
    }

    /// Returns the weight of the underlying node.
    fn weight(&self) -> usize {
        match self {
            Self::Inlined(inlined) => inlined.weight(),
            Self::Stored(stored) => stored.weight,
        }
    }

    fn from_thrift(t: thrift::LoadableShardedMapV2Node) -> Result<Self> {
        Ok(match t {
            thrift::LoadableShardedMapV2Node::inlined(inlined) => {
                Self::Inlined(ShardedMapV2Node::from_thrift(inlined)?)
            }
            thrift::LoadableShardedMapV2Node::stored(stored) => {
                Self::Stored(ShardedMapV2StoredNode::from_thrift(stored)?)
            }
            thrift::LoadableShardedMapV2Node::UnknownField(_) => bail!("Unknown variant"),
        })
    }

    fn into_thrift(self) -> thrift::LoadableShardedMapV2Node {
        match self {
            Self::Inlined(inlined) => {
                thrift::LoadableShardedMapV2Node::inlined(inlined.into_thrift())
            }
            Self::Stored(stored) => thrift::LoadableShardedMapV2Node::stored(stored.into_thrift()),
        }
    }
}

impl<Value: ShardedMapV2Value> ShardedMapV2Node<Value> {
    fn weight_limit() -> Result<usize> {
        if cfg!(test) {
            Ok(5)
        } else {
            thrift::SHARDED_MAP_V2_WEIGHT_LIMIT
                .try_into()
                .context("Failed to parse weight limit")
        }
    }

    fn weight(&self) -> usize {
        *self.weight.get_or_init(|| {
            self.value.is_some() as usize
                + self
                    .children
                    .iter()
                    .fold(0, |acc, (_byte, child)| match child {
                        LoadableShardedMapV2Node::Inlined(inlined) => acc + inlined.weight(),
                        LoadableShardedMapV2Node::Stored(_) => acc + 1,
                    })
        })
    }

    /// Create a ShardedMapV2Node out of an iterator of key-value pairs.
    pub async fn from_entries(
        ctx: &CoreContext,
        blobstore: &impl Blobstore,
        entries: impl IntoIterator<Item = (impl AsRef<[u8]>, Value)>,
    ) -> Result<Self> {
        Self::from_entries_and_partial_maps(
            ctx,
            blobstore,
            entries
                .into_iter()
                .map(|(key, value)| (key, Either::Left(value)))
                .collect(),
        )
        .await
    }

    /// Create a ShardedMapV2Node from a TrieMap of values and partial maps (LoadableShardedMapV2Nodes). The key
    /// for every input partial map is a prefix that's prepended to it, which represents that keys that have this
    /// prefix are all contained in that partial map.
    /// Returns an error if the key for a partial map is a prefix of any other key in the input TrieMap.
    pub async fn from_entries_and_partial_maps(
        ctx: &CoreContext,
        blobstore: &impl Blobstore,
        entries: TrieMap<Either<Value, LoadableShardedMapV2Node<Value>>>,
    ) -> Result<Self> {
        Self::from_entries_inner(ctx, blobstore, entries)
            .await?
            .load(ctx, blobstore)
            .await
    }

    #[async_recursion]
    async fn from_entries_inner(
        ctx: &CoreContext,
        blobstore: &impl Blobstore,
        entries: TrieMap<Either<Value, LoadableShardedMapV2Node<Value>>>,
    ) -> Result<LoadableShardedMapV2Node<Value>> {
        let (lcp, entries) = entries.split_longest_common_prefix();
        let (current_entry, children) = entries.expand();

        let current_value = match current_entry {
            Some(Either::Right(partial_map)) => {
                // If there's a partial map in the input TrieMap corresponding to the longest common prefix,
                // then it should be the only entry in the TrieMap, otherwise there's a conflict.
                if !children.is_empty() {
                    bail!("Cannot create sharded map node with conflicting entries");
                }

                // If the longest common prefix is empty we can reuse the partial map directly, otherwise
                // we have to load it in order to append the longest common prefix to it.
                match lcp.is_empty() {
                    true => return Ok(partial_map),
                    false => {
                        let mut node = partial_map.load(ctx, blobstore).await?;
                        node.prefix.insert_from_slice(0, &lcp);
                        return Ok(LoadableShardedMapV2Node::Inlined(node));
                    }
                }
            }
            Some(Either::Left(value)) => Some(value),
            None => None,
        };

        // The weight of a node is defined as the sum of weights of all its inlined children,
        // plus the count of its non-inlined children, plus one if it contains a value itself.

        let weight_limit = Self::weight_limit()?;

        // Assume that all children are not going to be inlined, then the weight of the
        // node will be the number of children plus one if the current node has a value.
        let weight = &mut (current_value.is_some() as usize + children.len());

        let children_pre_inlining_futures = children
            .into_iter()
            .map(|(next_byte, entries)| async move {
                let child = Self::from_entries_inner(ctx, blobstore, entries).await?;
                anyhow::Ok((next_byte, child))
            })
            .collect::<Vec<_>>();
        let children_pre_inlining = stream::iter(children_pre_inlining_futures)
            .buffer_unordered(100)
            .try_collect::<SortedVectorMap<_, _>>()
            .await?;

        // Go through each child in order and check if inlining will not cause the weight
        // of the current node to go beyond the weight limit.
        let children_futures = children_pre_inlining
            .into_iter()
            .map(|(next_byte, child)| {
                if *weight + child.weight() - 1 <= weight_limit {
                    // Below limit: inline it.
                    *weight += child.weight() - 1;
                    Either::Left(async move {
                        anyhow::Ok((next_byte, child.inlined(ctx, blobstore).await?))
                    })
                } else {
                    // Breaches limit: store as separate blob.
                    Either::Right(
                        async move { Ok((next_byte, child.stored(ctx, blobstore).await?)) },
                    )
                }
            })
            .collect::<Vec<_>>();
        let children = stream::iter(children_futures)
            .buffer_unordered(100)
            .try_collect::<SortedVectorMap<_, _>>()
            .await?;

        Ok(LoadableShardedMapV2Node::Inlined(Self {
            prefix: lcp,
            value: current_value,
            children,
            weight: OnceLock::from(*weight),
        }))
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
                .map(|(key, child)| Ok((key as u8, LoadableShardedMapV2Node::from_thrift(child)?)))
                .collect::<Result<_>>()?,
            weight: OnceLock::new(),
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

#[cfg(test)]
mod test {
    use std::collections::BTreeMap;
    use std::str::FromStr;

    use anyhow::anyhow;
    use anyhow::bail;
    use async_recursion::async_recursion;
    use async_trait::async_trait;
    use blobstore::BlobstoreKeyParam;
    use blobstore::BlobstoreKeyRange;
    use blobstore::BlobstoreKeySource;
    use blobstore::LoadableError;
    use blobstore::Storable;
    use context::CoreContext;
    use fbinit::FacebookInit;
    use memblob::Memblob;
    use quickcheck::Arbitrary;
    use quickcheck::Gen;
    use quickcheck::QuickCheck;
    use quickcheck::TestResult;
    use quickcheck::Testable;

    use super::*;
    use crate::impl_typed_hash;
    use crate::private::Blake2;
    use crate::BlobstoreKey;

    #[derive(Debug, Clone, Copy, Eq, PartialEq)]
    pub struct TestValue(i32);

    #[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Debug, Hash)]
    pub struct ShardedMapV2NodeTestId(Blake2);

    impl_typed_hash! {
        hash_type => ShardedMapV2NodeTestId,
        thrift_hash_type => thrift::ShardedMapV2NodeId,
        value_type => ShardedMapV2Node<TestValue>,
        context_type => ShardedMapV2NodeTestContext,
        context_key => "test.map2node",
    }

    #[test]
    fn sharded_map_v2_blobstore_key() {
        let id = ShardedMapV2NodeTestId::from_byte_array([1; 32]);
        assert_eq!(id.blobstore_key(), format!("test.map2node.blake2.{}", id));
    }

    impl ShardedMapV2Value for TestValue {
        type NodeId = ShardedMapV2NodeTestId;
        type Context = ShardedMapV2NodeTestContext;
    }

    impl ThriftConvert for TestValue {
        const NAME: &'static str = "TestValue";
        type Thrift = i32;
        fn into_thrift(self) -> Self::Thrift {
            self.0
        }
        fn from_thrift(t: Self::Thrift) -> Result<Self> {
            Ok(TestValue(t))
        }
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

    fn check_round_trip(map: ShardedMapV2Node<TestValue>) -> Result<()> {
        let map_t = map.clone().into_thrift();

        if ShardedMapV2Node::from_thrift(map_t).unwrap() == map {
            Ok(())
        } else {
            Err(anyhow!(
                "converting sharded map node to thrift and back doesn't produce the same map."
            ))
        }
    }

    struct CalculatedValues {
        weight: usize,
    }

    #[derive(Clone)]
    struct MapHelper(CoreContext, Memblob);
    impl MapHelper {
        async fn from_entries_removed_prefix(
            &self,
            entries: &[(&str, i32)],
            prefix_len: usize,
        ) -> Result<ShardedMapV2Node<TestValue>> {
            ShardedMapV2Node::from_entries(
                &self.0,
                &self.1,
                entries
                    .iter()
                    .map(|(key, value)| (&key[prefix_len..], TestValue(*value)))
                    .collect::<TrieMap<_>>(),
            )
            .await
        }

        async fn from_entries(
            &self,
            entries: &[(&str, i32)],
        ) -> Result<ShardedMapV2Node<TestValue>> {
            self.from_entries_removed_prefix(entries, 0).await
        }

        async fn from_entries_and_partial_maps(
            &self,
            entries: &[(&str, Either<i32, ShardedMapV2Node<TestValue>>)],
        ) -> Result<ShardedMapV2Node<TestValue>> {
            ShardedMapV2Node::from_entries_and_partial_maps(
                &self.0,
                &self.1,
                entries
                    .iter()
                    .map(|(key, entry)| {
                        let entry = match entry {
                            Either::Left(value) => Either::Left(TestValue(*value)),
                            Either::Right(map) => {
                                Either::Right(LoadableShardedMapV2Node::Inlined(map.clone()))
                            }
                        };
                        (key.as_bytes(), entry)
                    })
                    .collect(),
            )
            .await
        }

        async fn check_example_map(&self, map: ShardedMapV2Node<TestValue>) -> Result<()> {
            self.check_sharded_map(map.clone()).await?;
            Ok(())
        }

        #[async_recursion]
        async fn check_sharded_map(
            &self,
            map: ShardedMapV2Node<TestValue>,
        ) -> Result<CalculatedValues> {
            check_round_trip(map.clone())?;

            // The minimum weight that this node could have is the number of its children
            // plus one if it has a value.
            let min_possible_weight = map.value.is_some() as usize + map.children.len();

            let weight_limit = ShardedMapV2Node::<TestValue>::weight_limit()?;

            // Bypass the weight limit check if map's weight is the minimum possible (i.e. all children are stored),
            // this is to avoid failing in quickcheck tests in which sometimes a node will have more than weight
            // limit number of children.
            if map.weight() > weight_limit && map.weight() != min_possible_weight {
                bail!("weight of sharded map node exceeds the limit");
            }

            let mut calculated_weight = map.value.is_some() as usize;

            for (_next_byte, child) in map.children.iter() {
                let child_calculated_values = self
                    .check_sharded_map(child.clone().load(&self.0, &self.1).await?)
                    .await?;

                match child {
                    LoadableShardedMapV2Node::Inlined(_) => {
                        calculated_weight += child_calculated_values.weight;
                    }
                    LoadableShardedMapV2Node::Stored(_) => {
                        calculated_weight += 1;
                    }
                }
            }

            if calculated_weight != map.weight() {
                bail!("weight of sharded map node does not match its calculated weight");
            }

            Ok(CalculatedValues {
                weight: calculated_weight,
            })
        }

        async fn assert_all_keys(&self, keys: &[&str]) -> Result<()> {
            let data = self
                .1
                .enumerate(
                    &self.0,
                    &BlobstoreKeyParam::Start(BlobstoreKeyRange {
                        begin_key: String::new(),
                        end_key: String::new(),
                    }),
                )
                .await?;

            let mut data: Vec<_> = data.keys.into_iter().collect();
            data.sort();

            assert_eq!(
                data,
                keys.iter()
                    .map(|key| String::from(*key))
                    .collect::<Vec<_>>()
            );
            Ok(())
        }

        async fn stored_node(
            &self,
            node: ShardedMapV2Node<TestValue>,
            weight: usize,
            blobstore_key: &str,
        ) -> Result<LoadableShardedMapV2Node<TestValue>> {
            let id = node.into_blob().store(&self.0, &self.1).await?;
            assert_eq!(id.blobstore_key().as_str(), blobstore_key);

            Ok(LoadableShardedMapV2Node::Stored(ShardedMapV2StoredNode {
                id,
                weight,
            }))
        }
    }

    fn test_node(
        prefix: &str,
        value: Option<i32>,
        children: Vec<(u8, LoadableShardedMapV2Node<TestValue>)>,
    ) -> ShardedMapV2Node<TestValue> {
        ShardedMapV2Node {
            prefix: SmallBinary::from(prefix.as_bytes()),
            value: value.map(TestValue),
            children: children.into_iter().collect(),
            weight: Default::default(),
        }
    }

    fn inlined_node(
        prefix: &str,
        value: Option<i32>,
        children: Vec<(u8, LoadableShardedMapV2Node<TestValue>)>,
    ) -> LoadableShardedMapV2Node<TestValue> {
        LoadableShardedMapV2Node::Inlined(test_node(prefix, value, children))
    }

    #[fbinit::test]
    async fn test_sharded_map_v2_example(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let blobstore = Memblob::default();
        let helper = MapHelper(ctx, blobstore);

        let from_entries_map = helper.from_entries(EXAMPLE_ENTRIES).await?;

        helper.assert_all_keys(&[
            "test.map2node.blake2.6f7dc1a2ad07d16eb4d3e586e2f7361c0990dcf4a29b0bb06fa5d04e69710a64",
            "test.map2node.blake2.d40e11f4f3f08ad21b5eb6bab17e0916d449bffde464048dfb27efa3f9c19cee"
        ]).await?;

        helper.check_example_map(from_entries_map.clone()).await?;

        // map_abacab:
        //     *=7
        //     |
        //     a=8
        let map_abacab = inlined_node("", Some(7), vec![(b'a', inlined_node("", Some(8), vec![]))]);
        // map_abac:
        //     *
        //     |
        //     a
        //     |____________________
        //     |     \       \      \
        //     b=7   kkk=9  te=10  xi=11
        //     |
        //     a=8
        let map_abac = helper
            .stored_node(
                test_node(
                    "a",
                    None,
                    vec![
                        (b'b', map_abacab),
                        (b'k', inlined_node("kk", Some(9), vec![])),
                        (b't', inlined_node("e", Some(10), vec![])),
                        (b'x', inlined_node("i", Some(11), vec![])),
                    ],
                ),
                5,
                "test.map2node.blake2.d40e11f4f3f08ad21b5eb6bab17e0916d449bffde464048dfb27efa3f9c19cee",
            )
            .await?;
        // map_abal:
        //      *
        //      |
        //      a
        //      |____
        //      |    \
        //     ba=5  da=6
        let map_abal = inlined_node(
            "a",
            None,
            vec![
                (b'b', inlined_node("a", Some(5), vec![])),
                (b'd', inlined_node("a", Some(6), vec![])),
            ],
        );
        // map_a:
        //     *
        //     |
        //     ba=12
        //     |_______________________________
        //     |                               \
        //     ca                               la
        //     |____________________            |____
        //     |     \       \      \           |    \
        //     b=7   kkk=9  te=10  xi=11      ba=5  da=6
        //     |
        //     a=8
        let map_a = inlined_node(
            "ba",
            Some(12),
            vec![(b'c', map_abac.clone()), (b'l', map_abal)],
        );
        // map_omi:
        //     *
        //     |______
        //     |      \
        //     ojo=1  ux=2
        let map_omi = inlined_node(
            "",
            None,
            vec![
                (b'o', inlined_node("jo", Some(1), vec![])),
                (b'u', inlined_node("x", Some(2), vec![])),
            ],
        );
        // map_omu:
        //     *
        //     |
        //     n
        //     |______
        //     |      \
        //     do=3   gal=4
        let map_omu = inlined_node(
            "n",
            None,
            vec![
                (b'd', inlined_node("o", Some(3), vec![])),
                (b'g', inlined_node("al", Some(4), vec![])),
            ],
        );
        // map_o:
        //     *
        //     |
        //     m
        //     |______________
        //     |              \
        //     i              un
        //     |______        |______
        //     |      \       |      \
        //     ojo=1  ux=2    do=3   gal=4
        let map_o = helper
            .stored_node(
                test_node("m", None, vec![(b'i', map_omi), (b'u', map_omu)]),
                4,
                "test.map2node.blake2.6f7dc1a2ad07d16eb4d3e586e2f7361c0990dcf4a29b0bb06fa5d04e69710a64"
            )
            .await?;
        // map:
        //     *
        //     |_______________________________________________
        //     |                                               \
        //     aba=12                                           om
        //     |_______________________________                 |______________
        //     |                               \                |              \
        //     ca                               la              i              un
        //     |____________________            |____           |______        |______
        //     |     \       \      \           |    \          |      \       |      \
        //     b=7   kkk=9  te=10  xi=11      ba=5  da=6       ojo=1   ux=2    do=3   gal=4
        //     |
        //     a=8
        let map = test_node("", None, vec![(b'a', map_a), (b'o', map_o)]);

        assert_eq!(from_entries_map, map);

        Ok(())
    }

    #[fbinit::test]
    async fn test_sharded_map_v2_from_entries_only_maps(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let blobstore = Memblob::default();
        let helper = MapHelper(ctx, blobstore);

        let map_ab = helper
            .from_entries_removed_prefix(&EXAMPLE_ENTRIES[0..8], 2)
            .await?;
        let map_omi = helper
            .from_entries_removed_prefix(&EXAMPLE_ENTRIES[8..10], 3)
            .await?;
        let map_omu = helper
            .from_entries_removed_prefix(&EXAMPLE_ENTRIES[10..12], 3)
            .await?;

        let map = helper
            .from_entries_and_partial_maps(&[
                ("ab", Either::Right(map_ab)),
                ("omi", Either::Right(map_omi)),
                ("omu", Either::Right(map_omu)),
            ])
            .await?;

        helper.check_example_map(map).await?;

        Ok(())
    }

    #[fbinit::test]
    async fn test_sharded_map_v2_from_entries_maps_and_values(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let blobstore = Memblob::default();
        let helper = MapHelper(ctx, blobstore);

        let map_ab = helper
            .from_entries_removed_prefix(&EXAMPLE_ENTRIES[0..8], 2)
            .await?;

        let map = helper
            .from_entries_and_partial_maps(
                &std::iter::once(("ab", Either::Right(map_ab)))
                    .chain(
                        EXAMPLE_ENTRIES[8..]
                            .iter()
                            .map(|(key, value)| (*key, Either::Left(*value))),
                    )
                    .collect::<Vec<_>>(),
            )
            .await?;

        helper.check_example_map(map).await?;

        Ok(())
    }

    #[fbinit::test]
    async fn test_sharded_map_v2_from_entries_conflict_detection(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let blobstore = Memblob::default();
        let helper = MapHelper(ctx, blobstore);

        let map_first_six = helper
            .from_entries_removed_prefix(&EXAMPLE_ENTRIES[0..6], 2)
            .await?;
        let map_last_six = helper.from_entries(&EXAMPLE_ENTRIES[6..12]).await?;

        assert!(
            helper
                .from_entries_and_partial_maps(&[
                    ("ab", Either::Right(map_first_six)),
                    ("", Either::Right(map_last_six)),
                ])
                .await
                .is_err()
        );

        let map_ab = helper
            .from_entries_removed_prefix(&EXAMPLE_ENTRIES[0..8], 2)
            .await?;
        let map_om = helper
            .from_entries_removed_prefix(&EXAMPLE_ENTRIES[8..12], 2)
            .await?;

        assert!(
            helper
                .from_entries_and_partial_maps(&[
                    ("ab", Either::Right(map_ab.clone())),
                    ("om", Either::Right(map_om.clone())),
                ])
                .await
                .is_ok()
        );

        assert!(
            helper
                .from_entries_and_partial_maps(&[
                    ("ab", Either::Right(map_ab.clone())),
                    ("om", Either::Right(map_om.clone())),
                    ("abababab", Either::Left(100)),
                ])
                .await
                .is_err()
        );

        assert!(
            helper
                .from_entries_and_partial_maps(&[
                    ("ab", Either::Right(map_ab.clone())),
                    ("om", Either::Right(map_om.clone())),
                    ("zz", Either::Left(100)),
                ])
                .await
                .is_ok()
        );

        assert!(
            helper
                .from_entries_and_partial_maps(&[
                    ("ab", Either::Right(map_ab.clone())),
                    ("om", Either::Right(map_om.clone())),
                    ("omo", Either::Left(100)),
                ])
                .await
                .is_err()
        );

        assert!(
            helper
                .from_entries_and_partial_maps(&[
                    ("o", Either::Left(100)),
                    ("ab", Either::Right(map_ab)),
                    ("om", Either::Right(map_om)),
                ])
                .await
                .is_ok()
        );

        Ok(())
    }

    #[fbinit::test]
    fn test_sharded_map_v2_quickcheck(fb: FacebookInit) {
        let ctx = CoreContext::test_mock(fb);
        let blobstore = Memblob::default();
        use tokio::runtime::Runtime;

        struct TestHelper(Runtime, CoreContext, Memblob);
        impl Testable for TestHelper {
            fn result(&self, gen: &mut Gen) -> TestResult {
                let res = self.0.block_on(async {
                    let values: BTreeMap<String, i32> = Arbitrary::arbitrary(gen);
                    let helper = MapHelper(self.1.clone(), self.2.clone());

                    let map = helper
                        .from_entries(
                            &values
                                .iter()
                                .map(|(k, v)| (k.as_str(), *v))
                                .collect::<Vec<_>>(),
                        )
                        .await?;

                    helper.check_sharded_map(map).await?;

                    anyhow::Ok(())
                });

                match res {
                    Ok(()) => TestResult::passed(),
                    Err(e) => TestResult::error(format!("{}", e)),
                }
            }
        }

        QuickCheck::new().quickcheck(TestHelper(Runtime::new().unwrap(), ctx, blobstore));
    }
}
