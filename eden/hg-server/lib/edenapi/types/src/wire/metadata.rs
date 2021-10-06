/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::convert::Infallible;

#[cfg(any(test, feature = "for-tests"))]
use quickcheck::Arbitrary;
use serde::{self, de::Error, Deserializer, Serializer};
use serde_derive::{Deserialize, Serialize};

use crate::{
    wire::{is_default, TryFromBytesError},
    ContentId, DirectoryMetadata, DirectoryMetadataRequest, FileMetadata, FileMetadataRequest,
    FileType, FsnodeId, Sha1, Sha256, ToApi, ToWire, WireToApiConversionError,
};

/// Directory entry metadata
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireDirectoryMetadata {
    #[serde(rename = "0", default, skip_serializing_if = "is_default")]
    fsnode_id: Option<WireFsnodeId>,

    #[serde(rename = "1", default, skip_serializing_if = "is_default")]
    simple_format_sha1: Option<WireSha1>,

    #[serde(rename = "2", default, skip_serializing_if = "is_default")]
    simple_format_sha256: Option<WireSha256>,

    #[serde(rename = "3", default, skip_serializing_if = "is_default")]
    child_files_count: Option<u64>,

    #[serde(rename = "4", default, skip_serializing_if = "is_default")]
    child_files_total_size: Option<u64>,

    #[serde(rename = "5", default, skip_serializing_if = "is_default")]
    child_dirs_count: Option<u64>,

    #[serde(rename = "6", default, skip_serializing_if = "is_default")]
    descendant_files_count: Option<u64>,

    #[serde(rename = "7", default, skip_serializing_if = "is_default")]
    descendant_files_total_size: Option<u64>,
}

impl ToWire for DirectoryMetadata {
    type Wire = WireDirectoryMetadata;

    fn to_wire(self) -> Self::Wire {
        WireDirectoryMetadata {
            fsnode_id: self.fsnode_id.to_wire(),
            simple_format_sha1: self.simple_format_sha1.to_wire(),
            simple_format_sha256: self.simple_format_sha256.to_wire(),
            child_files_count: self.child_files_count,
            child_files_total_size: self.child_files_total_size,
            child_dirs_count: self.child_dirs_count,
            descendant_files_count: self.descendant_files_count,
            descendant_files_total_size: self.descendant_files_total_size,
        }
    }
}

impl ToApi for WireDirectoryMetadata {
    type Api = DirectoryMetadata;
    type Error = Infallible;

