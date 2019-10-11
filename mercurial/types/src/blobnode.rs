/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use bytes::Bytes;
use futures::{Future, Stream};
use heapsize_derive::HeapSizeOf;
use quickcheck::{Arbitrary, Gen};
use serde_derive::{Deserialize, Serialize};

use crate::blob::HgBlob;
use crate::hash::{self, Context};
use crate::nodehash::HgNodeHash;

/// Equivalent type from Mercurial's Rust code representing parents.
use types::Parents as HgTypesParents;

#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Debug, Hash)]
#[derive(Serialize, Deserialize, HeapSizeOf)]
pub enum HgParents {
    None,
    One(HgNodeHash),
    Two(HgNodeHash, HgNodeHash),
}

impl HgParents {
    pub fn new(p1: Option<HgNodeHash>, p2: Option<HgNodeHash>) -> Self {
        match (p1, p2) {
            (None, None) => HgParents::None,
            (Some(p1), None) => HgParents::One(p1),
            (None, Some(p2)) => HgParents::One(p2),
            (Some(p1), Some(p2)) => HgParents::Two(p1, p2),
        }
    }

    pub fn get_nodes(&self) -> (Option<HgNodeHash>, Option<HgNodeHash>) {
        match self {
            &HgParents::None => (None, None),
            &HgParents::One(p1) => (Some(p1), None),
            &HgParents::Two(p1, p2) => (Some(p1), Some(p2)),
        }
    }
}

impl Arbitrary for HgParents {
    fn arbitrary<G: Gen>(g: &mut G) -> Self {
        // We make single-parent a litle more common because it's a) little simpler b) a little
        // more common anyway.
        if bool::arbitrary(g) {
            Self::new(Some(HgNodeHash::arbitrary(g)), None)
        } else if bool::arbitrary(g) {
            Self::new(
                Some(HgNodeHash::arbitrary(g)),
                Some(HgNodeHash::arbitrary(g)),
            )
        } else {
            Self::new(None, None)
        }
    }
}

/// [HgTypesParents] (an alias for [types::Parents] from Mercurial's `types` crate) is
/// the Mercurial client's Rust representation of parents. It is an enum almost identical
/// to the [HgParents] enum in this crate. As such, this conversion is only useful when
/// interacting with the Mercurial client's Rust code.
impl From<HgParents> for HgTypesParents {
    fn from(parents: HgParents) -> Self {
        match parents {
            HgParents::None => HgTypesParents::None,
            HgParents::One(p1) => HgTypesParents::One(p1.into()),
            HgParents::Two(p1, p2) => HgTypesParents::Two(p1.into(), p2.into()),
        }
    }
}

impl<'a> IntoIterator for &'a HgParents {
    type IntoIter = ParentIter;
    type Item = HgNodeHash;
    fn into_iter(self) -> ParentIter {
        ParentIter(*self)
    }
}

#[derive(Debug)]
pub struct ParentIter(HgParents);

impl Iterator for ParentIter {
    type Item = HgNodeHash;
    fn next(&mut self) -> Option<Self::Item> {
        let (ret, new) = match self.0 {
            HgParents::None => (None, HgParents::None),
            HgParents::One(p1) => (Some(p1), HgParents::None),
            HgParents::Two(p1, p2) => (Some(p1), HgParents::One(p2)),
        };
        self.0 = new;
        ret
    }
}

/// A Mercurial node backed by some data. This can represent a changeset, a manifest or a file
/// blob.
#[derive(Clone, Eq, PartialEq, Ord, PartialOrd, Debug, Hash)]
#[derive(Serialize, Deserialize)]
pub struct HgBlobNode {
    blob: HgBlob,
    parents: HgParents,
}

impl HgBlobNode {
    /// Construct a node with the given content and parents.
    pub fn new<B>(blob: B, p1: Option<HgNodeHash>, p2: Option<HgNodeHash>) -> HgBlobNode
    where
        B: Into<HgBlob>,
    {
        let blob = blob.into();
        HgBlobNode {
            blob,
            parents: HgParents::new(p1, p2),
        }
    }

    pub fn size(&self) -> usize {
        self.blob.size()
    }

    pub fn as_blob(&self) -> &HgBlob {
        &self.blob
    }

    pub fn parents(&self) -> &HgParents {
        &self.parents
    }

    // Annoyingly, filenode is defined as sha1(p1 || p2 || content), not
    // sha1(p1 || p2 || sha1(content)), so we can't compute a filenode for
    // a blob we don't have
    pub fn nodeid(&self) -> HgNodeHash {
        calculate_hg_node_id(self.as_blob().as_slice(), &self.parents)
    }
}

