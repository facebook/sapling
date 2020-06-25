/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Plain files, symlinks

use super::envelope::HgBlobEnvelope;
use super::errors::ErrorKind;
use super::manifest::{fetch_manifest_envelope, fetch_raw_manifest_bytes, BlobManifest};
use crate::{
    calculate_hg_node_id,
    manifest::{Content, HgEntry, HgManifest, Type},
    nodehash::HgEntryId,
    FileBytes, FileType, HgBlob, HgBlobNode, HgFileEnvelope, HgFileNodeId, HgManifestId,
    HgNodeHash, HgParents, MPath, MPathElement,
};
use anyhow::{Error, Result};
use blobstore::{Blobstore, Loadable, LoadableError};
use bytes::{Bytes, BytesMut};
use context::CoreContext;
use failure_ext::{FutureFailureErrorExt, StreamFailureErrorExt};
use filestore;
use futures::{
    compat::Future01CompatExt,
    future::{BoxFuture, FutureExt, TryFutureExt},
};
use futures_ext::{BoxFuture as BoxFuture01, FutureExt as _, StreamExt as _};
use futures_old::{
    future::{lazy, Future},
    stream::Stream,
};
use itertools::Itertools;
use mononoke_types::hash::Sha256;
use std::{
    collections::HashMap,
    io::Write,
    str::{self, FromStr},
    sync::Arc,
};

#[derive(Clone)]
pub struct HgBlobEntry {
    blobstore: Arc<dyn Blobstore>,
    name: Option<MPathElement>,
    id: HgEntryId,
}

impl PartialEq for HgBlobEntry {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name && self.id == other.id
    }
}

impl Eq for HgBlobEntry {}

pub fn fetch_raw_filenode_bytes(
    ctx: CoreContext,
    blobstore: &Arc<dyn Blobstore>,
    node_id: HgFileNodeId,
    validate_hash: bool,
) -> BoxFuture01<HgBlob, Error> {
    node_id
        .load(ctx.clone(), blobstore)
        .compat()
        .from_err()
        .and_then({
            let blobstore = blobstore.clone();
            move |envelope| {
                let envelope = envelope.into_mut();
                let content_id = envelope.content_id;

                // TODO (T47717165): Avoid buffering here.
                let file_bytes_fut = filestore::fetch_concat(&blobstore, ctx, content_id)
                    .map(FileBytes)
                    .context("While fetching content blob");

                let metadata = envelope.metadata;
                let f = if metadata.is_empty() {
                    file_bytes_fut
                        .map(|contents| contents.into_bytes())
                        .left_future()
                } else {
                    file_bytes_fut
                        .map(move |contents| {
                            // The copy info and the blob have to be joined together.
                            // TODO (T30456231): avoid the copy
                            let contents = contents.into_bytes();
                            let mut buff = BytesMut::with_capacity(metadata.len() + contents.len());
                            buff.extend_from_slice(&metadata);
                            buff.extend_from_slice(&contents);
                            buff.freeze()
                        })
                        .right_future()
                };

                let p1 = envelope.p1.map(|p| p.into_nodehash());
                let p2 = envelope.p2.map(|p| p.into_nodehash());
                f.and_then(move |content| {
                    if validate_hash {
                        let actual = HgFileNodeId::new(calculate_hg_node_id(
                            &content,
                            &HgParents::new(p1, p2),
                        ));

                        if actual != node_id {
                            return Err(ErrorKind::CorruptHgFileNode {
                                expected: node_id,
                                actual,
                            }
                            .into());
                        }
                    }
                    Ok(content)
                })
                .map(HgBlob::from)
            }
        })
        .from_err()
        .boxify()
}

impl Loadable for HgFileNodeId {
    type Value = HgFileEnvelope;

    fn load<B: Blobstore + Clone>(
        &self,
        ctx: CoreContext,
        blobstore: &B,
    ) -> BoxFuture<'static, Result<Self::Value, LoadableError>> {
        let blobstore_key = self.blobstore_key();
        let get = blobstore.get(ctx, blobstore_key.clone()).compat();
        async move {
            let bytes = get.await?;
            let blobstore_bytes = match bytes {
                Some(bytes) => bytes,
                None => return Err(LoadableError::Missing(blobstore_key)),
            };
            Ok(HgFileEnvelope::from_blob(blobstore_bytes.into())?)
        }
        .boxed()
    }
}

