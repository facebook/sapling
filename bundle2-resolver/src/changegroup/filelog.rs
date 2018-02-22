// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::collections::HashMap;
use std::mem;

use bytes::Bytes;
use futures::Stream;
use futures_ext::{BoxFuture, BoxStream, StreamExt};
use heapsize::HeapSizeOf;
use quickcheck::{Arbitrary, Gen};

use blobrepo::{BlobEntry, BlobRepo};
use mercurial_bundles::changegroup::CgDeltaChunk;
use mercurial_types::{delta, manifest, Blob, Delta, MPath, NodeHash, RepoPath};
use mercurial_types::nodehash::NULL_HASH;

use errors::*;
use stats::*;
use upload_blobs::UploadableBlob;

#[derive(Debug, Eq, PartialEq)]
pub struct FilelogDeltaed {
    pub path: MPath,
    pub chunk: CgDeltaChunk,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Filelog {
    pub path: RepoPath,
    pub node: NodeHash,
    pub p1: Option<NodeHash>,
    pub p2: Option<NodeHash>,
    pub linknode: NodeHash,
    pub blob: Blob<Bytes>,
}

impl UploadableBlob for Filelog {
    type Value = BoxFuture<(BlobEntry, RepoPath), Error>;

    fn upload(self, repo: &BlobRepo) -> Result<(NodeHash, Self::Value)> {
        repo.upload_entry(self.blob, manifest::Type::File, self.p1, self.p2, self.path)
    }
}

pub fn convert_to_revlog_filelog<S>(deltaed: S) -> BoxStream<Filelog, Error>
where
    S: Stream<Item = FilelogDeltaed, Error = Error> + Send + 'static,
{
    let mut delta_cache = DeltaCache::new();
    deltaed
        .and_then(move |FilelogDeltaed { path, chunk }| {
            let blob =
                delta_cache.decode(chunk.node.clone(), chunk.base.into_option(), chunk.delta)?;

            Ok(Filelog {
                path: RepoPath::file(path)?,
                node: chunk.node,
                p1: chunk.p1.into_option(),
                p2: chunk.p2.into_option(),
                linknode: chunk.linknode,
                blob,
            })
        })
        .boxify()
}

struct DeltaCache {
    bytes_cache: HashMap<NodeHash, Bytes>,
    bytes_heap_size: i64,
}

impl DeltaCache {
    fn new() -> Self {
        Self {
            bytes_cache: HashMap::new(),
            bytes_heap_size: 0,
        }
    }

    fn decode(
        &mut self,
        node: NodeHash,
        base: Option<NodeHash>,
        delta: Delta,
    ) -> Result<Blob<Bytes>> {
        let vec = match base {
            None => delta::apply(b"", &delta),
            Some(base) => match self.bytes_cache.get(&base) {
                Some(bytes) => delta::apply(bytes, &delta),
                None => bail_msg!(
                    "Either the Filelogs are not topologically sorted \
                     or the base ({:?}) is not present in this bundle2",
                    base
                ),
            },
        };

        let bytes = Bytes::from(vec);
        ensure_msg!(
            self.bytes_cache.insert(node, bytes.clone()).is_none(),
            "Two Filelogs with identical node were provided"
        );

        let dsize = delta.heap_size_of_children() as i64;
        STATS::deltacache_dsize.add_value(dsize);
        STATS::deltacache_dsize_large.add_value(dsize);

        let fsize = (mem::size_of::<u8>() * bytes.as_ref().len()) as i64;
        STATS::deltacache_fsize.add_value(fsize);
        STATS::deltacache_fsize_large.add_value(fsize);
        self.bytes_heap_size += fsize;

        Ok(Blob::from(bytes))
    }
}

impl ::std::ops::Drop for DeltaCache {
    fn drop(&mut self) {
        let approx_bucket_size =
            self.bytes_cache.capacity() * (mem::size_of::<Bytes>() + mem::size_of::<NodeHash>());

        let approx_keys_size = self.bytes_cache.len() * mem::size_of_val(&NULL_HASH);

        let cache_size = approx_bucket_size as i64 + approx_keys_size as i64 + self.bytes_heap_size;

        STATS::deltacache_size.add_value(cache_size);
        STATS::deltacache_size_large.add_value(cache_size);
    }
}

impl Arbitrary for Filelog {
    fn arbitrary<G: Gen>(g: &mut G) -> Self {
        Filelog {
            path: RepoPath::file(MPath::arbitrary(g))
                .unwrap_or(RepoPath::file(MPath::new(b"test").unwrap()).unwrap()),
            node: NodeHash::arbitrary(g),
            p1: NodeHash::arbitrary(g).into_option(),
            p2: NodeHash::arbitrary(g).into_option(),
            linknode: NodeHash::arbitrary(g),
            blob: Blob::from(Bytes::from(Vec::<u8>::arbitrary(g))),
        }
    }

