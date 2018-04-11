// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use hash::{self, Context};
use nodehash::NodeHash;

use blob::HgBlob;

#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Debug, Hash)]
#[derive(Serialize, Deserialize, HeapSizeOf)]
pub enum Parents {
    None,
    One(NodeHash),
    Two(NodeHash, NodeHash),
}

impl Parents {
    pub fn new(p1: Option<&NodeHash>, p2: Option<&NodeHash>) -> Self {
        match (p1, p2) {
            (None, None) => Parents::None,
            (Some(p1), None) => Parents::One(*p1),
            (None, Some(p2)) => Parents::One(*p2),
            (Some(p1), Some(p2)) if p1 == p2 => Parents::One(*p1),
            (Some(p1), Some(p2)) => Parents::Two(*p1, *p2),
        }
    }

    pub fn get_nodes(&self) -> (Option<&NodeHash>, Option<&NodeHash>) {
        match self {
            &Parents::None => (None, None),
            &Parents::One(ref p1) => (Some(p1), None),
            &Parents::Two(ref p1, ref p2) => (Some(p1), Some(p2)),
        }
    }
}

impl<'a> IntoIterator for &'a Parents {
    type IntoIter = ParentIter;
    type Item = NodeHash;
    fn into_iter(self) -> ParentIter {
        ParentIter(*self)
    }
}

#[derive(Debug)]
pub struct ParentIter(Parents);

impl Iterator for ParentIter {
    type Item = NodeHash;
    fn next(&mut self) -> Option<Self::Item> {
        let (ret, new) = match self.0 {
            Parents::None => (None, Parents::None),
            Parents::One(p1) => (Some(p1), Parents::None),
            Parents::Two(p1, p2) => (Some(p1), Parents::One(p2)),
        };
        self.0 = new;
        ret
    }
}

/// A Mercurial node backed by some data. This can represent a changeset, a manifest or a file
/// blob.
#[derive(Clone, Eq, PartialEq, Ord, PartialOrd, Debug, Hash)]
#[derive(Serialize, Deserialize)]
pub struct BlobNode {
    blob: HgBlob,
    parents: Parents,
    maybe_copied: bool,
}

impl BlobNode {
    /// Construct a node with the given content and parents.
    /// NOTE: Mercurial encodes the fact that a file has been copied from some other path
    /// by encoding the fact by using p2 instead of p1 to refer to the parent version.
    /// Two parent nodes are always considered to have been potentially copied.
    pub fn new<B>(blob: B, p1: Option<&NodeHash>, p2: Option<&NodeHash>) -> BlobNode
    where
        B: Into<HgBlob>,
    {
        let blob = blob.into();
        BlobNode {
            blob: blob,
            parents: Parents::new(p1, p2),
            maybe_copied: p1.is_none(),
        }
    }

    pub fn size(&self) -> Option<usize> {
        self.blob.size()
    }

    pub fn as_blob(&self) -> &HgBlob {
        &self.blob
    }

    pub fn parents(&self) -> &Parents {
        &self.parents
    }

    pub fn maybe_copied(&self) -> bool {
        self.maybe_copied
    }

    // Annoyingly, filenode is defined as sha1(p1 || p2 || content), not
    // sha1(p1 || p2 || sha1(content)), so we can't compute a filenode for
    // a blob we don't have
    pub fn nodeid(&self) -> Option<NodeHash> {
        let null = hash::NULL;

        let (h1, h2) = match &self.parents {
            &Parents::None => (&null, &null),
            &Parents::One(ref p1) => (&null, p1.sha1()),
            &Parents::Two(ref p1, ref p2) if p1 > p2 => (p2.sha1(), p1.sha1()),
            &Parents::Two(ref p1, ref p2) => (p1.sha1(), p2.sha1()),
        };

        self.as_blob().as_slice().map(|data| {
            let mut ctxt = Context::new();

            ctxt.update(h1);
            ctxt.update(h2);
            ctxt.update(data);

            NodeHash::new(ctxt.finish())
        })
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use blob::HgBlob;
    use bytes::Bytes;

    #[test]
    fn test_node_none() {
        let blob = HgBlob::from(Bytes::from(&[0; 10][..]));
        let n = BlobNode::new(blob, None, None);
        assert_eq!(n.parents, Parents::None);
    }

    #[test]
    fn test_node_one() {
        let blob = HgBlob::from(Bytes::from(&[0; 10][..]));
        let p = &BlobNode::new(blob.clone(), None, None);
        assert!(p.maybe_copied);
        {
            let pid: Option<NodeHash> = p.nodeid();
            let n = BlobNode::new(blob.clone(), pid.as_ref(), None);
            assert_eq!(n.parents, Parents::One(pid.unwrap()));
            assert!(!n.maybe_copied);
        }
        {
            let pid: Option<NodeHash> = p.nodeid();
            let n = BlobNode::new(blob.clone(), None, pid.as_ref());
            assert_eq!(n.parents, Parents::One(pid.unwrap()));
            assert!(n.maybe_copied);
        }
        {
            let pid: Option<NodeHash> = p.nodeid();
            let n = BlobNode::new(blob.clone(), pid.as_ref(), pid.as_ref());
            assert_eq!(n.parents, Parents::One(pid.unwrap()));
            assert!(!n.maybe_copied);
        }
    }

    #[test]
    fn test_node_two() {
        use std::mem;
        let mut p1 = BlobNode::new(HgBlob::from(Bytes::from(&b"foo1"[..])), None, None);
        let mut p2 = BlobNode::new(HgBlob::from(Bytes::from(&b"foo2"[..])), None, None);
        assert!(p1.maybe_copied);
        assert!(p2.maybe_copied);

        if p1 > p2 {
            mem::swap(&mut p1, &mut p2);
        }

        let pid1: Option<NodeHash> = (&p1).nodeid();
        let pid2: Option<NodeHash> = (&p2).nodeid();

        let node1 = {
            let n = BlobNode::new(
                HgBlob::from(Bytes::from(&b"bar"[..])),
                pid1.as_ref(),
                pid2.as_ref(),
            );
            assert_eq!(n.parents, Parents::Two(pid1.unwrap(), pid2.unwrap()));
            assert!(!n.maybe_copied);
            n.nodeid().expect("no nodeid 1")
        };
        let node2 = {
            let n = BlobNode::new(
                HgBlob::from(Bytes::from(&b"bar"[..])),
                pid2.as_ref(),
                pid1.as_ref(),
            );
            assert_eq!(n.parents, Parents::Two(pid2.unwrap(), pid1.unwrap()));
            assert!(!n.maybe_copied);
            n.nodeid().expect("no nodeid 2")
        };
        assert_eq!(node1, node2);
    }
}