impl HgBlobEntry {
    pub fn new(
        blobstore: Arc<dyn Blobstore>,
        name: MPathElement,
        nodeid: HgNodeHash,
        ty: Type,
    ) -> Self {
        Self {
            blobstore,
            name: Some(name),
            id: match ty {
                Type::Tree => HgEntryId::Manifest(HgManifestId::new(nodeid)),
                Type::File(file_type) => HgEntryId::File(file_type, HgFileNodeId::new(nodeid)),
            },
        }
    }

    pub fn new_root(blobstore: Arc<dyn Blobstore>, manifestid: HgManifestId) -> Self {
        Self {
            blobstore,
            name: None,
            id: manifestid.into(),
        }
    }

    fn get_raw_content_inner(&self, ctx: CoreContext) -> BoxFuture01<HgBlob, Error> {
        let validate_hash = false;
        match self.id {
            HgEntryId::Manifest(manifest_id) => {
                fetch_raw_manifest_bytes(ctx, &self.blobstore, manifest_id)
            }
            HgEntryId::File(_, filenode_id) => {
                // TODO (torozco) T48791324: Identify if get_raw_content is being used at all on
                // filenodes, and remove callers so we can remove it. As-is, if called, this could
                // try to access arbitrarily large files.
                fetch_raw_filenode_bytes(ctx, &self.blobstore, filenode_id, validate_hash)
            }
        }
    }

    pub fn get_envelope(&self, ctx: CoreContext) -> BoxFuture01<Box<dyn HgBlobEnvelope>, Error> {
        match self.id {
            HgEntryId::Manifest(hash) => fetch_manifest_envelope(ctx, &self.blobstore, hash)
                .map(|e| Box::new(e) as Box<dyn HgBlobEnvelope>)
                .left_future(),
            HgEntryId::File(_, hash) => hash
                .load(ctx, &self.blobstore)
                .compat()
                .from_err()
                .map(|e| Box::new(e) as Box<dyn HgBlobEnvelope>)
                .right_future(),
        }
        .boxify()
    }
}

impl HgEntry for HgBlobEntry {
    fn get_type(&self) -> Type {
        self.id.get_type()
    }

    fn get_parents(&self, ctx: CoreContext) -> BoxFuture01<HgParents, Error> {
        self.get_envelope(ctx).map(|e| e.get_parents()).boxify()
    }

    fn get_raw_content(&self, ctx: CoreContext) -> BoxFuture01<HgBlob, Error> {
        self.get_raw_content_inner(ctx)
    }

    fn get_content(&self, ctx: CoreContext) -> BoxFuture01<Content, Error> {
        let blobstore = self.blobstore.clone();

        let id = self.id.clone();
        let name = self.name.clone();
        // Note: do not remove `lazy(|| ...)` below! It helps with memory usage on serving
        // gettreepack requests.
        match self.id {
            HgEntryId::Manifest(manifest_id) => lazy(move || {
                BlobManifest::load(ctx, blobstore, manifest_id)
                    .and_then({
                        move |blob_manifest| {
                            let manifest = blob_manifest.ok_or(ErrorKind::HgContentMissing(
                                id.into_nodehash(),
                                Type::Tree,
                            ))?;
                            Ok(Content::Tree(manifest.boxed()))
                        }
                    })
                    .context(format!(
                        "While HgBlobEntry::get_content for id {}, name {:?}",
                        id, name,
                    ))
                    .from_err()
            })
            .boxify(),
            HgEntryId::File(file_type, filenode_id) => lazy(move || {
                filenode_id
                    .load(ctx.clone(), &blobstore)
                    .compat()
                    .from_err()
                    .map(move |envelope| {
                        let envelope = envelope.into_mut();
                        let content_id = envelope.content_id;

                        let stream = filestore::fetch_stream(&blobstore, ctx, content_id)
                            .map(FileBytes)
                            .context("While fetching content blob")
                            .from_err()
                            .boxify();

                        match file_type {
                            FileType::Regular => Content::File(stream),
                            FileType::Executable => Content::Executable(stream),
                            FileType::Symlink => Content::Symlink(stream),
                        }
                    })
                    .context(format!(
                        "While HgBlobEntry::get_content for id {}, name {:?}",
                        id, name
                    ))
                    .from_err()
            })
            .boxify(),
        }
    }

