// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::collections::HashMap;
use std::io::Write;
use std::str;

use itertools::Itertools;

use mercurial_types::{HgBlob, HgBlobNode, HgNodeHash, MPath};
use mononoke_types::FileContents;

use errors::*;

#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct File {
    node: HgBlobNode,
}

const META_MARKER: &[u8] = b"\x01\n";
const COPY_PATH_KEY: &[u8] = b"copy";
const COPY_REV_KEY: &[u8] = b"copyrev";
const META_SZ: usize = 2;

impl File {
    pub fn new<B: Into<HgBlob>>(blob: B, p1: Option<&HgNodeHash>, p2: Option<&HgNodeHash>) -> Self {
        let node = HgBlobNode::new(blob, p1, p2);
        File { node }
    }

    // (there's a use case for not providing parents, so should parents not be inside the file?)
    #[inline]
    pub fn data_only<B: Into<HgBlob>>(blob: B) -> Self {
        Self::new(blob, None, None)
    }

    // HgBlobNode should probably go away eventually, probably? So mark this private.
    #[inline]
    pub(crate) fn from_blobnode(node: HgBlobNode) -> Self {
        File { node }
    }

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

    fn parse_meta(file: &[u8]) -> HashMap<&[u8], &[u8]> {
        let (meta, _) = Self::extract_meta(file);
        let mut kv = HashMap::new();

        // Yay, Mercurial has yet another ad-hoc encoding. This one is kv pairs separated by \n,
        // with ": " separating the key and value
        for line in meta.split(|c| *c == b'\n') {
            if line.len() < 2 {
                continue;
            }

            // split on ": " - no quoting within key/value
            for idx in 0..line.len() - 1 {
                if line[idx] == b':' && line[idx + 1] == b' ' {
                    kv.insert(&line[..idx], &line[idx + 2..]);
                    break;
                }
            }
        }

        kv
    }

    pub fn copied_from(&self) -> Result<Option<(MPath, HgNodeHash)>> {
        if !self.node.maybe_copied() {
            return Ok(None);
        }
        match self.node
            .as_blob()
            .as_slice()
            .map(|buf| Self::get_copied_from(Self::parse_meta(buf)))
        {
            Some(result) => result,
            None => Ok(None),
        }
    }

    pub(crate) fn get_copied_from(
        meta: HashMap<&[u8], &[u8]>,
    ) -> Result<Option<(MPath, HgNodeHash)>> {
        let path = meta.get(COPY_PATH_KEY).cloned().map(MPath::new);
        let nodeid = meta.get(COPY_REV_KEY)
            .and_then(|rev| str::from_utf8(rev).ok())
            .and_then(|rev| rev.parse().ok());
        match (path, nodeid) {
            (Some(Ok(path)), Some(nodeid)) => Ok(Some((path, nodeid))),
            (Some(Err(e)), _) => Err(e.context("invalid path in copy metadata").into()),
            _ => Ok(None),
        }
    }

    pub fn generate_copied_from<T>(
        copy_info: Option<(MPath, HgNodeHash)>,
        buf: &mut T,
    ) -> Result<()>
    where
        T: Write,
    {
        buf.write_all(META_MARKER)?;
        match copy_info {
            None => (),
            Some((path, version)) => {
                buf.write_all(COPY_PATH_KEY)?;
                buf.write_all(b": ")?;
                path.generate(buf)?;
                buf.write_all(b"\n")?;

                buf.write_all(COPY_REV_KEY)?;
                buf.write_all(b": ")?;
                buf.write_all(version.to_hex().as_ref())?;
            }
        };
        buf.write_all(META_MARKER)?;
        Ok(())
    }

    pub fn content(&self) -> &[u8] {
        let data = self.node
            .as_blob()
            .as_slice()
            .expect("BlobNode should always have data");
        let (_, off) = Self::extract_meta(data);
        &data[off..]
    }

    pub fn file_contents(&self) -> FileContents {
        let data = self.node
            .as_blob()
            .as_inner()
            .expect("BlobNode should always have data");
        let (_, off) = Self::extract_meta(data);
        FileContents::Bytes(data.slice_from(off))
    }

    pub fn size(&self) -> usize {
        // XXX This doesn't really help because the HgBlobNode will have already been constructed
        // with the content so a size-only query will have already done too much work.
        if self.node.maybe_copied() {
            self.content().len()
        } else {
            self.node.size().expect("BlobNode should always have data")
        }
    }
}

#[cfg(test)]
mod test {
    use super::{File, META_MARKER, META_SZ};
    use mercurial_types::{HgNodeHash, MPath};

    #[test]
    fn extract_meta_sz() {
        assert_eq!(META_SZ, META_MARKER.len())
    }

    #[test]
    fn extract_meta_0() {
        const DATA: &[u8] = b"foo - no meta";

        assert_eq!(File::extract_meta(DATA), (&[][..], 0));
    }

    #[test]
    fn extract_meta_1() {
        const DATA: &[u8] = b"\x01\n\x01\nfoo - empty meta";

        assert_eq!(File::extract_meta(DATA), (&[][..], 4));
    }

    #[test]
    fn extract_meta_2() {
        const DATA: &[u8] = b"\x01\nabc\x01\nfoo - some meta";

        assert_eq!(File::extract_meta(DATA), (&b"abc"[..], 7));
    }

    #[test]
    fn extract_meta_3() {
        const DATA: &[u8] = b"\x01\nfoo - bad unterminated meta";

        assert_eq!(File::extract_meta(DATA), (&[][..], 2));
    }

    #[test]
    fn extract_meta_4() {
        const DATA: &[u8] = b"\x01\n\x01\n\x01\nfoo - bad unterminated meta";

        assert_eq!(File::extract_meta(DATA), (&[][..], 4));
    }

    #[test]
    fn extract_meta_5() {
        const DATA: &[u8] = b"\x01\n\x01\n";

        assert_eq!(File::extract_meta(DATA), (&[][..], 4));
    }

    #[test]
    fn parse_meta_0() {
        const DATA: &[u8] = b"foo - no meta";

        assert!(File::parse_meta(DATA).is_empty())
    }

    #[test]
    fn test_meta_1() {
        const DATA: &[u8] = b"\x01\n\x01\nfoo - empty meta";

        assert!(File::parse_meta(DATA).is_empty())
    }

    #[test]
    fn test_meta_2() {
        const DATA: &[u8] = b"\x01\nfoo: bar\x01\nfoo - empty meta";

        let kv: Vec<_> = File::parse_meta(DATA).into_iter().collect();

        assert_eq!(kv, vec![(b"foo".as_ref(), b"bar".as_ref())])
    }

    #[test]
    fn test_meta_3() {
        const DATA: &[u8] = b"\x01\nfoo: bar\nblim: blop: blap\x01\nfoo - empty meta";

        let mut kv: Vec<_> = File::parse_meta(DATA).into_iter().collect();
        kv.as_mut_slice().sort();

        assert_eq!(
            kv,
            vec![
                (b"blim".as_ref(), b"blop: blap".as_ref()),
                (b"foo".as_ref(), b"bar".as_ref()),
            ]
        )
    }

    quickcheck! {
        fn copy_info_roundtrip(copy_info: Option<(MPath, HgNodeHash)>) -> bool {
            let mut buf = Vec::new();
            let result = File::generate_copied_from(copy_info.clone(), &mut buf)
                .and_then(|_| {
                    File::get_copied_from(File::parse_meta(&buf))
                });
            match result {
                Ok(out_copy_info) => copy_info == out_copy_info,
                _ => {
                    false
                }
            }
        }
    }
}
