/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::ServerError;

use faster_hex::hex_decode;
use std::fmt;
use std::str::FromStr;

#[cfg(any(test, feature = "for-tests"))]
use quickcheck::Arbitrary;
use serde_derive::{Deserialize, Serialize};

pub const SHA1_HASH_LENGTH_BYTES: usize = 20;
pub const SHA1_HASH_LENGTH_HEX: usize = SHA1_HASH_LENGTH_BYTES * 2;

pub const SHA256_HASH_LENGTH_BYTES: usize = 32;
pub const SHA256_HASH_LENGTH_HEX: usize = SHA256_HASH_LENGTH_BYTES * 2;

pub const CONTENT_ID_HASH_LENGTH_BYTES: usize = 32;
pub const CONTENT_ID_HASH_LENGTH_HEX: usize = CONTENT_ID_HASH_LENGTH_BYTES * 2;

/// Directory entry metadata
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DirectoryMetadata {
    pub fsnode_id: Option<FsnodeId>,
    pub simple_format_sha1: Option<Sha1>,
    pub simple_format_sha256: Option<Sha256>,
    pub child_files_count: Option<u64>,
    pub child_files_total_size: Option<u64>,
    pub child_dirs_count: Option<u64>,
    pub descendant_files_count: Option<u64>,
    pub descendant_files_total_size: Option<u64>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DirectoryMetadataRequest {
    pub with_fsnode_id: bool,
    pub with_simple_format_sha1: bool,
    pub with_simple_format_sha256: bool,
    pub with_child_files_count: bool,
    pub with_child_files_total_size: bool,
    pub with_child_dirs_count: bool,
    pub with_descendant_files_count: bool,
    pub with_descendant_files_total_size: bool,
}

/// File entry metadata
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FileMetadata {
    pub revisionstore_flags: Option<u64>,
    pub content_id: Option<ContentId>,
    pub file_type: Option<FileType>,
    pub size: Option<u64>,
    pub content_sha1: Option<Sha1>,
    pub content_sha256: Option<Sha256>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileMetadataRequest {
    pub with_revisionstore_flags: bool,
    pub with_content_id: bool,
    pub with_file_type: bool,
    pub with_size: bool,
    pub with_content_sha1: bool,
    pub with_content_sha256: bool,
}

#[derive(Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sha1(pub [u8; SHA1_HASH_LENGTH_BYTES]);

impl From<[u8; SHA1_HASH_LENGTH_BYTES]> for Sha1 {
    fn from(v: [u8; SHA1_HASH_LENGTH_BYTES]) -> Self {
        Sha1(v)
    }
}

impl AsRef<[u8]> for Sha1 {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl fmt::Display for Sha1 {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        for d in &self.0 {
            write!(fmt, "{:02x}", d)?;
        }
        Ok(())
    }
}

impl fmt::Debug for Sha1 {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        write!(fmt, "Sha1(\"{}\")", self)
    }
}

impl FromStr for Sha1 {
    type Err = ServerError;

    fn from_str(s: &str) -> Result<Sha1, Self::Err> {
        if s.len() != SHA1_HASH_LENGTH_HEX {
            return Err(Self::Err::generic(format!(
                "sha1 parsing failure: need exactly {} hex digits",
                SHA1_HASH_LENGTH_HEX
            )));
        }
        let mut ret = Sha1([0; SHA1_HASH_LENGTH_BYTES]);
        match hex_decode(s.as_bytes(), &mut ret.0) {
            Ok(_) => Ok(ret),
            Err(_) => Err(Self::Err::generic(
                "sha1 parsing failure: bad hex character",
            )),
        }
    }
}

#[derive(Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sha256(pub [u8; SHA256_HASH_LENGTH_BYTES]);

impl From<[u8; SHA256_HASH_LENGTH_BYTES]> for Sha256 {
    fn from(v: [u8; SHA256_HASH_LENGTH_BYTES]) -> Self {
        Sha256(v)
    }
}

impl AsRef<[u8]> for Sha256 {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl fmt::Display for Sha256 {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        for d in &self.0 {
            write!(fmt, "{:02x}", d)?;
        }
        Ok(())
    }
}

impl fmt::Debug for Sha256 {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        write!(fmt, "Sha256(\"{}\")", self)
    }
}

impl FromStr for Sha256 {
    type Err = ServerError;

    fn from_str(s: &str) -> Result<Sha256, Self::Err> {
        if s.len() != SHA256_HASH_LENGTH_HEX {
            return Err(Self::Err::generic(format!(
                "sha256 parsing failure: need exactly {} hex digits",
                SHA256_HASH_LENGTH_HEX
            )));
        }
        let mut ret = Sha256([0; SHA256_HASH_LENGTH_BYTES]);
        match hex_decode(s.as_bytes(), &mut ret.0) {
            Ok(_) => Ok(ret),
            Err(_) => Err(Self::Err::generic(
                "sha256 parsing failure: bad hex character",
            )),
        }
    }
}

#[derive(
    Clone,
    Copy,
    Default,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Serialize,
    Deserialize
)]
pub struct ContentId(pub [u8; CONTENT_ID_HASH_LENGTH_BYTES]);

impl From<[u8; CONTENT_ID_HASH_LENGTH_BYTES]> for ContentId {
    fn from(v: [u8; CONTENT_ID_HASH_LENGTH_BYTES]) -> Self {
        ContentId(v)
    }
}

impl AsRef<[u8]> for ContentId {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl fmt::Display for ContentId {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        for d in &self.0 {
            write!(fmt, "{:02x}", d)?;
        }
        Ok(())
    }
}

impl fmt::Debug for ContentId {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        write!(fmt, "ContentId(\"{}\")", self)
    }
}