    fn get_size(&self, ctx: CoreContext) -> BoxFuture01<Option<u64>, Error> {
        self.get_envelope(ctx).map(|e| e.get_size()).boxify()
    }

    fn get_hash(&self) -> HgEntryId {
        self.id
    }

    fn get_name(&self) -> Option<&MPathElement> {
        self.name.as_ref()
    }
}

/// A Mercurial file. Knows about its parents, and might content inline metadata.
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct File {
    node: HgBlobNode,
}

pub const META_MARKER: &[u8] = b"\x01\n";
const COPY_PATH_KEY: &[u8] = b"copy";
const COPY_REV_KEY: &[u8] = b"copyrev";
pub const META_SZ: usize = 2;

impl File {
    pub fn new<B: Into<HgBlob>>(
        blob: B,
        p1: Option<HgFileNodeId>,
        p2: Option<HgFileNodeId>,
    ) -> Self {
        let node = HgBlobNode::new(
            blob,
            p1.map(HgFileNodeId::into_nodehash),
            p2.map(HgFileNodeId::into_nodehash),
        );
        File { node }
    }

    // (there's a use case for not providing parents, so should parents not be inside the file?)
    #[inline]
    pub fn data_only<B: Into<HgBlob>>(blob: B) -> Self {
        Self::new(blob, None, None)
    }

    // Note that this function drops empty metadata. For lossless preservation, use the metadata
    // function instead.
    pub fn extract_meta(file: &[u8]) -> (&[u8], usize) {
        if file.len() < META_SZ {
            return (&[], 0);
        }
        if &file[..META_SZ] != META_MARKER {
            (&[], 0)
        } else {
            let metasz = &file[META_SZ..]
                .iter()
                .enumerate()
                .tuple_windows()
                .find(|&((_, a), (_, b))| *a == META_MARKER[0] && *b == META_MARKER[1])
                .map(|((idx, _), _)| idx + META_SZ * 2)
                .unwrap_or(META_SZ); // XXX malformed if None - unterminated metadata

            let metasz = *metasz;
            if metasz >= META_SZ * 2 {
                (&file[META_SZ..metasz - META_SZ], metasz)
            } else {
                (&[], metasz)
            }
        }
    }

