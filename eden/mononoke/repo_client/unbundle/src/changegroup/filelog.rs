/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::mem;

use anyhow::bail;
use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use bytes::Bytes;
use cloned::cloned;
use context::CoreContext;
use futures::future::BoxFuture;
use futures::Future;
use futures::FutureExt;
use futures::Stream;
use futures::TryFutureExt;
use futures::TryStreamExt;
use futures_ext::future::TryShared;
use futures_ext::FbTryFutureExt;
use quickcheck::Arbitrary;
use quickcheck::Gen;

use blobrepo::BlobRepo;
use blobstore::Loadable;
use filestore::FetchKey;
use mercurial_bundles::changegroup::CgDeltaChunk;
use mercurial_types::blobs::ContentBlobMeta;
use mercurial_types::blobs::File;
use mercurial_types::blobs::UploadHgFileContents;
use mercurial_types::blobs::UploadHgFileEntry;
use mercurial_types::blobs::UploadHgNodeHash;
use mercurial_types::delta;
use mercurial_types::Delta;
use mercurial_types::HgFileNodeId;
use mercurial_types::HgNodeHash;
use mercurial_types::HgNodeKey;
use mercurial_types::MPath;
use mercurial_types::RepoPath;
use mercurial_types::RevFlags;
use mercurial_types::NULL_HASH;
use remotefilelog::create_raw_filenode_blob;

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
    type Value = TryShared<BoxFuture<'static, Result<(HgFileNodeId, RepoPath)>>>;

    fn upload(self, ctx: &CoreContext, repo: &BlobRepo) -> Result<(HgNodeKey, Self::Value)> {
        let node_key = self.node_key;
        let path = match &node_key.path {
            RepoPath::FilePath(path) => path.clone(),
            other => bail!("internal error: expected file path, got {}", other),
        };

        // If LFSMetaData
        let contents = match self.data {
            FilelogData::RawBytes(bytes) => {
                UploadHgFileContents::RawBytes(bytes, repo.filestore_config())
            }
            FilelogData::LfsMetaData(meta) => UploadHgFileContents::ContentUploaded(meta),
        };

        let upload = UploadHgFileEntry {
            upload_node_id: UploadHgNodeHash::Checked(node_key.hash),
            contents,
            p1: self.p1.map(HgFileNodeId::new),
            p2: self.p2.map(HgFileNodeId::new),
        };

        let fut = upload.upload_with_path(ctx.clone(), repo.get_blobstore().boxed(), path);
        Ok((node_key, fut.boxed().try_shared()))
    }
}

pub(crate) fn convert_to_revlog_filelog(
    ctx: CoreContext,
    repo: BlobRepo,
    deltaed: impl Stream<Item = Result<FilelogDeltaed>> + Send + 'static,
) -> impl Stream<Item = Result<Filelog>> {
    let mut delta_cache = DeltaCache::new(repo.clone());
    deltaed
        .map_ok(move |FilelogDeltaed { path, chunk }| {
            let CgDeltaChunk {
                node,
                base,
                delta,
                p1,
                p2,
                linknode,
                flags,
            } = chunk;

            delta_cache
                .decode(&ctx, node, base.into_option(), delta)
                .and_then({
                    cloned!(ctx, node, path, repo);
                    move |data| {
                        cloned!(ctx, node, path, repo);
                        let flags = flags.unwrap_or(RevFlags::REVIDX_DEFAULT_FLAGS);
                        async move {
                            let file_log_data = get_filelog_data(&ctx, &repo, data, flags).await?;
                            Ok(Filelog {
                                node_key: HgNodeKey {
                                    path: RepoPath::FilePath(path),
                                    hash: node,
                                },
                                p1: p1.into_option(),
                                p2: p2.into_option(),
                                linknode,
                                data: file_log_data,
                                flags,
                            })
                        }
                    }
                })
                .map(move |res| {
                    res.with_context(move || {
                        format!(
                            "While decoding delta cache for file id {}, path {}",
                            node, path
                        )
                    })
                })
        })
        .try_buffer_unordered(100)
}

async fn generate_lfs_meta_data(
    ctx: &CoreContext,
    repo: &BlobRepo,
    data: Bytes,
) -> Result<ContentBlobMeta, Error> {
    // TODO(anastasiyaz): check size
    let lfs_content = File::data_only(data).get_lfs_content()?;
    let content_id = FetchKey::from(lfs_content.oid())
        .load(ctx, repo.blobstore())
        .await?;
    Ok(ContentBlobMeta {
        id: content_id,
        copy_from: lfs_content.copy_from(),
        size: lfs_content.size(),
    })
}

async fn get_filelog_data(
    ctx: &CoreContext,
    repo: &BlobRepo,
    data: Bytes,
    flags: RevFlags,
) -> Result<FilelogData, Error> {
    if flags.contains(RevFlags::REVIDX_EXTSTORED) {
        let cbmeta = generate_lfs_meta_data(ctx, repo, data).await?;
        Ok(FilelogData::LfsMetaData(cbmeta))
    } else {
        Ok(FilelogData::RawBytes(data))
    }
}

