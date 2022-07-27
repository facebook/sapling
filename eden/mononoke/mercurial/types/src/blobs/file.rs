/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Plain files, symlinks

use super::errors::ErrorKind;
use crate::FileBytes;
use crate::HgBlob;
use crate::HgBlobNode;
use crate::HgFileEnvelope;
use crate::HgFileNodeId;
use crate::MPath;
use anyhow::Result;
use async_trait::async_trait;
use blobstore::Blobstore;
use blobstore::Loadable;
use blobstore::LoadableError;
use bytes::Bytes;
use context::CoreContext;
use itertools::Itertools;
use mononoke_types::hash::Sha256;
use std::collections::HashMap;
use std::io::Write;
use std::str;
use std::str::FromStr;

#[async_trait]
impl Loadable for HgFileNodeId {
    type Value = HgFileEnvelope;

    async fn load<'a, B: Blobstore>(
        &'a self,
        ctx: &'a CoreContext,
        blobstore: &'a B,
    ) -> Result<Self::Value, LoadableError> {
        let blobstore_key = self.blobstore_key();
        let bytes = blobstore.get(ctx, &blobstore_key).await?;
        let blobstore_bytes = match bytes {
            Some(bytes) => bytes,
            None => return Err(LoadableError::Missing(blobstore_key)),
        };
        Ok(HgFileEnvelope::from_blob(blobstore_bytes.into())?)
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
                .map_or(META_SZ, |((idx, _), _)| idx + META_SZ * 2); // XXX malformed if None - unterminated metadata

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
        Self::extract_copied_from(buf)
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
            (Some(Err(e)), _) => Err(e.context("invalid path in copy metadata")),
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
            .ok_or_else(|| {
                ErrorKind::IncorrectLfsFileContent(
                    "VERSION mandatory field parsing failed in Lfs file content".to_string(),
                )
            })?;

        let oid = contents
            .get(OID)
            .and_then(|s| str::from_utf8(*s).ok())
            .and_then(|s| {
                let prefix_len = SHA256_PREFIX.len();

                let check = prefix_len <= s.len() && s[..prefix_len].as_bytes() == SHA256_PREFIX;
                if check {
                    Some(s[prefix_len..].to_string())
                } else {
                    None
                }
            })
            .and_then(|s| Sha256::from_str(&s).ok())
            .ok_or_else(|| {
                ErrorKind::IncorrectLfsFileContent(
                    "OID mandatory field parsing failed in Lfs file content".to_string(),
                )
            })?;
        let size = contents
            .get(SIZE)
            .and_then(|s| str::from_utf8(*s).ok())
            .and_then(|s| s.parse::<u64>().ok())
            .ok_or_else(|| {
                ErrorKind::IncorrectLfsFileContent(
                    "SIZE mandatory field parsing failed in Lfs file content".to_string(),
                )
            })?;
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
