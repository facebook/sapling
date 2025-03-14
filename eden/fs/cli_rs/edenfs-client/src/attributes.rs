/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fmt::Display;
use std::future::Future;
use std::path::Path;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;

use anyhow::anyhow;
use anyhow::Context;
use edenfs_error::EdenFsError;
use edenfs_error::Result;
use edenfs_utils::bytes_from_path;
use thrift_types::edenfs::FileAttributes;
use thrift_types::edenfs::GetAttributesFromFilesParams;
use thrift_types::fbthrift::ThriftEnum;

use crate::client::EdenFsClient;
use crate::request_factory::RequestFactory;
use crate::request_factory::RequestParam;
use crate::request_factory::RequestResult;
use crate::types::SyncBehavior;

// YES, the following code is extremely repetitive. It's unfortunately the only way (for now). We
// could potentially use macros in the future, but that would require language feature
// 'more_qualified_paths' to be stabilized first: https://github.com/rust-lang/rust/issues/86935
// So for now, we will deal with the repetition... :(

pub enum Sha1OrError {
    Sha1(Vec<u8>),
    Error(EdenFsError),
    UnknownField(i32),
}

impl From<thrift_types::edenfs::Sha1OrError> for Sha1OrError {
    fn from(from: thrift_types::edenfs::Sha1OrError) -> Self {
        match from {
            thrift_types::edenfs::Sha1OrError::sha1(sha1) => Self::Sha1(sha1),
            thrift_types::edenfs::Sha1OrError::error(e) => {
                Self::Error(EdenFsError::ThriftRequestError(e.into()))
            }
            thrift_types::edenfs::Sha1OrError::UnknownField(i) => Self::UnknownField(i),
        }
    }
}

pub enum SizeOrError {
    Size(i64),
    Error(EdenFsError),
    UnknownField(i32),
}

impl From<thrift_types::edenfs::SizeOrError> for SizeOrError {
    fn from(from: thrift_types::edenfs::SizeOrError) -> Self {
        match from {
            thrift_types::edenfs::SizeOrError::size(size) => Self::Size(size),
            thrift_types::edenfs::SizeOrError::error(e) => {
                Self::Error(EdenFsError::ThriftRequestError(e.into()))
            }
            thrift_types::edenfs::SizeOrError::UnknownField(i) => Self::UnknownField(i),
        }
    }
}

pub enum SourceControlType {
    Tree,
    RegularFile,
    ExecutableFile,
    Symlink,
    Unknown,
}

impl From<thrift_types::edenfs::SourceControlType> for SourceControlType {
    fn from(from: thrift_types::edenfs::SourceControlType) -> Self {
        match from {
            thrift_types::edenfs::SourceControlType::TREE => Self::Tree,
            thrift_types::edenfs::SourceControlType::REGULAR_FILE => Self::RegularFile,
            thrift_types::edenfs::SourceControlType::EXECUTABLE_FILE => Self::ExecutableFile,
            thrift_types::edenfs::SourceControlType::SYMLINK => Self::Symlink,
            _ => Self::Unknown,
        }
    }
}

pub enum SourceControlTypeOrError {
    SourceControlType(SourceControlType),
    Error(EdenFsError),
    UnknownField(i32),
}

impl From<thrift_types::edenfs::SourceControlTypeOrError> for SourceControlTypeOrError {
    fn from(from: thrift_types::edenfs::SourceControlTypeOrError) -> Self {
        match from {
            thrift_types::edenfs::SourceControlTypeOrError::sourceControlType(scm_type) => {
                Self::SourceControlType(scm_type.into())
            }
            thrift_types::edenfs::SourceControlTypeOrError::error(e) => {
                Self::Error(EdenFsError::ThriftRequestError(e.into()))
            }
            thrift_types::edenfs::SourceControlTypeOrError::UnknownField(i) => {
                Self::UnknownField(i)
            }
        }
    }
}

pub enum ObjectIdOrError {
    ObjectId(Vec<u8>),
    Error(EdenFsError),
    UnknownField(i32),
}

impl From<thrift_types::edenfs::ObjectIdOrError> for ObjectIdOrError {
    fn from(from: thrift_types::edenfs::ObjectIdOrError) -> Self {
        match from {
            thrift_types::edenfs::ObjectIdOrError::objectId(size) => Self::ObjectId(size),
            thrift_types::edenfs::ObjectIdOrError::error(e) => {
                Self::Error(EdenFsError::ThriftRequestError(e.into()))
            }
            thrift_types::edenfs::ObjectIdOrError::UnknownField(i) => Self::UnknownField(i),
        }
    }
}

pub enum Blake3OrError {
    Blake3(Vec<u8>),
    Error(EdenFsError),
    UnknownField(i32),
}