    pub fn parse_to_hash_map<'a>(
        content: &'a [u8],
        delimiter: &[u8],
    ) -> HashMap<&'a [u8], &'a [u8]> {
        let mut kv = HashMap::new();
        let delimiter_len = delimiter.len();

        for line in content.split(|c| *c == b'\n') {
            if line.len() < delimiter_len {
                continue;
            }

            // split on "delimiter" - no quoting within key/value
            for idx in 0..line.len() - delimiter_len + 1 {
                if &line[idx..idx + delimiter_len] == delimiter {
                    kv.insert(&line[..idx], &line[idx + delimiter_len..]);
                    break;
                }
            }
        }
        kv
    }

    pub fn parse_meta(file: &[u8]) -> HashMap<&[u8], &[u8]> {
        let (meta, _) = Self::extract_meta(file);

        // Yay, Mercurial has yet another ad-hoc encoding. This one is kv pairs separated by \n,
        // with ": " separating the key and value
        Self::parse_to_hash_map(meta, &[b':', b' '])
    }

    pub fn parse_content_to_lfs_hash_map(content: &[u8]) -> HashMap<&[u8], &[u8]> {
        Self::parse_to_hash_map(content, &[b' '])
    }

    pub fn copied_from(&self) -> Result<Option<(MPath, HgFileNodeId)>> {
        let buf = self.node.as_blob().as_slice();
        Self::extract_copied_from(&buf)
    }

    fn get_copied_from_with_keys(
        meta: &HashMap<&[u8], &[u8]>,
        copy_path_key: &'static [u8],
        copy_rev_key: &'static [u8],
    ) -> Result<Option<(MPath, HgFileNodeId)>> {
        let path = meta.get(copy_path_key).cloned().map(MPath::new);
        let nodeid = meta
            .get(copy_rev_key)
            .and_then(|rev| str::from_utf8(rev).ok())
            .and_then(|rev| rev.parse().map(HgFileNodeId::new).ok());
        match (path, nodeid) {
            (Some(Ok(path)), Some(nodeid)) => Ok(Some((path, nodeid))),
            (Some(Err(e)), _) => Err(e.context("invalid path in copy metadata").into()),
            _ => Ok(None),
        }
    }

    pub fn extract_copied_from(buf: &[u8]) -> Result<Option<(MPath, HgFileNodeId)>> {
        let meta = Self::parse_meta(buf);
        Self::get_copied_from_with_keys(&meta, COPY_PATH_KEY, COPY_REV_KEY)
    }

    pub fn generate_metadata<T>(
        copy_from: Option<&(MPath, HgFileNodeId)>,
        file_bytes: &FileBytes,
        buf: &mut T,
    ) -> Result<()>
    where
        T: Write,
    {
        match copy_from {
            None => {
                if file_bytes.starts_with(META_MARKER) {
                    // If the file contents starts with META_MARKER, the metadata must be
                    // written out to avoid ambiguity.
                    buf.write_all(META_MARKER)?;
                    buf.write_all(META_MARKER)?;
                }
            }
            Some((path, version)) => {
                buf.write_all(META_MARKER)?;
                buf.write_all(COPY_PATH_KEY)?;
                buf.write_all(b": ")?;
                path.generate(buf)?;
                buf.write_all(b"\n")?;

                buf.write_all(COPY_REV_KEY)?;
                buf.write_all(b": ")?;
                buf.write_all(version.to_hex().as_ref())?;
                buf.write_all(b"\n")?;
                buf.write_all(META_MARKER)?;
            }
        };
        Ok(())
    }

    pub fn content(&self) -> &[u8] {
        let data = self.node.as_blob().as_slice();
        let (_, off) = Self::extract_meta(data);
        &data[off..]
    }

    pub fn metadata(&self) -> Bytes {
        let data = self.node.as_blob().as_inner();
        let (_, off) = Self::extract_meta(data);
        data.slice(..off)
    }

    pub fn file_contents(&self) -> FileBytes {
        let data = self.node.as_blob().as_inner();
        let (_, off) = Self::extract_meta(data);
        FileBytes(data.slice(off..))
    }

    pub fn get_lfs_content(&self) -> Result<LFSContent> {
        let data = self.node.as_blob().as_inner();
        let (_, off) = Self::extract_meta(data);

        Self::get_lfs_struct(&Self::parse_content_to_lfs_hash_map(&data.slice(off..)))
    }

    fn parse_mandatory_lfs(contents: &HashMap<&[u8], &[u8]>) -> Result<(String, Sha256, u64)> {
        let version = contents
            .get(VERSION)
            .and_then(|s| str::from_utf8(*s).ok())
            .map(|s| s.to_string())
            .ok_or(ErrorKind::IncorrectLfsFileContent(
                "VERSION mandatory field parsing failed in Lfs file content".to_string(),
            ))?;

        let oid = contents
            .get(OID)
            .and_then(|s| str::from_utf8(*s).ok())
            .and_then(|s| {
                let prefix_len = SHA256_PREFIX.len();

                let check = prefix_len <= s.len() && &s[..prefix_len].as_bytes() == &SHA256_PREFIX;
                if check {
                    Some(s[prefix_len..].to_string())
                } else {
                    None
                }
            })
            .and_then(|s| Sha256::from_str(&s).ok())
            .ok_or(ErrorKind::IncorrectLfsFileContent(
                "OID mandatory field parsing failed in Lfs file content".to_string(),
            ))?;
        let size = contents
            .get(SIZE)
            .and_then(|s| str::from_utf8(*s).ok())
            .and_then(|s| s.parse::<u64>().ok())
            .ok_or(ErrorKind::IncorrectLfsFileContent(
                "SIZE mandatory field parsing failed in Lfs file content".to_string(),
            ))?;
        Ok((version, oid, size))
    }

    pub fn get_lfs_struct(contents: &HashMap<&[u8], &[u8]>) -> Result<LFSContent> {
        Self::parse_mandatory_lfs(contents)
            .and_then(|(version, oid, size)| {
                Self::get_copied_lfs(contents).map(move |copy_from| (version, oid, size, copy_from))
            })
            .map(|(version, oid, size, copy_from)| LFSContent {
                version,
                oid,
                size,
                copy_from,
            })
    }

    fn get_copied_lfs(contents: &HashMap<&[u8], &[u8]>) -> Result<Option<(MPath, HgFileNodeId)>> {
        Self::get_copied_from_with_keys(contents, HGCOPY, HGCOPYREV)
    }

    pub fn generate_lfs_file(
        oid: Sha256,
        size: u64,
        copy_from: Option<(MPath, HgFileNodeId)>,
    ) -> Result<Bytes> {
        let git_version = String::from_utf8(GIT_VERSION.to_vec())?;
        let lfs_content = LFSContent {
            version: git_version,
            oid,
            size,
            copy_from,
        };
        lfs_content.into_bytes()
    }
}

