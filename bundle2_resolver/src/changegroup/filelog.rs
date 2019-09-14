// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::collections::HashMap;
use std::mem;

use bytes::Bytes;
use cloned::cloned;
use context::CoreContext;
use failure::{format_err, Compat};
use failure_ext::bail_msg;
use futures::future::Shared;
use futures::{Future, IntoFuture, Stream};
use futures_ext::{BoxFuture, BoxStream, FutureExt, StreamExt};
use heapsize::HeapSizeOf;
use quickcheck::{Arbitrary, Gen};

use blobrepo::{
    BlobRepo, ContentBlobInfo, ContentBlobMeta, UploadHgFileContents, UploadHgFileEntry,
    UploadHgNodeHash,
};
use mercurial_bundles::changegroup::CgDeltaChunk;
use mercurial_types::{
    blobs::{File, HgBlobEntry},
    delta, parse_rev_flags, Delta, FileType, HgFileNodeId, HgNodeHash, HgNodeKey, MPath, RepoPath,
    RevFlags, NULL_HASH,
};
use remotefilelog::create_raw_filenode_blob;

use crate::errors::*;
use crate::stats::*;
use crate::upload_blobs::UploadableHgBlob;

#[derive(Debug, Eq, PartialEq)]
pub struct FilelogDeltaed {
    pub path: MPath,
    pub chunk: CgDeltaChunk,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum FilelogData {
    RawBytes(Bytes),
    LfsMetaData(ContentBlobMeta),
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Filelog {
    pub node_key: HgNodeKey,
    pub p1: Option<HgNodeHash>,
    pub p2: Option<HgNodeHash>,
    pub linknode: HgNodeHash,
    pub data: FilelogData,
    pub flags: RevFlags,
}

impl UploadableHgBlob for Filelog {
    // * Shared is required here because a single file node can be referred to by more than
    //   one changeset, and all of those will want to refer to the corresponding future.
    // * The Compat<Error> here is because the error type for Shared (a cloneable wrapper called
    //   SharedError) doesn't implement Fail, and only implements Error if the wrapped type
    //   implements Error.
    type Value = (
        ContentBlobInfo,
        Shared<BoxFuture<(HgBlobEntry, RepoPath), Compat<Error>>>,
    );

    fn upload(self, ctx: CoreContext, repo: &BlobRepo) -> Result<(HgNodeKey, Self::Value)> {
        let node_key = self.node_key;
        let path = match &node_key.path {
            RepoPath::FilePath(path) => path.clone(),
            other => bail_msg!("internal error: expected file path, got {}", other),
        };

        // If LFSMetaData
        let contents = match self.data {
            FilelogData::RawBytes(bytes) => UploadHgFileContents::RawBytes(bytes),
            FilelogData::LfsMetaData(meta) => UploadHgFileContents::ContentUploaded(meta),
        };

        let upload = UploadHgFileEntry {
            upload_node_id: UploadHgNodeHash::Checked(node_key.hash),
            contents,
            // XXX should this really be Regular?
            file_type: FileType::Regular,
            p1: self.p1.map(HgFileNodeId::new),
            p2: self.p2.map(HgFileNodeId::new),
            path,
        };

        let (cbinfo, fut) = upload.upload(ctx, repo.get_blobstore().boxed())?;
        Ok((
            node_key,
            (cbinfo, fut.map_err(Error::compat).boxify().shared()),
        ))
    }
}

pub fn convert_to_revlog_filelog<S>(
    ctx: CoreContext,
    repo: BlobRepo,
    deltaed: S,
) -> BoxStream<Filelog, Error>
where
    S: Stream<Item = FilelogDeltaed, Error = Error> + Send + 'static,
{
    let mut delta_cache = DeltaCache::new(repo.clone());
    deltaed
        .map(move |FilelogDeltaed { path, chunk }| {
            let CgDeltaChunk {
                node,
                base,
                delta,
                p1,
                p2,
                linknode,
                flags: flags_value,
            } = chunk;

            delta_cache
                .decode(ctx.clone(), node.clone(), base.into_option(), delta)
                .and_then({
                    cloned!(ctx, node, path, repo);
                    move |data| {
                        parse_rev_flags(flags_value)
                            .into_future()
                            .and_then(move |flags| {
                                get_filelog_data(ctx.clone(), repo, data, flags).map(
                                    move |file_log_data| Filelog {
                                        node_key: HgNodeKey {
                                            path: RepoPath::FilePath(path),
                                            hash: node,
                                        },
                                        p1: p1.into_option(),
                                        p2: p2.into_option(),
                                        linknode,
                                        data: file_log_data,
                                        flags,
                                    },
                                )
                            })
                    }
                })
                .with_context(move |_| {
                    format!(
                        "While decoding delta cache for file id {}, path {}",
                        node, path
                    )
                })
                .from_err()
                .boxify()
        })
        .buffer_unordered(100)
        .boxify()
}

fn generate_lfs_meta_data(
    ctx: CoreContext,
    repo: BlobRepo,
    data: Bytes,
) -> impl Future<Item = ContentBlobMeta, Error = Error> {
    // TODO(anastasiyaz): check size
    File::data_only(data)
        .get_lfs_content()
        .into_future()
        .and_then(move |lfs_content| {
            (
                repo.get_file_content_id_by_sha256(ctx, lfs_content.oid()),
                Ok(lfs_content.copy_from()),
                Ok(lfs_content.size()),
            )
        })
        .map(move |(content_id, copy_from, size)| ContentBlobMeta {
            id: content_id,
            copy_from,
            size,
        })
}

fn get_filelog_data(
    ctx: CoreContext,
    repo: BlobRepo,
    data: Bytes,
    flags: RevFlags,
) -> impl Future<Item = FilelogData, Error = Error> {
    if flags.contains(RevFlags::REVIDX_EXTSTORED) {
        generate_lfs_meta_data(ctx, repo, data)
            .map(|cbmeta| FilelogData::LfsMetaData(cbmeta))
            .left_future()
    } else {
        Ok(FilelogData::RawBytes(data)).into_future().right_future()
    }
}

struct DeltaCache {
    repo: BlobRepo,
    bytes_cache: HashMap<HgNodeHash, Shared<BoxFuture<Bytes, Compat<Error>>>>,
}

impl DeltaCache {
    fn new(repo: BlobRepo) -> Self {
        Self {
            repo,
            bytes_cache: HashMap::new(),
        }
    }

    fn decode(
        &mut self,
        ctx: CoreContext,
        node: HgNodeHash,
        base: Option<HgNodeHash>,
        delta: Delta,
    ) -> BoxFuture<Bytes, Error> {
        let bytes = match self.bytes_cache.get(&node).cloned() {
            Some(bytes) => bytes,
            None => {
                let dsize = delta.heap_size_of_children() as i64;
                STATS::deltacache_dsize.add_value(dsize);
                STATS::deltacache_dsize_large.add_value(dsize);

                let vec = match base {
                    None => delta::apply(b"", &delta)
                        .with_context(|_| format!("File content empty, delta: {:?}", delta))
                        .map_err(Error::from)
                        .map_err(Error::compat)
                        .into_future()
                        .boxify(),
                    Some(base) => {
                        let fut = match self.bytes_cache.get(&base) {
                            Some(bytes) => bytes
                                .clone()
                                .map_err(Error::from)
                                .and_then(move |bytes| {
                                    delta::apply(&bytes, &delta)
                                        .with_context(|_| {
                                            format!("File content: {:?} delta: {:?}", bytes, delta)
                                        })
                                        .map_err(Error::from)
                                })
                                .boxify(),
                            None => {
                                let validate_hash = false;
                                create_raw_filenode_blob(
                                    ctx,
                                    self.repo.clone(),
                                    HgFileNodeId::new(base),
                                    validate_hash,
                                )
                                .and_then(move |bytes| {
                                    delta::apply(bytes.as_ref(), &delta)
                                        .with_context(|_| {
                                            format!("File content: {:?} delta: {:?}", bytes, delta)
                                        })
                                        .map_err(Error::from)
                                })
                                .boxify()
                            }
                        };
                        fut.map_err(move |err| {
                            Error::from(err.context(format_err!(
                                "While looking for base {:?} to apply on delta {:?}",
                                base,
                                node
                            )))
                            .compat()
                        })
                        .boxify()
                    }
                };

                let bytes = vec.map(|vec| Bytes::from(vec)).boxify().shared();

                if self.bytes_cache.insert(node, bytes.clone()).is_some() {
                    panic!("Logic error: byte cache returned None for HashMap::get with node");
                }
                bytes
            }
        };

        bytes
            .inspect(|bytes| {
                let fsize = (mem::size_of::<u8>() * bytes.as_ref().len()) as i64;
                STATS::deltacache_fsize.add_value(fsize);
                STATS::deltacache_fsize_large.add_value(fsize);
            })
            .map(|bytes| (*bytes).clone())
            .from_err()
            .boxify()
    }
}

impl Arbitrary for Filelog {
    fn arbitrary<G: Gen>(g: &mut G) -> Self {
        Filelog {
            node_key: HgNodeKey {
                path: RepoPath::FilePath(MPath::arbitrary(g)),
                hash: HgNodeHash::arbitrary(g),
            },
            p1: HgNodeHash::arbitrary(g).into_option(),
            p2: HgNodeHash::arbitrary(g).into_option(),
            linknode: HgNodeHash::arbitrary(g),
            data: FilelogData::RawBytes(Bytes::from(Vec::<u8>::arbitrary(g))),
            flags: RevFlags::REVIDX_DEFAULT_FLAGS,
        }
    }

    fn shrink(&self) -> Box<dyn Iterator<Item = Self>> {
        fn append(result: &mut Vec<Filelog>, f: Filelog) {
            result.append(&mut f.shrink().collect());
            result.push(f);
        }

        let mut result = Vec::new();

        if self.node_key.hash != NULL_HASH {
            let mut f = self.clone();
            f.node_key.hash = NULL_HASH;
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

        if let FilelogData::RawBytes(ref bytes) = self.data {
            if bytes.len() != 0 {
                let mut f = self.clone();
                f.data = FilelogData::RawBytes(Bytes::from(Vec::new()));
                append(&mut result, f);
            }
        }

        Box::new(result.into_iter())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::cmp::min;

    use blobrepo_factory::new_memblob_empty;
    use fbinit::FacebookInit;
    use futures::stream::iter_ok;
    use futures::Future;
    use itertools::{assert_equal, EitherOrBoth, Itertools};
    use quickcheck::quickcheck;

    use mercurial_types::delta::Fragment;
    use mercurial_types::NULL_HASH;
    use mercurial_types_mocks::nodehash::*;

    struct NodeHashGen {
        bytes: Vec<u8>,
    }

    impl NodeHashGen {
        fn new() -> Self {
            Self {
                bytes: Vec::from(NULL_HASH.as_ref()),
            }
        }

        fn next(&mut self) -> HgNodeHash {
            for i in 0..self.bytes.len() {
                if self.bytes[i] == 255 {
                    self.bytes[i] = 0;
                } else {
                    self.bytes[i] = self.bytes[i] + 1;
                    return HgNodeHash::from_bytes(self.bytes.as_slice()).unwrap();
                }
            }

            panic!("NodeHashGen overflow");
        }
    }

    fn check_conversion<I, J>(ctx: CoreContext, inp: I, exp: J)
    where
        I: IntoIterator<Item = FilelogDeltaed>,
        J: IntoIterator<Item = Filelog>,
    {
        let result = convert_to_revlog_filelog(
            ctx,
            new_memblob_empty(None).unwrap(),
            iter_ok(inp.into_iter().collect::<Vec<_>>()),
        )
        .collect()
        .wait()
        .unwrap();

        assert_equal(result, exp);
    }

    fn filelog_to_deltaed(f: &Filelog) -> FilelogDeltaed {
        match f.data {
            FilelogData::RawBytes(ref bytes) => FilelogDeltaed {
                path: f.node_key.path.mpath().unwrap().clone(),
                chunk: CgDeltaChunk {
                    node: f.node_key.hash.clone(),
                    p1: f.p1.clone().unwrap_or(NULL_HASH),
                    p2: f.p2.clone().unwrap_or(NULL_HASH),
                    base: NULL_HASH,
                    linknode: f.linknode.clone(),
                    delta: Delta::new_fulltext(bytes.as_ref()),
                    flags: None,
                },
            },
            _ => panic!("RawBytes FilelogData is only supported in tests"),
        }
    }

    fn filelog_compute_delta(b1: &FilelogData, b2: &FilelogData) -> Delta {
        match (b1, b2) {
            (FilelogData::RawBytes(b1_data), FilelogData::RawBytes(b2_data)) => {
                compute_delta(&b1_data, &b2_data)
            }
            _ => panic!("RawBytes FilelogData is only supported in tests"),
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

    #[fbinit::test]
    fn two_fulltext_files(fb: FacebookInit) {
        let ctx = CoreContext::test_mock(fb);
        let f1 = Filelog {
            node_key: HgNodeKey {
                path: RepoPath::FilePath(MPath::new(b"test").unwrap()),
                hash: ONES_HASH,
            },
            p1: Some(TWOS_HASH),
            p2: Some(THREES_HASH),
            linknode: FOURS_HASH,
            data: FilelogData::RawBytes(Bytes::from("test file content")),
            flags: RevFlags::REVIDX_DEFAULT_FLAGS,
        };

        let f2 = Filelog {
            node_key: HgNodeKey {
                path: RepoPath::FilePath(MPath::new(b"test2").unwrap()),
                hash: FIVES_HASH,
            },
            p1: Some(SIXES_HASH),
            p2: Some(SEVENS_HASH),
            linknode: EIGHTS_HASH,
            data: FilelogData::RawBytes(Bytes::from("test2 file content")),
            flags: RevFlags::REVIDX_DEFAULT_FLAGS,
        };

        check_conversion(
            ctx,
            vec![filelog_to_deltaed(&f1), filelog_to_deltaed(&f2)],
            vec![f1, f2],
        );
    }

    fn files_check_order(ctx: CoreContext, correct_order: bool) {
        let f1 = Filelog {
            node_key: HgNodeKey {
                path: RepoPath::FilePath(MPath::new(b"test").unwrap()),
                hash: ONES_HASH,
            },
            p1: Some(TWOS_HASH),
            p2: Some(THREES_HASH),
            linknode: FOURS_HASH,
            data: FilelogData::RawBytes(Bytes::from("test file content")),
            flags: RevFlags::REVIDX_DEFAULT_FLAGS,
        };

        let f2 = Filelog {
            node_key: HgNodeKey {
                path: RepoPath::FilePath(MPath::new(b"test2").unwrap()),
                hash: FIVES_HASH,
            },
            p1: Some(SIXES_HASH),
            p2: Some(SEVENS_HASH),
            linknode: EIGHTS_HASH,
            data: FilelogData::RawBytes(Bytes::from("test2 file content")),
            flags: RevFlags::REVIDX_DEFAULT_FLAGS,
        };

        let f1_deltaed = filelog_to_deltaed(&f1);
        let mut f2_deltaed = filelog_to_deltaed(&f2);

        f2_deltaed.chunk.base = f1.node_key.hash.clone();
        f2_deltaed.chunk.delta = filelog_compute_delta(&f1.data, &f2.data);

        let inp = if correct_order {
            vec![f1_deltaed, f2_deltaed]
        } else {
            vec![f2_deltaed, f1_deltaed]
        };

        let result = convert_to_revlog_filelog(ctx, new_memblob_empty(None).unwrap(), iter_ok(inp))
            .collect()
            .wait();

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

    #[fbinit::test]
    fn files_order_correct(fb: FacebookInit) {
        files_check_order(CoreContext::test_mock(fb), true);
    }

    #[fbinit::test]
    fn files_order_incorrect(fb: FacebookInit) {
        files_check_order(CoreContext::test_mock(fb), false);
    }

    quickcheck! {
        fn sanitycheck_delta_computation(b1: Vec<u8>, b2: Vec<u8>) -> bool {
            assert_equal(&b2, &delta::apply(&b1, &compute_delta(&b1, &b2)).unwrap());
            true
        }

        fn correct_conversion_single(f: Filelog) -> bool {
            // TODO: this needs to be passed down from #[fbinit::test] instead.
            let fb = *fbinit::FACEBOOK;

            let ctx = CoreContext::test_mock(fb);
            check_conversion(
                ctx,
                vec![filelog_to_deltaed(&f)],
                vec![f],
            );

            true
        }

        fn correct_conversion_delta_against_first(f: Filelog, fs: Vec<Filelog>) -> bool {
            // TODO: this needs to be passed down from #[fbinit::test] instead.
            let fb = *fbinit::FACEBOOK;

            let ctx = CoreContext::test_mock(fb);
            let mut hash_gen = NodeHashGen::new();

            let mut f = f.clone();
            f.node_key.hash = hash_gen.next();

            let mut fs = fs.clone();
            for el in fs.iter_mut() {
                el.node_key.hash = hash_gen.next();
            }

            let mut deltas = vec![filelog_to_deltaed(&f)];
            for filelog in &fs {
                let mut delta = filelog_to_deltaed(filelog);
                delta.chunk.base = f.node_key.hash.clone();
                delta.chunk.delta =
                    filelog_compute_delta(&f.data, &filelog.data);
                deltas.push(delta);
            }

            check_conversion(ctx, deltas, vec![f].into_iter().chain(fs));

            true
        }

        fn correct_conversion_delta_against_next(fs: Vec<Filelog>) -> bool {
            // TODO: this needs to be passed down from #[fbinit::test] instead.
            let fb = *fbinit::FACEBOOK;

            let ctx = CoreContext::test_mock(fb);
            let mut hash_gen = NodeHashGen::new();

            let mut fs = fs.clone();
            for el in fs.iter_mut() {
                el.node_key.hash = hash_gen.next();
            }

            let deltas = {
                let mut it = fs.iter();
                let mut deltas = match it.next() {
                    None => return true, // empty test case
                    Some(f) => vec![filelog_to_deltaed(f)],
                };

                for (prev, next) in fs.iter().zip(it) {
                    let mut delta = filelog_to_deltaed(next);
                    delta.chunk.base = prev.node_key.hash.clone();
                    delta.chunk.delta =
                        filelog_compute_delta(&prev.data, &next.data);
                    deltas.push(delta);
                }

                deltas
            };

            check_conversion(ctx, deltas, fs);

            true
        }
    }
}