impl From<thrift_types::edenfs::Blake3OrError> for Blake3OrError {
    fn from(from: thrift_types::edenfs::Blake3OrError) -> Self {
        match from {
            thrift_types::edenfs::Blake3OrError::blake3(size) => Self::Blake3(size),
            thrift_types::edenfs::Blake3OrError::error(e) => {
                Self::Error(EdenFsError::ThriftRequestError(e.into()))
            }
            thrift_types::edenfs::Blake3OrError::UnknownField(i) => Self::UnknownField(i),
        }
    }
}

pub enum DigestHashOrError {
    DigestHash(Vec<u8>),
    Error(EdenFsError),
    UnknownField(i32),
}

impl From<thrift_types::edenfs::DigestHashOrError> for DigestHashOrError {
    fn from(from: thrift_types::edenfs::DigestHashOrError) -> Self {
        match from {
            thrift_types::edenfs::DigestHashOrError::digestHash(size) => Self::DigestHash(size),
            thrift_types::edenfs::DigestHashOrError::error(e) => {
                Self::Error(EdenFsError::ThriftRequestError(e.into()))
            }
            thrift_types::edenfs::DigestHashOrError::UnknownField(i) => Self::UnknownField(i),
        }
    }
}

pub enum DigestSizeOrError {
    DigestSize(i64),
    Error(EdenFsError),
    UnknownField(i32),
}

impl From<thrift_types::edenfs::DigestSizeOrError> for DigestSizeOrError {
    fn from(from: thrift_types::edenfs::DigestSizeOrError) -> Self {
        match from {
            thrift_types::edenfs::DigestSizeOrError::digestSize(size) => Self::DigestSize(size),
            thrift_types::edenfs::DigestSizeOrError::error(e) => {
                Self::Error(EdenFsError::ThriftRequestError(e.into()))
            }
            thrift_types::edenfs::DigestSizeOrError::UnknownField(i) => Self::UnknownField(i),
        }
    }
}

pub struct FileAttributeDataV2 {
    pub sha1: Option<Sha1OrError>,
    pub size: Option<SizeOrError>,
    pub scm_type: Option<SourceControlTypeOrError>,
    pub object_id: Option<ObjectIdOrError>,
    pub blake3: Option<Blake3OrError>,
    pub digest_size: Option<DigestSizeOrError>,
    pub digest_hash: Option<DigestHashOrError>,
}

impl From<thrift_types::edenfs::FileAttributeDataV2> for FileAttributeDataV2 {
    fn from(from: thrift_types::edenfs::FileAttributeDataV2) -> Self {
        Self {
            sha1: from.sha1.map(Into::into),
            size: from.size.map(Into::into),
            scm_type: from.sourceControlType.map(Into::into),
            object_id: from.objectId.map(Into::into),
            blake3: from.blake3.map(Into::into),
            digest_size: from.digestSize.map(Into::into),
            digest_hash: from.digestHash.map(Into::into),
        }
    }
}

pub enum FileAttributeDataOrErrorV2 {
    FileAttributeData(FileAttributeDataV2),
    Error(EdenFsError),
    UnknownField(i32),
}

impl From<thrift_types::edenfs::FileAttributeDataOrErrorV2> for FileAttributeDataOrErrorV2 {
    fn from(from: thrift_types::edenfs::FileAttributeDataOrErrorV2) -> Self {
        match from {
            thrift_types::edenfs::FileAttributeDataOrErrorV2::fileAttributeData(data) => {
                Self::FileAttributeData(data.into())
            }
            thrift_types::edenfs::FileAttributeDataOrErrorV2::error(e) => {
                Self::Error(EdenFsError::ThriftRequestError(e.into()))
            }
            thrift_types::edenfs::FileAttributeDataOrErrorV2::UnknownField(i) => {
                Self::UnknownField(i)
            }
        }
    }
}

pub struct GetAttributesFromFilesResultV2 {
    pub res: Vec<FileAttributeDataOrErrorV2>,
}

impl From<thrift_types::edenfs::GetAttributesFromFilesResultV2> for GetAttributesFromFilesResultV2 {
    fn from(from: thrift_types::edenfs::GetAttributesFromFilesResultV2) -> Self {
        Self {
            res: from.res.into_iter().map(Into::into).collect(),
        }
    }
}

pub enum AttributesRequestScope {
    FilesOnly,
    TreesOnly,
    TreesAndFiles,
}

impl From<thrift_types::edenfs::AttributesRequestScope> for AttributesRequestScope {
    fn from(from: thrift_types::edenfs::AttributesRequestScope) -> Self {
        match from {
            thrift_types::edenfs::AttributesRequestScope::FILES => Self::FilesOnly,
            thrift_types::edenfs::AttributesRequestScope::TREES => Self::TreesOnly,
            thrift_types::edenfs::AttributesRequestScope::TREES_AND_FILES => Self::TreesAndFiles,
            _ => Self::TreesAndFiles,
        }
    }
}