fn hg_node_id_hash_context(parents: &HgParents) -> Context {
    let null = hash::NULL;

    let (h1, h2) = match &parents {
        &HgParents::None => (&null, &null),
        &HgParents::One(ref p1) => (&null, &p1.0),
        &HgParents::Two(ref p1, ref p2) if p1 > p2 => (&p2.0, &p1.0),
        &HgParents::Two(ref p1, ref p2) => (&p1.0, &p2.0),
    };

    let mut ctxt = Context::new();

    ctxt.update(h1);
    ctxt.update(h2);

    ctxt
}

/// Compute a Hg Node ID from parents and in-place data.
pub fn calculate_hg_node_id(data: &[u8], parents: &HgParents) -> HgNodeHash {
    let mut ctxt = hg_node_id_hash_context(parents);
    ctxt.update(data);
    HgNodeHash(ctxt.finish())
}

/// Compute a Hg Node ID from parents and a stream of data.
pub fn calculate_hg_node_id_stream<S, E>(
    stream: S,
    parents: &HgParents,
) -> impl Future<Item = HgNodeHash, Error = E>
where
    S: Stream<Item = Bytes, Error = E>,
{
    let ctxt = hg_node_id_hash_context(parents);
    stream
        .fold(ctxt, |mut ctxt, bytes| {
            ctxt.update(bytes);
            Ok(ctxt)
        })
        .map(|ctxt| ctxt.finish())
        .map(HgNodeHash)
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::blob::HgBlob;
    use futures::stream;
    use quickcheck::quickcheck;
    use tokio::runtime::Runtime;

    #[test]
    fn test_node_none() {
        let blob = HgBlob::from(Bytes::from(&[0; 10][..]));
        let n = HgBlobNode::new(blob, None, None);
        assert_eq!(n.parents, HgParents::None);
    }

    #[test]
    fn test_node_one() {
        let blob = HgBlob::from(Bytes::from(&[0; 10][..]));
        let p = &HgBlobNode::new(blob.clone(), None, None);
        {
            let pid: Option<HgNodeHash> = Some(p.nodeid());
            let n = HgBlobNode::new(blob.clone(), pid, None);
            assert_eq!(n.parents, HgParents::One(pid.unwrap()));
        }
        {
            let pid: Option<HgNodeHash> = Some(p.nodeid());
            let n = HgBlobNode::new(blob.clone(), None, pid);
            assert_eq!(n.parents, HgParents::One(pid.unwrap()));
        }
        {
            let pid: Option<HgNodeHash> = Some(p.nodeid());
            let n = HgBlobNode::new(blob.clone(), pid, pid);
            assert_eq!(n.parents, HgParents::Two(pid.unwrap(), pid.unwrap()));
        }
    }

    #[test]
    fn test_node_two() {
        use std::mem;
        let mut p1 = HgBlobNode::new(HgBlob::from(Bytes::from(&b"foo1"[..])), None, None);
        let mut p2 = HgBlobNode::new(HgBlob::from(Bytes::from(&b"foo2"[..])), None, None);

        if p1 > p2 {
            mem::swap(&mut p1, &mut p2);
        }

        let pid1: Option<HgNodeHash> = Some((&p1).nodeid());
        let pid2: Option<HgNodeHash> = Some((&p2).nodeid());

        let node1 = {
            let n = HgBlobNode::new(HgBlob::from(Bytes::from(&b"bar"[..])), pid1, pid2);
            assert_eq!(n.parents, HgParents::Two(pid1.unwrap(), pid2.unwrap()));
            n.nodeid()
        };
        let node2 = {
            let n = HgBlobNode::new(HgBlob::from(Bytes::from(&b"bar"[..])), pid2, pid1);
            assert_eq!(n.parents, HgParents::Two(pid2.unwrap(), pid1.unwrap()));
            n.nodeid()
        };
        assert_eq!(node1, node2);
    }

    quickcheck! {
        // Verify that the two Node Id computation implementations (in place and streaming) are
        // consistent.
        fn test_node_consistency(input: Vec<Vec<u8>>, hg_parents: HgParents) -> bool {
            let mut rt = Runtime::new().unwrap();
            let input: Vec<Bytes> = input.into_iter().map(Bytes::from).collect();

            let stream = stream::iter_ok::<_, ()>(input.clone());

            let bytes = input.iter().fold(Bytes::new(), |mut bytes, chunk| {
                bytes.extend_from_slice(&chunk);
                bytes
            });

            let out_inplace = calculate_hg_node_id(bytes.as_ref(), &hg_parents);
            let out_stream = rt.block_on(calculate_hg_node_id_stream(stream, &hg_parents)).unwrap();

            out_inplace == out_stream
        }
    }
}
