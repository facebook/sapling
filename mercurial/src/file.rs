// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::collections::HashMap;
use std::str;

use errors::*;

use itertools::Itertools;

use mercurial_types::{BlobNode, NodeHash, Path};

#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct File {
    node: BlobNode,
}

const META_MARKER: &[u8] = b"\x01\n";
const META_SZ: usize = 2;

impl File {
    pub fn new(node: BlobNode) -> File {
        File { node: node }
    }

    fn extract_meta(file: &[u8]) -> (&[u8], usize) {
        if &file[..META_SZ] != META_MARKER {
            (&[], 0)
        } else {
            let metasz = &file[META_SZ..]
                .iter()
                .enumerate()
                .tuple_windows()
                .find(|&((_, a), (_, b))| {
                    *a == META_MARKER[0] && *b == META_MARKER[1]
                })
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

    pub fn copied_from(&self) -> Result<Option<(Path, NodeHash)>> {
        if !self.node.maybe_copied() {
            return Ok(None);
        }

        let meta = self.node.as_blob().as_slice().map(Self::parse_meta);
        let ret = meta.and_then(|meta| {
            let path = meta.get(b"copy".as_ref()).cloned().map(Path::new);
            let nodeid = meta.get(b"copyrev".as_ref())
                .and_then(|rev| str::from_utf8(rev).ok())
                .and_then(|rev| rev.parse().ok());

            if let (Some(path), Some(nodeid)) = (path, nodeid) {
                Some((path, nodeid))
            } else {
                None
            }
        });

        match ret {
            Some((Ok(path), nodeid)) => Ok(Some((path, nodeid))),
            Some((Err(e), _nodeid)) => Err(e).chain_err(|| "invalid path in copy metadata"),
            None => Ok(None),
        }
    }

    pub fn content(&self) -> Option<&[u8]> {
        self.node.as_blob().as_slice().map(|s| {
            let (_, off) = Self::extract_meta(s);
            &s[off..]
        })
    }

    pub fn size(&self) -> Option<usize> {
        // XXX This doesn't really help because the BlobNode will have already been constructed
        // with the content so a size-only query will have already done too much work.
        if self.node.maybe_copied() {
            self.content().map(|s| s.len())
        } else {
            self.node.size()
        }
    }
}

#[cfg(test)]
mod test {
    use super::{File, META_MARKER, META_SZ};

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
}