    fn shrink(&self) -> Box<Iterator<Item = Self>> {
        fn append(result: &mut Vec<Filelog>, f: Filelog) {
            result.append(&mut f.shrink().collect());
            result.push(f);
        }

        let mut result = Vec::new();

        if self.path.mpath() != Some(&MPath::new(b"test").unwrap()) {
            let mut f = self.clone();
            f.path = RepoPath::file(MPath::new(b"test").unwrap()).unwrap();
            append(&mut result, f);
        }

        if self.node != NULL_HASH {
            let mut f = self.clone();
            f.node = NULL_HASH;
            append(&mut result, f);
        }

        if self.p1 != None {
            let mut f = self.clone();
            f.p1 = None;
            append(&mut result, f);
        }

        if self.p2 != None {
            let mut f = self.clone();
            f.p2 = None;
            append(&mut result, f);
        }

        if self.linknode != NULL_HASH {
            let mut f = self.clone();
            f.linknode = NULL_HASH;
            append(&mut result, f);
        }

        if self.blob.size() != Some(0) {
            let mut f = self.clone();
            f.blob = Blob::from(Bytes::from(Vec::new()));
            append(&mut result, f);
        }

        Box::new(result.into_iter())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::cmp::min;

    use futures::Future;
    use futures::stream::iter_ok;
    use itertools::{assert_equal, EitherOrBoth, Itertools};

    use mercurial_types::delta::Fragment;

    struct NodeHashGen {
        bytes: Vec<u8>,
    }

    impl NodeHashGen {
        fn new() -> Self {
            Self {
                bytes: Vec::from(NULL_HASH.as_ref()),
            }
        }

        fn next(&mut self) -> NodeHash {
            for i in 0..self.bytes.len() {
                if self.bytes[i] == 255 {
                    self.bytes[i] = 0;
                } else {
                    self.bytes[i] = self.bytes[i] + 1;
                    return NodeHash::from_bytes(self.bytes.as_slice()).unwrap();
                }
            }

            panic!("NodeHashGen overflow");
        }
    }

    fn check_conversion<I, J>(inp: I, exp: J)
    where
        I: IntoIterator<Item = FilelogDeltaed>,
        J: IntoIterator<Item = Filelog>,
    {
        let result = convert_to_revlog_filelog(iter_ok(inp.into_iter().collect::<Vec<_>>()))
            .collect()
            .wait()
            .unwrap();

        assert_equal(result, exp);
    }

    fn filelog_to_deltaed(f: &Filelog) -> FilelogDeltaed {
        FilelogDeltaed {
            path: f.path.mpath().unwrap().clone(),
            chunk: CgDeltaChunk {
                node: f.node.clone(),
                p1: f.p1.clone().unwrap_or(NULL_HASH),
                p2: f.p2.clone().unwrap_or(NULL_HASH),
                base: NULL_HASH,
                linknode: f.linknode.clone(),
                delta: Delta::new_fulltext(f.blob.as_slice().unwrap()),
            },
        }
    }

    fn compute_delta(b1: &[u8], b2: &[u8]) -> Delta {
        let mut frags = Vec::new();
        let mut start = 0;
        let mut frag = Vec::new();
        for (idx, val) in b1.iter().zip_longest(b2.iter()).enumerate() {
            match val {
                EitherOrBoth::Both(v1, v2) => {
                    if v1 == v2 && !frag.is_empty() {
                        frags.push(Fragment {
                            start,
                            end: start + frag.len(),
                            content: mem::replace(&mut frag, Vec::new()),
                        });
                    } else if v1 != v2 {
                        if frag.is_empty() {
                            start = idx;
                        }
                        frag.push(*v2);
                    }
                }
                EitherOrBoth::Left(_) => continue,
                EitherOrBoth::Right(v) => {
                    if frag.is_empty() {
                        start = idx;
                    }
                    frag.push(*v)
                }
            }
        }
        if !frag.is_empty() {
            frags.push(Fragment {
                start,
                end: min(start + frag.len(), b1.len()),
                content: mem::replace(&mut frag, Vec::new()),
            });
        }
        if b1.len() > b2.len() {
            frags.push(Fragment {
                start: b2.len(),
                end: b1.len(),
                content: Vec::new(),
            });
        }

        Delta::new(frags).unwrap()
    }

    #[test]
    fn two_fulltext_files() {
        use mercurial_types_mocks::nodehash::*;

        let f1 = Filelog {
            path: RepoPath::file(MPath::new(b"test").unwrap()).unwrap(),
            node: ONES_HASH,
            p1: Some(TWOS_HASH),
            p2: Some(THREES_HASH),
            linknode: FOURS_HASH,
            blob: Blob::from(Bytes::from("test file content")),
        };

        let f2 = Filelog {
            path: RepoPath::file(MPath::new(b"test2").unwrap()).unwrap(),
            node: FIVES_HASH,
            p1: Some(SIXES_HASH),
            p2: Some(SEVENS_HASH),
            linknode: EIGHTS_HASH,
            blob: Blob::from(Bytes::from("test2 file content")),
        };

        check_conversion(
            vec![filelog_to_deltaed(&f1), filelog_to_deltaed(&f2)],
            vec![f1, f2],
        );
    }

    fn files_check_order(correct_order: bool) {
        use mercurial_types_mocks::nodehash::*;

        let f1 = Filelog {
            path: RepoPath::file(MPath::new(b"test").unwrap()).unwrap(),
            node: ONES_HASH,
            p1: Some(TWOS_HASH),
            p2: Some(THREES_HASH),
            linknode: FOURS_HASH,
            blob: Blob::from(Bytes::from("test file content")),
        };

        let f2 = Filelog {
            path: RepoPath::file(MPath::new(b"test2").unwrap()).unwrap(),
            node: FIVES_HASH,
            p1: Some(SIXES_HASH),
            p2: Some(SEVENS_HASH),
            linknode: EIGHTS_HASH,
            blob: Blob::from(Bytes::from("test2 file content")),
        };

        let f1_deltaed = filelog_to_deltaed(&f1);
        let mut f2_deltaed = filelog_to_deltaed(&f2);

        f2_deltaed.chunk.base = f1.node.clone();
        f2_deltaed.chunk.delta =
            compute_delta(f1.blob.as_slice().unwrap(), f2.blob.as_slice().unwrap());

        let inp = if correct_order {
            vec![f1_deltaed, f2_deltaed]
        } else {
            vec![f2_deltaed, f1_deltaed]
        };

        let result = convert_to_revlog_filelog(iter_ok(inp)).collect().wait();

        match result {
            Ok(_) => assert!(
                correct_order,
                "Successfuly converted even though order was incorrect"
            ),
            Err(_) => assert!(
                !correct_order,
                "Filed to convert even though order was correct"
            ),
        }
    }

    #[test]
    fn files_order_correct() {
        files_check_order(true);
    }

    #[test]
    fn files_order_incorrect() {
        files_check_order(false);
    }

    quickcheck! {
        fn sanitycheck_delta_computation(b1: Vec<u8>, b2: Vec<u8>) -> bool {
            assert_equal(&b2, &delta::apply(&b1, &compute_delta(&b1, &b2)));
            true
        }

        fn correct_conversion_single(f: Filelog) -> bool {
            check_conversion(
                vec![filelog_to_deltaed(&f)],
                vec![f],
            );

            true
        }

        fn correct_conversion_delta_against_first(f: Filelog, fs: Vec<Filelog>) -> bool {
            let mut hash_gen = NodeHashGen::new();

            let mut f = f.clone();
            f.node = hash_gen.next();

            let mut fs = fs.clone();
            for el in fs.iter_mut() {
                el.node = hash_gen.next();
            }

            let mut deltas = vec![filelog_to_deltaed(&f)];
            for filelog in &fs {
                let mut delta = filelog_to_deltaed(filelog);
                delta.chunk.base = f.node.clone();
                delta.chunk.delta =
                    compute_delta(f.blob.as_slice().unwrap(), filelog.blob.as_slice().unwrap());
                deltas.push(delta);
            }

            check_conversion(deltas, vec![f].into_iter().chain(fs));

            true
        }

        fn correct_conversion_delta_against_next(fs: Vec<Filelog>) -> bool {
            let mut hash_gen = NodeHashGen::new();

            let mut fs = fs.clone();
            for el in fs.iter_mut() {
                el.node = hash_gen.next();
            }

            let deltas = {
                let mut it = fs.iter();
                let mut deltas = match it.next() {
                    None => return true, // empty test case
                    Some(f) => vec![filelog_to_deltaed(f)],
                };

                for (prev, next) in fs.iter().zip(it) {
                    let mut delta = filelog_to_deltaed(next);
                    delta.chunk.base = prev.node.clone();
                    delta.chunk.delta =
                        compute_delta(prev.blob.as_slice().unwrap(), next.blob.as_slice().unwrap());
                    deltas.push(delta);
                }

                deltas
            };

            check_conversion(deltas, fs);

            true
        }
    }
}
