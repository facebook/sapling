// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use hash::{self, Context};
use nodehash::DNodeHash;

use blob::HgBlob;

#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Debug, Hash)]
#[derive(Serialize, Deserialize, HeapSizeOf)]
pub enum DParents {
    None,
    One(DNodeHash),
    Two(DNodeHash, DNodeHash),
}

impl DParents {
    pub fn new(p1: Option<&DNodeHash>, p2: Option<&DNodeHash>) -> Self {
        match (p1, p2) {
            (None, None) => DParents::None,
            (Some(p1), None) => DParents::One(*p1),
            (None, Some(p2)) => DParents::One(*p2),
            (Some(p1), Some(p2)) => DParents::Two(*p1, *p2),
        }
    }

    pub fn get_nodes(&self) -> (Option<&DNodeHash>, Option<&DNodeHash>) {
        match self {
            &DParents::None => (None, None),
            &DParents::One(ref p1) => (Some(p1), None),
            &DParents::Two(ref p1, ref p2) => (Some(p1), Some(p2)),
        }
    }
}

impl<'a> IntoIterator for &'a DParents {
    type IntoIter = ParentIter;
    type Item = DNodeHash;
    fn into_iter(self) -> ParentIter {
        ParentIter(*self)
    }
}

#[derive(Debug)]
pub struct ParentIter(DParents);

impl Iterator for ParentIter {
    type Item = DNodeHash;
    fn next(&mut self) -> Option<Self::Item> {
        let (ret, new) = match self.0 {
            DParents::None => (None, DParents::None),
            DParents::One(p1) => (Some(p1), DParents::None),
            DParents::Two(p1, p2) => (Some(p1), DParents::One(p2)),
        };
        self.0 = new;
        ret
    }
}

/// A Mercurial node backed by some data. This can represent a changeset, a manifest or a file
/// blob.
#[derive(Clone, Eq, PartialEq, Ord, PartialOrd, Debug, Hash)]
#[derive(Serialize, Deserialize)]
pub struct DBlobNode {
    blob: HgBlob,
    parents: DParents,
    maybe_copied: bool,
}

impl DBlobNode {
    /// Construct a node with the given content and parents.
    /// NOTE: Mercurial encodes the fact that a file has been copied from some other path
    /// in metadata. The possible presence of metadata is signaled by marking p1 as None.
    /// * If both p1 and p2 are not None, there's no copy involved (no metadata).
    /// * If a merge has two parents and there's a copy involved, p1 is null, p2 is non-null and
    ///   is the parent rev that isn't copied, and the metadata contains a copyrev that's the
    ///   parent that's copied.
    /// * If both p1 and p2 are None, it shouldn't really be possible to have copy info. But
    ///   the Mercurial Python client tries to parse metadata anyway, so match that behavior.
    pub fn new<B>(blob: B, p1: Option<&DNodeHash>, p2: Option<&DNodeHash>) -> DBlobNode
    where
        B: Into<HgBlob>,
    {
        let blob = blob.into();
        DBlobNode {
            blob: blob,
            parents: DParents::new(p1, p2),
            maybe_copied: p1.is_none(),
        }
    }

    pub fn size(&self) -> Option<usize> {
        self.blob.size()
    }

    pub fn as_blob(&self) -> &HgBlob {
        &self.blob
    }

    pub fn parents(&self) -> &DParents {
        &self.parents
    }

    pub fn maybe_copied(&self) -> bool {
        self.maybe_copied
    }

    // Annoyingly, filenode is defined as sha1(p1 || p2 || content), not
    // sha1(p1 || p2 || sha1(content)), so we can't compute a filenode for
    // a blob we don't have
    pub fn nodeid(&self) -> Option<DNodeHash> {
        let null = hash::NULL;

        let (h1, h2) = match &self.parents {
            &DParents::None => (&null, &null),
            &DParents::One(ref p1) => (&null, &p1.0),
            &DParents::Two(ref p1, ref p2) if p1 > p2 => (&p2.0, &p1.0),
            &DParents::Two(ref p1, ref p2) => (&p1.0, &p2.0),
        };

        self.as_blob().as_slice().map(|data| {
            let mut ctxt = Context::new();

            ctxt.update(h1);
            ctxt.update(h2);
            ctxt.update(data);

            DNodeHash(ctxt.finish())
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
        let n = DBlobNode::new(blob, None, None);
        assert_eq!(n.parents, DParents::None);
    }

    #[test]
    fn test_node_one() {
        let blob = HgBlob::from(Bytes::from(&[0; 10][..]));
        let p = &DBlobNode::new(blob.clone(), None, None);
        assert!(p.maybe_copied);
        {
            let pid: Option<DNodeHash> = p.nodeid();
            let n = DBlobNode::new(blob.clone(), pid.as_ref(), None);
            assert_eq!(n.parents, DParents::One(pid.unwrap()));
            assert!(!n.maybe_copied);
        }
        {
            let pid: Option<DNodeHash> = p.nodeid();
            let n = DBlobNode::new(blob.clone(), None, pid.as_ref());
            assert_eq!(n.parents, DParents::One(pid.unwrap()));
            assert!(n.maybe_copied);
        }
        {
            let pid: Option<DNodeHash> = p.nodeid();
            let n = DBlobNode::new(blob.clone(), pid.as_ref(), pid.as_ref());
            assert_eq!(n.parents, DParents::Two(pid.unwrap(), pid.unwrap()));
            assert!(!n.maybe_copied);
        }
    }

    #[test]
    fn test_node_two() {
        use std::mem;
        let mut p1 = DBlobNode::new(HgBlob::from(Bytes::from(&b"foo1"[..])), None, None);
        let mut p2 = DBlobNode::new(HgBlob::from(Bytes::from(&b"foo2"[..])), None, None);
        assert!(p1.maybe_copied);
        assert!(p2.maybe_copied);

        if p1 > p2 {
            mem::swap(&mut p1, &mut p2);
        }

        let pid1: Option<DNodeHash> = (&p1).nodeid();
        let pid2: Option<DNodeHash> = (&p2).nodeid();

        let node1 = {
            let n = DBlobNode::new(
                HgBlob::from(Bytes::from(&b"bar"[..])),
                pid1.as_ref(),
                pid2.as_ref(),
            );
            assert_eq!(n.parents, DParents::Two(pid1.unwrap(), pid2.unwrap()));
            assert!(!n.maybe_copied);
            n.nodeid().expect("no nodeid 1")
        };
        let node2 = {
            let n = DBlobNode::new(
                HgBlob::from(Bytes::from(&b"bar"[..])),
                pid2.as_ref(),
                pid1.as_ref(),
            );
            assert_eq!(n.parents, DParents::Two(pid2.unwrap(), pid1.unwrap()));
            assert!(!n.maybe_copied);
            n.nodeid().expect("no nodeid 2")
        };
        assert_eq!(node1, node2);
    }
}