impl FromStr for ContentId {
    type Err = ServerError;

    fn from_str(s: &str) -> Result<ContentId, Self::Err> {
        if s.len() != CONTENT_ID_HASH_LENGTH_HEX {
            return Err(Self::Err::generic(format!(
                "content_id parsing failure: need exactly {} hex digits",
                CONTENT_ID_HASH_LENGTH_HEX
            )));
        }
        let mut ret = ContentId([0; CONTENT_ID_HASH_LENGTH_BYTES]);
        match hex_decode(s.as_bytes(), &mut ret.0) {
            Ok(_) => Ok(ret),
            Err(_) => Err(Self::Err::generic(
                "content_id parsing failure: bad hex character",
            )),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FileType {
    Regular,
    Executable,
    Symlink,
}

#[derive(Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FsnodeId(pub [u8; 32]);

impl fmt::Display for FsnodeId {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        for d in &self.0 {
            write!(fmt, "{:02x}", d)?;
        }
        Ok(())
    }
}

impl fmt::Debug for FsnodeId {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        write!(fmt, "FsnodeId(\"{}\")", self)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AnyFileContentId {
    ContentId(ContentId),
    Sha1(Sha1),
    Sha256(Sha256),
}

impl Default for AnyFileContentId {
    fn default() -> Self {
        AnyFileContentId::ContentId(ContentId::default())
    }
}

impl FromStr for AnyFileContentId {
    type Err = ServerError;

    fn from_str(s: &str) -> Result<AnyFileContentId, Self::Err> {
        let v: Vec<&str> = s.split('/').collect();
        if v.len() != 2 {
            return Err(Self::Err::generic(
                "AnyFileContentId parsing failure: format is 'idtype/id'",
            ));
        }
        let idtype = v[0];
        let id = v[1];
        let any_file_content_id = match idtype {
            "content_id" => AnyFileContentId::ContentId(ContentId::from_str(id)?),
            "sha1" => AnyFileContentId::Sha1(Sha1::from_str(id)?),
            "sha256" => AnyFileContentId::Sha256(Sha256::from_str(id)?),
            _ => {
                return Err(Self::Err::generic(
                    "AnyFileContentId parsing failure: supported id types are: 'content_id', 'sha1' and 'sha256'",
                ));
            }
        };
        Ok(any_file_content_id)
    }
}

impl fmt::Display for AnyFileContentId {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            AnyFileContentId::ContentId(id) => write!(f, "{}", id),
            AnyFileContentId::Sha1(id) => write!(f, "{}", id),
            AnyFileContentId::Sha256(id) => write!(f, "{}", id),
        }
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for DirectoryMetadata {
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> Self {
        Self {
            fsnode_id: Arbitrary::arbitrary(g),
            simple_format_sha1: Arbitrary::arbitrary(g),
            simple_format_sha256: Arbitrary::arbitrary(g),
            child_files_count: Arbitrary::arbitrary(g),
            child_files_total_size: Arbitrary::arbitrary(g),
            child_dirs_count: Arbitrary::arbitrary(g),
            descendant_files_count: Arbitrary::arbitrary(g),
            descendant_files_total_size: Arbitrary::arbitrary(g),
        }
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for DirectoryMetadataRequest {
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> Self {
        Self {
            with_fsnode_id: Arbitrary::arbitrary(g),
            with_simple_format_sha1: Arbitrary::arbitrary(g),
            with_simple_format_sha256: Arbitrary::arbitrary(g),
            with_child_files_count: Arbitrary::arbitrary(g),
            with_child_files_total_size: Arbitrary::arbitrary(g),
            with_child_dirs_count: Arbitrary::arbitrary(g),
            with_descendant_files_count: Arbitrary::arbitrary(g),
            with_descendant_files_total_size: Arbitrary::arbitrary(g),
        }
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for FileMetadata {
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> Self {
        Self {
            revisionstore_flags: Arbitrary::arbitrary(g),
            content_id: Arbitrary::arbitrary(g),
            file_type: Arbitrary::arbitrary(g),
            size: Arbitrary::arbitrary(g),
            content_sha1: Arbitrary::arbitrary(g),
            content_sha256: Arbitrary::arbitrary(g),
        }
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for FileMetadataRequest {
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> Self {
        Self {
            with_revisionstore_flags: Arbitrary::arbitrary(g),
            with_content_id: Arbitrary::arbitrary(g),
            with_file_type: Arbitrary::arbitrary(g),
            with_size: Arbitrary::arbitrary(g),
            with_content_sha1: Arbitrary::arbitrary(g),
            with_content_sha256: Arbitrary::arbitrary(g),
        }
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for FileType {
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> Self {
        use rand::Rng;
        use FileType::*;

        let variant = g.gen_range(0, 3);
        match variant {
            0 => Regular,
            1 => Executable,
            2 => Symlink,
            _ => unreachable!(),
        }
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for FsnodeId {
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> Self {
        let mut v = Self::default();
        g.fill_bytes(&mut v.0);
        v
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for ContentId {
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> Self {
        let mut v = Self::default();
        g.fill_bytes(&mut v.0);
        v
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for Sha1 {
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> Self {
        let mut v = Self::default();
        g.fill_bytes(&mut v.0);
        v
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for Sha256 {
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> Self {
        let mut v = Self::default();
        g.fill_bytes(&mut v.0);
        v
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for AnyFileContentId {
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> Self {
        use rand::Rng;
        use AnyFileContentId::*;

        let variant = g.gen_range(0, 3);
        match variant {
            0 => ContentId(Arbitrary::arbitrary(g)),
            1 => Sha1(Arbitrary::arbitrary(g)),
            2 => Sha256(Arbitrary::arbitrary(g)),
            _ => unreachable!(),
        }
    }
}
