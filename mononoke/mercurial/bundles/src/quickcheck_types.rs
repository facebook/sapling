/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

//! Quickcheck support for a few types that don't have support upstream,
//! and for a few other test types.

use std::convert::From;
use std::iter;
#[cfg(test)]
use std::vec::IntoIter;

#[cfg(test)]
use anyhow::{Error, Result};
use bytes::Bytes;
#[cfg(test)]
use futures::stream;
use quickcheck::{empty_shrinker, Arbitrary, Gen};
use rand::Rng;

use mercurial_types::{Delta, HgNodeHash, MPath, RevFlags};

use crate::changegroup;

#[derive(Clone, Debug)]
pub struct QCBytes(Bytes);

impl From<QCBytes> for Bytes {
    fn from(qcbytes: QCBytes) -> Bytes {
        qcbytes.0
    }
}

impl Arbitrary for QCBytes {
    fn arbitrary<G: Gen>(g: &mut G) -> Self {
        // Just use the Vec<u8> quickcheck underneath.
        let v: Vec<u8> = Vec::arbitrary(g);
        QCBytes(v.into())
    }

    fn shrink(&self) -> Box<dyn Iterator<Item = Self>> {
        Box::new(self.0.to_vec().shrink().map(|v| QCBytes(v.into())))
    }
}

#[derive(Clone, Debug)]
pub struct CgPartSequence {
    // Storing the ends in here bypasses a number of lifetime issues.
    changesets: Vec<changegroup::Part>,
    changesets_end: changegroup::Part,
    manifests: Vec<changegroup::Part>,
    manifests_end: changegroup::Part,
    treemanifest_end: Option<changegroup::Part>,
    filelogs: Vec<(Vec<changegroup::Part>, changegroup::Part)>,
    end: changegroup::Part,

    version: changegroup::unpacker::CgVersion,
}

impl CgPartSequence {
    /// Combine all the changesets, manifests and filelogs into a single iterator.
    pub fn as_iter<'a>(&'a self) -> Box<dyn Iterator<Item = &'a changegroup::Part> + 'a> {
        // Trying to describe the type here is madness. Just box it.
        Box::new(
            self.changesets
                .iter()
                .chain(iter::once(&self.changesets_end))
                .chain(self.manifests.iter())
                .chain(iter::once(&self.manifests_end))
                .chain(self.treemanifest_end.iter())
                .chain(
                    self.filelogs
                        .iter()
                        .filter(|&&(ref parts, _)| {
                            // If there are no filelog parts, it isn't valid to return a
                            // SectionEnd since that won't be referring to anything. So
                            // just skip the whole filelog.
                            !parts.is_empty()
                        })
                        .flat_map(|&(ref parts, ref end)| parts.iter().chain(iter::once(end))),
                )
                .chain(iter::once(&self.end)),
        )
    }

    /// Combine all the changesets, manifests and filelogs into a single stream.
    ///
    /// This returns a clone of everything because streams can't really return
    /// references at the moment.
    #[cfg(test)]
    pub fn to_stream(&self) -> stream::IterOk<IntoIter<Result<changegroup::Part>>, Error> {
        let part_results: Vec<_> = self.as_iter().cloned().map(|x| Ok(x)).collect();
        stream::iter_ok(part_results.into_iter())
    }

    #[cfg(test)]
    pub fn version(&self) -> &changegroup::unpacker::CgVersion {
        &self.version
    }
}

impl PartialEq<[changegroup::Part]> for CgPartSequence {
    fn eq(&self, other: &[changegroup::Part]) -> bool {
        self.as_iter().eq(other.iter())
    }
}

impl Arbitrary for CgPartSequence {
    fn arbitrary<G: Gen>(g: &mut G) -> Self {
        use crate::changegroup::unpacker::*;
        use crate::changegroup::*;

        let version_ind = g.gen_ratio(1, 2);
        let version = match version_ind {
            true => CgVersion::Cg2Version,
            false => CgVersion::Cg3Version,
        };
        let gen_parts = match version {
            CgVersion::Cg2Version => gen_parts_v2,
            CgVersion::Cg3Version => gen_parts_v3,
        };
        let gen_sequence = match version {
            CgVersion::Cg2Version => gen_sequence_v2,
            CgVersion::Cg3Version => gen_sequence_v3,
        };

        // Generate a valid part sequence (changegroup, then manifest, then filelogs).
        let size = g.size();

        let changesets = gen_parts(Section::Changeset, g);
        let manifests = gen_parts(Section::Manifest, g);

        let nfilelogs = g.gen_range(0, size);
        let mut filelogs = Vec::with_capacity(nfilelogs);

        for _ in 0..nfilelogs {
            // Changegroups can't support empty paths, so skip over those.
            let path = MPath::arbitrary(g);
            let section_end = Part::SectionEnd(Section::Filelog(path.clone()));
            filelogs.push((gen_parts(Section::Filelog(path), g), section_end));
        }

        gen_sequence(changesets, manifests, filelogs)
    }