    fn to_api(self) -> Result<Self::Api, Self::Error> {
        Ok(DirectoryMetadata {
            fsnode_id: self.fsnode_id.to_api()?,
            simple_format_sha1: self.simple_format_sha1.to_api()?,
            simple_format_sha256: self.simple_format_sha256.to_api()?,
            child_files_count: self.child_files_count,
            child_files_total_size: self.child_files_total_size,
            child_dirs_count: self.child_dirs_count,
            descendant_files_count: self.descendant_files_count,
            descendant_files_total_size: self.descendant_files_total_size,
        })
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireDirectoryMetadataRequest {
    #[serde(rename = "0", default, skip_serializing_if = "is_default")]
    with_fsnode_id: bool,

    #[serde(rename = "1", default, skip_serializing_if = "is_default")]
    with_simple_format_sha1: bool,

    #[serde(rename = "2", default, skip_serializing_if = "is_default")]
    with_simple_format_sha256: bool,

    #[serde(rename = "3", default, skip_serializing_if = "is_default")]
    with_child_files_count: bool,

    #[serde(rename = "4", default, skip_serializing_if = "is_default")]
    with_child_files_total_size: bool,

    #[serde(rename = "5", default, skip_serializing_if = "is_default")]
    with_child_dirs_count: bool,

    #[serde(rename = "6", default, skip_serializing_if = "is_default")]
    with_descendant_files_count: bool,

    #[serde(rename = "7", default, skip_serializing_if = "is_default")]
    with_descendant_files_total_size: bool,
}

impl ToWire for DirectoryMetadataRequest {
    type Wire = WireDirectoryMetadataRequest;

    fn to_wire(self) -> Self::Wire {
        WireDirectoryMetadataRequest {
            with_fsnode_id: self.with_fsnode_id,
            with_simple_format_sha1: self.with_simple_format_sha1,
            with_simple_format_sha256: self.with_simple_format_sha256,
            with_child_files_count: self.with_child_files_count,
            with_child_files_total_size: self.with_child_files_total_size,
            with_child_dirs_count: self.with_child_dirs_count,
            with_descendant_files_count: self.with_descendant_files_count,
            with_descendant_files_total_size: self.with_descendant_files_total_size,
        }
    }
}

impl ToApi for WireDirectoryMetadataRequest {
    type Api = DirectoryMetadataRequest;
    type Error = Infallible;

    fn to_api(self) -> Result<Self::Api, Self::Error> {
        Ok(DirectoryMetadataRequest {
            with_fsnode_id: self.with_fsnode_id,
            with_simple_format_sha1: self.with_simple_format_sha1,
            with_simple_format_sha256: self.with_simple_format_sha256,
            with_child_files_count: self.with_child_files_count,
            with_child_files_total_size: self.with_child_files_total_size,
            with_child_dirs_count: self.with_child_dirs_count,
            with_descendant_files_count: self.with_descendant_files_count,
            with_descendant_files_total_size: self.with_descendant_files_total_size,
        })
    }
}

/// File entry metadata
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireFileMetadata {
    #[serde(rename = "0", default, skip_serializing_if = "is_default")]
    revisionstore_flags: Option<u64>,

    #[serde(rename = "1", default, skip_serializing_if = "is_default")]
    content_id: Option<WireContentId>,

    #[serde(rename = "2", default, skip_serializing_if = "is_default")]
    file_type: Option<WireFileType>,

    #[serde(rename = "3", default, skip_serializing_if = "is_default")]
    size: Option<u64>,

    #[serde(rename = "4", default, skip_serializing_if = "is_default")]
    content_sha1: Option<WireSha1>,

    #[serde(rename = "5", default, skip_serializing_if = "is_default")]
    content_sha256: Option<WireSha256>,
}

impl ToWire for FileMetadata {
    type Wire = WireFileMetadata;

    fn to_wire(self) -> Self::Wire {
        WireFileMetadata {
            revisionstore_flags: self.revisionstore_flags,
            content_id: self.content_id.to_wire(),
            file_type: self.file_type.to_wire(),
            size: self.size,
            content_sha1: self.content_sha1.to_wire(),
            content_sha256: self.content_sha256.to_wire(),
        }
    }
}

impl ToApi for WireFileMetadata {
    type Api = FileMetadata;
    type Error = WireToApiConversionError;

    fn to_api(self) -> Result<Self::Api, Self::Error> {
        Ok(FileMetadata {
            revisionstore_flags: self.revisionstore_flags,
            content_id: self.content_id.to_api()?,
            file_type: self.file_type.to_api()?,
            size: self.size,
            content_sha1: self.content_sha1.to_api()?,
            content_sha256: self.content_sha256.to_api()?,
        })
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireFileMetadataRequest {
    #[serde(rename = "0", default, skip_serializing_if = "is_default")]
    with_revisionstore_flags: bool,

    #[serde(rename = "1", default, skip_serializing_if = "is_default")]
    with_content_id: bool,

    #[serde(rename = "2", default, skip_serializing_if = "is_default")]
    with_file_type: bool,

    #[serde(rename = "3", default, skip_serializing_if = "is_default")]
    with_size: bool,

    #[serde(rename = "4", default, skip_serializing_if = "is_default")]
    with_content_sha1: bool,

    #[serde(rename = "5", default, skip_serializing_if = "is_default")]
    with_content_sha256: bool,
}

impl ToWire for FileMetadataRequest {
    type Wire = WireFileMetadataRequest;

    fn to_wire(self) -> Self::Wire {
        WireFileMetadataRequest {
            with_revisionstore_flags: self.with_revisionstore_flags,
            with_content_id: self.with_content_id,
            with_file_type: self.with_file_type,
            with_size: self.with_size,
            with_content_sha1: self.with_content_sha1,
            with_content_sha256: self.with_content_sha256,
        }
    }
}

impl ToApi for WireFileMetadataRequest {
    type Api = FileMetadataRequest;
    type Error = Infallible;

    fn to_api(self) -> Result<Self::Api, Self::Error> {
        Ok(FileMetadataRequest {
            with_revisionstore_flags: self.with_revisionstore_flags,
            with_content_id: self.with_content_id,
            with_file_type: self.with_file_type,
            with_size: self.with_size,
            with_content_sha1: self.with_content_sha1,
            with_content_sha256: self.with_content_sha256,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum WireFileType {
    #[serde(rename = "1")]
    Regular,

    #[serde(rename = "2")]
    Executable,

    #[serde(rename = "3")]
    Symlink,

    #[serde(other, rename = "0")]
    Unknown,
}

impl ToWire for FileType {
    type Wire = WireFileType;

    fn to_wire(self) -> Self::Wire {
        use FileType::*;
        match self {
            Regular => WireFileType::Regular,
            Executable => WireFileType::Executable,
            Symlink => WireFileType::Symlink,
        }
    }
}

impl ToApi for WireFileType {
    type Api = FileType;
    type Error = WireToApiConversionError;

    fn to_api(self) -> Result<Self::Api, Self::Error> {
        use WireFileType::*;
        Ok(match self {
            Unknown => {
                return Err(WireToApiConversionError::UnrecognizedEnumVariant(
                    "WireFileType",
                ));
            }
            Regular => FileType::Regular,
            Executable => FileType::Executable,
            Symlink => FileType::Symlink,
        })
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WireFsnodeId([u8; WireFsnodeId::len()]);

impl WireFsnodeId {
    pub const fn len() -> usize {
        32
    }
}

impl ToWire for FsnodeId {
    type Wire = WireFsnodeId;

    fn to_wire(self) -> Self::Wire {
        WireFsnodeId(self.0)
    }
}

impl ToApi for WireFsnodeId {
    type Api = FsnodeId;
    type Error = Infallible;

    fn to_api(self) -> Result<Self::Api, Self::Error> {
        Ok(FsnodeId(self.0))
    }
}

impl serde::Serialize for WireFsnodeId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_bytes(&self.0)
    }
}

impl<'de> serde::Deserialize<'de> for WireFsnodeId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let bytes: serde_bytes::ByteBuf = serde_bytes::deserialize(deserializer)?;
        let bytes = bytes.as_ref();

        if bytes.len() == Self::len() {
            let mut ary = [0u8; Self::len()];
            ary.copy_from_slice(&bytes);
            Ok(WireFsnodeId(ary))
        } else {
            Err(D::Error::custom(TryFromBytesError {
                expected_len: Self::len(),
                found_len: bytes.len(),
            }))
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WireContentId([u8; WireContentId::len()]);

impl WireContentId {
    pub const fn len() -> usize {
        32
    }
}

impl ToWire for ContentId {
    type Wire = WireContentId;

    fn to_wire(self) -> Self::Wire {
        WireContentId(self.0)
    }
}

impl ToApi for WireContentId {
    type Api = ContentId;
    type Error = Infallible;

    fn to_api(self) -> Result<Self::Api, Self::Error> {
        Ok(ContentId(self.0))
    }
}

impl serde::Serialize for WireContentId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_bytes(&self.0)
    }
}

impl<'de> serde::Deserialize<'de> for WireContentId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let bytes: serde_bytes::ByteBuf = serde_bytes::deserialize(deserializer)?;
        let bytes = bytes.as_ref();

        if bytes.len() == Self::len() {
            let mut ary = [0u8; Self::len()];
            ary.copy_from_slice(&bytes);
            Ok(WireContentId(ary))
        } else {
            Err(D::Error::custom(TryFromBytesError {
                expected_len: Self::len(),
                found_len: bytes.len(),
            }))
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WireSha1([u8; WireSha1::len()]);

impl WireSha1 {
    pub const fn len() -> usize {
        20
    }
}

impl ToWire for Sha1 {
    type Wire = WireSha1;

    fn to_wire(self) -> Self::Wire {
        WireSha1(self.0)
    }
}

impl ToApi for WireSha1 {
    type Api = Sha1;
    type Error = Infallible;

    fn to_api(self) -> Result<Self::Api, Self::Error> {
        Ok(Sha1(self.0))
    }
}

impl serde::Serialize for WireSha1 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_bytes(&self.0)
    }
}

impl<'de> serde::Deserialize<'de> for WireSha1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let bytes: serde_bytes::ByteBuf = serde_bytes::deserialize(deserializer)?;
        let bytes = bytes.as_ref();

        if bytes.len() == Self::len() {
            let mut ary = [0u8; Self::len()];
            ary.copy_from_slice(&bytes);
            Ok(WireSha1(ary))
        } else {
            Err(D::Error::custom(TryFromBytesError {
                expected_len: Self::len(),
                found_len: bytes.len(),
            }))
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WireSha256([u8; WireSha256::len()]);

impl WireSha256 {
    pub const fn len() -> usize {
        32
    }
}

impl ToWire for Sha256 {
    type Wire = WireSha256;

    fn to_wire(self) -> Self::Wire {
        WireSha256(self.0)
    }
}

impl ToApi for WireSha256 {
    type Api = Sha256;
    type Error = Infallible;

    fn to_api(self) -> Result<Self::Api, Self::Error> {
        Ok(Sha256(self.0))
    }
}

impl serde::Serialize for WireSha256 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_bytes(&self.0)
    }
}

impl<'de> serde::Deserialize<'de> for WireSha256 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let bytes: serde_bytes::ByteBuf = serde_bytes::deserialize(deserializer)?;
        let bytes = bytes.as_ref();

        if bytes.len() == Self::len() {
            let mut ary = [0u8; Self::len()];
            ary.copy_from_slice(&bytes);
            Ok(WireSha256(ary))
        } else {
            Err(D::Error::custom(TryFromBytesError {
                expected_len: Self::len(),
                found_len: bytes.len(),
            }))
        }
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for WireDirectoryMetadata {
    fn arbitrary(g: &mut quickcheck::Gen) -> Self {
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
impl Arbitrary for WireDirectoryMetadataRequest {
    fn arbitrary(g: &mut quickcheck::Gen) -> Self {
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
impl Arbitrary for WireFileMetadata {
    fn arbitrary(g: &mut quickcheck::Gen) -> Self {
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
impl Arbitrary for WireFileMetadataRequest {
    fn arbitrary(g: &mut quickcheck::Gen) -> Self {
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
impl Arbitrary for WireFileType {
    fn arbitrary(g: &mut quickcheck::Gen) -> Self {
        use WireFileType::*;

        let variant = g.choose(&[0, 1, 2, 3]).unwrap();
        match variant {
            0 => Regular,
            1 => Executable,
            2 => Symlink,
            3 => Unknown,
            _ => unreachable!(),
        }
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for WireFsnodeId {
    fn arbitrary(g: &mut quickcheck::Gen) -> Self {
        let mut v = Self::default();
        for b in v.0.iter_mut() {
            *b = u8::arbitrary(g);
        }
        v
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for WireContentId {
    fn arbitrary(g: &mut quickcheck::Gen) -> Self {
        let mut v = Self::default();
        for b in v.0.iter_mut() {
            *b = u8::arbitrary(g);
        }
        v
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for WireSha1 {
    fn arbitrary(g: &mut quickcheck::Gen) -> Self {
        let mut v = Self::default();
        for b in v.0.iter_mut() {
            *b = u8::arbitrary(g);
        }
        v
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for WireSha256 {
    fn arbitrary(g: &mut quickcheck::Gen) -> Self {
        let mut v = Self::default();
        for b in v.0.iter_mut() {
            *b = u8::arbitrary(g);
        }
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::wire::tests::{check_serialize_roundtrip, check_wire_roundtrip};

    use quickcheck::quickcheck;

    quickcheck! {
        // Wire serialize roundtrips
        fn test_file_meta_roundtrip_serialize(v: WireFileMetadata) -> bool {
            check_serialize_roundtrip(v)
        }

        fn test_file_meta_req_roundtrip_serialize(v: WireFileMetadataRequest) -> bool {
            check_serialize_roundtrip(v)
        }

        // API-Wire roundtrips
        fn test_file_meta_roundtrip_wire(v: FileMetadata) -> bool {
            check_wire_roundtrip(v)
        }

        fn test_file_meta_req_roundtrip_wire(v: FileMetadataRequest) -> bool {
            check_wire_roundtrip(v)
        }
    }
}