const VERSION: &[u8] = b"version";
const OID: &[u8] = b"oid";
const SIZE: &[u8] = b"size";
const HGCOPY: &[u8] = b"x-hg-copy";
const HGCOPYREV: &[u8] = b"x-hg-copyrev";
const _ISBINARY: &[u8] = b"x-is-binary";
const GIT_VERSION: &[u8] = b"https://git-lfs.github.com/spec/v1";
const SHA256_PREFIX: &[u8] = b"sha256:";

// See [https://www.mercurial-scm.org/wiki/LfsPlan], By default, version, oid and size are required
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct LFSContent {
    // mandatory fields
    version: String,
    oid: Sha256,
    size: u64,

    // copy fields
    copy_from: Option<(MPath, HgFileNodeId)>,
}

impl LFSContent {
    pub fn new(
        version: String,
        oid: Sha256,
        size: u64,
        copy_from: Option<(MPath, HgFileNodeId)>,
    ) -> Self {
        Self {
            version,
            oid,
            size,
            copy_from,
        }
    }

    pub fn size(&self) -> u64 {
        self.size
    }

    pub fn oid(&self) -> Sha256 {
        self.oid.clone()
    }

    pub fn copy_from(&self) -> Option<(MPath, HgFileNodeId)> {
        self.copy_from.clone()
    }

    pub fn into_bytes(&self) -> Result<Bytes> {
        let mut out: Vec<u8> = vec![];

        out.write_all(VERSION)?;
        out.write_all(b" ")?;
        out.write_all(self.version.as_ref())?;
        out.write_all(b"\n")?;

        out.write_all(OID)?;
        out.write_all(b" ")?;
        out.write_all(SHA256_PREFIX)?;
        out.write_all(self.oid.to_hex().as_ref())?;
        out.write_all(b"\n")?;

        out.write_all(SIZE)?;
        out.write_all(b" ")?;
        out.write_all(format!("{}", self.size).as_ref())?;
        out.write_all(b"\n")?;

        if let Some((ref mpath, ref nodehash)) = self.copy_from {
            out.write_all(HGCOPY)?;
            out.write_all(b" ")?;
            mpath.generate(&mut out)?;
            out.write_all(b"\n")?;

            out.write_all(HGCOPYREV)?;
            out.write_all(b" ")?;
            out.write_all(nodehash.to_hex().as_ref())?;
            out.write_all(b"\n")?;
        }

        Ok(Bytes::from(out))
    }
}