    fn shrink(&self) -> Box<dyn Iterator<Item = Self>> {
        use crate::changegroup::unpacker::*;

        let gen_sequence = match self.version {
            CgVersion::Cg2Version => gen_sequence_v2,
            CgVersion::Cg3Version => gen_sequence_v3,
        };

        // All the parts can be shrinked independently as long as the section
        // remains the same (ensured in the impl of Arbitrary for
        // changegroup::Part).
        Box::new(
            (
                self.changesets.clone(),
                self.manifests.clone(),
                self.filelogs.clone(),
            )
                .shrink()
                .map(move |(c, m, f)| gen_sequence(c, m, f)),
        )
    }
}

fn gen_parts_v2<G: Gen>(section: changegroup::Section, g: &mut G) -> Vec<changegroup::Part> {
    let size = g.size();
    (0..g.gen_range(0, size))
        .map(|_| {
            changegroup::Part::CgChunk(section.clone(), changegroup::CgDeltaChunk::arbitrary(g))
        })
        .collect()
}

fn gen_parts_v3<G: Gen>(section: changegroup::Section, g: &mut G) -> Vec<changegroup::Part> {
    let size = g.size();
    (0..g.gen_range(0, size))
        .map(|_| {
            changegroup::Part::CgChunk(
                section.clone(),
                changegroup::CgDeltaChunk::arbitrary_with_flags(g),
            )
        })
        .collect()
}

fn gen_sequence_v2(
    changesets: Vec<changegroup::Part>,
    manifests: Vec<changegroup::Part>,
    filelogs: Vec<(Vec<changegroup::Part>, changegroup::Part)>,
) -> CgPartSequence {
    use crate::changegroup::*;
    CgPartSequence {
        changesets,
        changesets_end: Part::SectionEnd(Section::Changeset),
        manifests,
        manifests_end: Part::SectionEnd(Section::Manifest),
        treemanifest_end: None,
        filelogs,
        end: Part::End,

        version: changegroup::unpacker::CgVersion::Cg2Version,
    }
}
fn gen_sequence_v3(
    changesets: Vec<changegroup::Part>,
    manifests: Vec<changegroup::Part>,
    filelogs: Vec<(Vec<changegroup::Part>, changegroup::Part)>,
) -> CgPartSequence {
    use crate::changegroup::*;
    CgPartSequence {
        changesets,
        changesets_end: Part::SectionEnd(Section::Changeset),
        manifests,
        manifests_end: Part::SectionEnd(Section::Manifest),
        treemanifest_end: Some(Part::SectionEnd(Section::Treemanifest)),
        filelogs,
        end: Part::End,

        version: changegroup::unpacker::CgVersion::Cg3Version,
    }
}

impl Arbitrary for changegroup::Part {
    fn arbitrary<G: Gen>(_g: &mut G) -> Self {
        unimplemented!()
    }

    fn shrink(&self) -> Box<dyn Iterator<Item = Self>> {
        use crate::changegroup::Part::CgChunk;

        match self {
            &CgChunk(ref section, ref delta_chunk) => {
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
    fn arbitrary<G: Gen>(g: &mut G) -> Self {
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

impl changegroup::CgDeltaChunk {
    fn arbitrary_with_flags<G: Gen>(g: &mut G) -> Self {
        let flags = u8::arbitrary(g) % 3;
        let flags = if flags == 0 {
            RevFlags::REVIDX_DEFAULT_FLAGS
        } else if flags == 1 {
            RevFlags::REVIDX_EXTSTORED
        } else {
            RevFlags::REVIDX_ELLIPSIS
        };
        changegroup::CgDeltaChunk {
            node: HgNodeHash::arbitrary(g),
            p1: HgNodeHash::arbitrary(g),
            p2: HgNodeHash::arbitrary(g),
            base: HgNodeHash::arbitrary(g),
            linknode: HgNodeHash::arbitrary(g),
            delta: Delta::arbitrary(g),
            flags: Some(flags),
        }
    }
}