impl From<AttributesRequestScope> for thrift_types::edenfs::AttributesRequestScope {
    fn from(from: AttributesRequestScope) -> Self {
        match from {
            AttributesRequestScope::FilesOnly => Self::FILES,
            AttributesRequestScope::TreesOnly => Self::TREES,
            AttributesRequestScope::TreesAndFiles => Self::TREES_AND_FILES,
        }
    }
}

impl Default for AttributesRequestScope {
    fn default() -> Self {
        Self::TreesAndFiles
    }
}

fn attributes_as_bitmask(attrs: &[FileAttributes]) -> i64 {
    attrs.iter().fold(0, |acc, x| acc | x.inner_value() as i64)
}

pub fn all_attributes_as_bitmask() -> i64 {
    attributes_as_bitmask(FileAttributes::variant_values())
}

pub fn all_attributes() -> &'static [&'static str] {
    FileAttributes::variants()
}

pub fn file_attributes_from_strings<T>(attrs: &[T]) -> Result<i64>
where
    T: AsRef<str> + Display,
{
    let attrs: Result<Vec<FileAttributes>, _> = attrs
        .iter()
        .map(|attr| {
            FileAttributes::from_str(attr.as_ref())
                .map_err(|e| EdenFsError::Other(anyhow!("invalid file attribute: {:?}", e)))
        })
        .collect();
    Ok(attributes_as_bitmask(attrs?.as_slice()))
}

impl EdenFsClient {
    async fn get_attributes_from_files_v2_from_params(
        &self,
        params: &thrift_types::edenfs::GetAttributesFromFilesParams,
    ) -> Result<GetAttributesFromFilesResultV2> {
        self.with_thrift(|thrift| thrift.getAttributesFromFilesV2(params))
            .await
            .map_err(|e| {
                EdenFsError::Other(anyhow!(
                    "failed to get getAttributesFromFilesV2 result: {:?}",
                    e
                ))
            })
            .map(Into::into)
    }

    pub async fn get_attributes_from_files_v2<P: AsRef<Path>>(
        &self,
        mount_point: P,
        requested_attributes: i64,
        paths: Vec<String>,
        sync: Option<SyncBehavior>,
        scope: Option<AttributesRequestScope>,
    ) -> Result<GetAttributesFromFilesResultV2> {
        let params = thrift_types::edenfs::GetAttributesFromFilesParams {
            mountPoint: bytes_from_path(mount_point.as_ref().to_path_buf())?,
            requestedAttributes: requested_attributes,
            paths: paths.iter().map(|s| s.as_bytes().to_vec()).collect(),
            sync: sync.map(Into::into).unwrap_or_default(),
            scope: scope.map(Into::into),
            ..Default::default()
        };
        self.get_attributes_from_files_v2_from_params(&params).await
    }
}

pub struct GetAttributesV2Request {
    get_attrs_params: GetAttributesFromFilesParams,
}

impl GetAttributesV2Request {
    pub fn new(
        mount_path: PathBuf,
        paths: Vec<Vec<u8>>,
        requested_attributes: &[String],
    ) -> Result<Self> {
        Ok(Self {
            get_attrs_params: GetAttributesFromFilesParams {
                mountPoint: bytes_from_path(mount_path)?,
                paths,
                requestedAttributes: file_attributes_from_strings(requested_attributes)?,
                sync: SyncBehavior::no_sync().into(),
                scope: Some(AttributesRequestScope::default().into()),
                ..Default::default()
            },
        })
    }
}

impl RequestFactory for GetAttributesV2Request {
    fn make_request(&self) -> impl FnOnce(RequestParam) -> RequestResult {
        let get_attrs_params = self.get_attrs_params.clone();
        move |client: Box<Arc<EdenFsClient>>| {
            Box::new(async move {
                match client
                    .get_attributes_from_files_v2_from_params(&get_attrs_params)
                    .await
                {
                    Ok(_) => Ok(()),
                    Err(e) => Err(e),
                }
            })
        }
    }

    fn request_name(&self) -> &'static str {
        "getAttributesFromFilesV2"
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_attributes_from_strings() -> Result<()> {
        assert_eq!(file_attributes_from_strings::<String>(&[])?, 0);
        assert_eq!(
            file_attributes_from_strings(&["SHA1_HASH", "SOURCE_CONTROL_TYPE"])?,
            FileAttributes::SHA1_HASH.inner_value() as i64
                | FileAttributes::SOURCE_CONTROL_TYPE.inner_value() as i64
        );
        assert!(file_attributes_from_strings(&["INVALID"]).is_err());
        Ok(())
    }
}