struct DeltaCache {
    repo: BlobRepo,
    bytes_cache: HashMap<HgNodeHash, TryShared<BoxFuture<'static, Result<Bytes>>>>,
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
        ctx: &CoreContext,
        node: HgNodeHash,
        base: Option<HgNodeHash>,
        delta: Delta,
    ) -> impl Future<Output = Result<Bytes>> {
        let bytes = self.bytes_cache.get(&node).cloned().unwrap_or_else(|| {
            let bytes = {
                let vec_u8 = match base {
                    None => async move {
                        delta::apply(b"", &delta)
                            .with_context(|| format!("File content empty, delta: {:?}", delta))
                    }
                    .left_future(),
                    Some(base) => self
                        .apply_delta_on_base(ctx, base, delta)
                        .map_err(move |err| {
                            err.context(format!(
                                "While looking for base {:?} to apply on delta {:?}",
                                base, node
                            ))
                        })
                        .right_future(),
                };
                vec_u8.map_ok(Bytes::from)
            };

            let bytes = bytes.boxed().try_shared();

            if self.bytes_cache.insert(node, bytes.clone()).is_some() {
                panic!("Logic error: byte cache returned Some for HashMap::get with node");
            }
            bytes
        });

        async move {
            let bytes = bytes.await?;

            let fsize = (mem::size_of::<u8>() * bytes.as_ref().len()) as i64;
            STATS::deltacache_fsize.add_value(fsize);
            STATS::deltacache_fsize_large.add_value(fsize);

            Ok(bytes)
        }
    }

    fn apply_delta_on_base(
        &self,
        ctx: &CoreContext,
        base: HgNodeHash,
        delta: Delta,
    ) -> impl Future<Output = Result<Vec<u8>>> {
        let cache_entry = self.bytes_cache.get(&base).cloned();
        cloned!(ctx, self.repo);
        async move {
            let bytes = match cache_entry {
                Some(bytes) => bytes.clone().await?,
                None => {
                    let validate_hash = false;
                    create_raw_filenode_blob(ctx, repo, HgFileNodeId::new(base), validate_hash)
                        .await?
                }
            };
            delta::apply(&bytes, &delta)
                .with_context(|| format!("File content: {:?} delta: {:?}", bytes, delta))
        }
    }
}

impl Arbitrary for Filelog {
    fn arbitrary(g: &mut Gen) -> Self {
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
            if !bytes.is_empty() {
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

    use fbinit::FacebookInit;
    use futures::stream::iter;
    use itertools::assert_equal;
    use itertools::EitherOrBoth;
    use itertools::Itertools;
    use quickcheck_macros::quickcheck;

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
                    self.bytes[i] += 1;
                    return HgNodeHash::from_bytes(self.bytes.as_slice()).unwrap();
                }
            }

            panic!("NodeHashGen overflow");
        }
    }

    async fn check_conversion<I, J>(ctx: CoreContext, inp: I, exp: J)
    where
        I: IntoIterator<Item = FilelogDeltaed>,
        J: IntoIterator<Item = Filelog>,
    {
        let repo_factory = test_repo_factory::build_empty(ctx.fb).unwrap();
        let result = convert_to_revlog_filelog(
            ctx,
            repo_factory,
            iter(inp.into_iter().map(Ok).collect::<Vec<_>>()),
        )
        .try_collect::<Vec<_>>()
        .await
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
                compute_delta(b1_data, b2_data)
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
                            content: std::mem::take(&mut frag),
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
                content: std::mem::take(&mut frag),
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
    async fn two_fulltext_files(fb: FacebookInit) {
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
        )
        .await;
    }

    async fn files_check_order(ctx: CoreContext, correct_order: bool) {
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

        let repo_factory = test_repo_factory::build_empty(ctx.fb).unwrap();
        let result = convert_to_revlog_filelog(ctx, repo_factory, iter(inp.into_iter().map(Ok)))
            .try_collect::<Vec<_>>()
            .await;

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
    async fn files_order_correct(fb: FacebookInit) {
        files_check_order(CoreContext::test_mock(fb), true).await;
    }

    #[fbinit::test]
    async fn files_order_incorrect(fb: FacebookInit) {
        files_check_order(CoreContext::test_mock(fb), false).await;
    }

    #[quickcheck]
    fn sanitycheck_delta_computation(b1: Vec<u8>, b2: Vec<u8>) -> bool {
        assert_equal(&b2, &delta::apply(&b1, &compute_delta(&b1, &b2)).unwrap());
        true
    }

    #[quickcheck_async::tokio]
    async fn correct_conversion_single(fb: FacebookInit, f: Filelog) -> bool {
        let ctx = CoreContext::test_mock(fb);
        check_conversion(ctx, vec![filelog_to_deltaed(&f)], vec![f]).await;

        true
    }

    #[quickcheck_async::tokio]
    async fn correct_conversion_delta_against_first(
        fb: FacebookInit,
        f: Filelog,
        fs: Vec<Filelog>,
    ) -> bool {
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
            delta.chunk.delta = filelog_compute_delta(&f.data, &filelog.data);
            deltas.push(delta);
        }

        check_conversion(ctx, deltas, vec![f].into_iter().chain(fs)).await;

        true
    }

    #[quickcheck_async::tokio]
    async fn correct_conversion_delta_against_next(fb: FacebookInit, fs: Vec<Filelog>) -> bool {
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
                delta.chunk.delta = filelog_compute_delta(&prev.data, &next.data);
                deltas.push(delta);
            }

            deltas
        };

        check_conversion(ctx, deltas, fs).await;

        true
    }
}
