/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Quickcheck support for a few types that don't have support upstream,
//! and for a few other test types.

use bytes::Bytes;
use mercurial_types::Delta;
use mercurial_types::HgNodeHash;
use quickcheck::Arbitrary;
use quickcheck::Gen;
use quickcheck::empty_shrinker;

use crate::changegroup;

#[derive(Clone, Debug)]
pub struct QCBytes(Bytes);

impl From<QCBytes> for Bytes {
    fn from(qcbytes: QCBytes) -> Bytes {
        qcbytes.0
    }
}

impl Arbitrary for QCBytes {
    fn arbitrary(g: &mut Gen) -> Self {
        // Just use the Vec<u8> quickcheck underneath.
        let v: Vec<u8> = Vec::arbitrary(g);
        QCBytes(v.into())
    }

    fn shrink(&self) -> Box<dyn Iterator<Item = Self>> {
        Box::new(self.0.to_vec().shrink().map(|v| QCBytes(v.into())))
    }
}

impl Arbitrary for changegroup::Part {
    fn arbitrary(_g: &mut Gen) -> Self {
        unimplemented!()
    }

    fn shrink(&self) -> Box<dyn Iterator<Item = Self>> {
        use crate::changegroup::Part::CgChunk;

        match self {
            CgChunk(section, delta_chunk) => {
                // Keep the section the same, but shrink the delta chunk.
                let section = section.clone();
                Box::new(
                    delta_chunk
                        .shrink()
                        .map(move |chunk| CgChunk(section.clone(), chunk)),
                )
            }
            _ => empty_shrinker(),
        }
    }
}

impl Arbitrary for changegroup::CgDeltaChunk {
    fn arbitrary(g: &mut Gen) -> Self {
        // TODO: should these be more structured? e.g. base = p1 some of the time
        changegroup::CgDeltaChunk {
            node: HgNodeHash::arbitrary(g),
            p1: HgNodeHash::arbitrary(g),
            p2: HgNodeHash::arbitrary(g),
            base: HgNodeHash::arbitrary(g),
            linknode: HgNodeHash::arbitrary(g),
            delta: Delta::arbitrary(g),
            flags: None,
        }
    }

    fn shrink(&self) -> Box<dyn Iterator<Item = Self>> {
        // Don't bother trying to shrink node hashes -- the meat is in the delta.
        let clone = self.clone();
        Box::new(
            self.delta
                .shrink()
                .map(move |delta| changegroup::CgDeltaChunk {
                    node: clone.node.clone(),
                    p1: clone.p1.clone(),
                    p2: clone.p2.clone(),
                    base: clone.base.clone(),
                    linknode: clone.linknode.clone(),
                    delta,
                    flags: clone.flags.clone(),
                }),
        )
    }
}
