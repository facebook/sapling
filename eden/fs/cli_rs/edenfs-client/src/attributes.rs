/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::Path;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;

use anyhow::anyhow;
use edenfs_error::EdenDataIntoEdenFsResult;
use edenfs_error::EdenFsError;
use edenfs_error::Result;
use edenfs_error::impl_eden_data_into_edenfs_result;
use edenfs_utils::bytes_from_path;

use crate::client::Client;
use crate::client::EdenFsClient;
use crate::methods::EdenThriftMethod;
use crate::request_factory::RequestFactory;
use crate::request_factory::RequestParam;
use crate::request_factory::RequestResult;
use crate::types::FileAttributes;
use crate::types::SyncBehavior;
use crate::types::TryIntoFileAttributeBitmask;

// YES, the following code is extremely repetitive. It's unfortunately the only way (for now). We
// could potentially use macros in the future, but that would require language feature
// 'more_qualified_paths' to be stabilized first: https://github.com/rust-lang/rust/issues/86935
// So for now, we will deal with the repetition... :(

/// Represents either a SHA1 hash or an error.
///
/// This enum is used to represent the result of a SHA1 hash request, which can
/// either be a successful hash value or an error.
#[derive(Debug)]
pub enum Sha1OrError {
    /// A successful SHA1 hash value.
    ///
    /// The hash is represented as a vector of bytes.
    Sha1(Vec<u8>),
    /// An error occurred during the request.
    Error(EdenFsError),
    /// The request included an unknown field.
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

/// Represents either a file size or an error.
///
/// This enum is used to represent the result of a file size request, which can
/// either be a successful size value or an error.
#[derive(Debug)]
pub enum SizeOrError {
    /// A successful file size value.
    Size(i64),
    /// An error occurred during the request.
    Error(EdenFsError),
    /// The request included an unknown field.
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

/// Represents the type of an object in source control.
///
/// This enum is used to represent the type of an object in source control,
/// such as a tree, regular file, executable file, or symlink.
#[derive(Debug, Clone)]
pub enum SourceControlType {
    /// A directory (tree) in source control.
    Tree,
    /// A regular file in source control.
    RegularFile,
    /// An executable file in source control.
    ExecutableFile,
    /// A symlink in source control.
    Symlink,
    /// An unknown or unsupported type.
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

/// Represents either a source control type or an error.
///
/// This enum is used to represent the result of a source control type request,
/// which can either be a successful type value or an error.
#[derive(Debug)]
pub enum SourceControlTypeOrError {
    /// A successful source control type value.
    SourceControlType(SourceControlType),
    /// An error occurred during the request.
    Error(EdenFsError),
    /// The request included an unknown field.
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

/// Represents either an object ID or an error.
///
/// This enum is used to represent the result of an object ID request, which can
/// either be a successful ID value or an error.
#[derive(Debug)]
pub enum ObjectIdOrError {
    /// A successful object ID value.
    ///
    /// The ID is represented as a vector of bytes.
    ObjectId(Vec<u8>),
    /// An error occurred during the request.
    Error(EdenFsError),
    /// The request included an unknown field.
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

/// Represents either a BLAKE3 hash or an error.
///
/// This enum is used to represent the result of a BLAKE3 hash request, which can
/// either be a successful hash value or an error.
#[derive(Debug)]
pub enum Blake3OrError {
    /// A successful BLAKE3 hash value.
    ///
    /// The hash is represented as a vector of bytes.
    Blake3(Vec<u8>),
    /// An error occurred during the request.
    Error(EdenFsError),
    /// The request included an unknown field.
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

/// Represents either a digest hash or an error.
///
/// This enum is used to represent the result of a digest hash request, which can
/// either be a successful hash value or an error.
#[derive(Debug)]
pub enum DigestHashOrError {
    /// A successful digest hash value.
    ///
    /// The hash is represented as a vector of bytes.
    DigestHash(Vec<u8>),
    /// An error occurred during the request.
    Error(EdenFsError),
    /// The request included an unknown field.
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

/// Represents either a digest size or an error.
///
/// This enum is used to represent the result of a digest size request, which can
/// either be a successful size value or an error.
#[derive(Debug)]
pub enum DigestSizeOrError {
    /// A successful digest size value.
    DigestSize(i64),
    /// An error occurred during the request.
    Error(EdenFsError),
    /// The request included an unknown field.
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

/// Contains file attribute data for a file.
///
/// This struct contains various attributes of a file, such as its SHA1 hash,
/// size, source control type, and more. Each attribute is optional and may
/// contain either a value or an error.
#[derive(Debug)]
pub struct FileAttributeDataV2 {
    /// The SHA1 hash of the file, if requested and available.
    pub sha1: Option<Sha1OrError>,
    /// The size of the file, if requested and available.
    pub size: Option<SizeOrError>,
    /// The source control type of the file, if requested and available.
    pub scm_type: Option<SourceControlTypeOrError>,
    /// The object ID of the file, if requested and available.
    pub object_id: Option<ObjectIdOrError>,
    /// The BLAKE3 hash of the file, if requested and available.
    pub blake3: Option<Blake3OrError>,
    /// The digest size of the file, if requested and available.
    pub digest_size: Option<DigestSizeOrError>,
    /// The digest hash of the file, if requested and available.
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

/// Represents either file attribute data or an error.
///
/// This enum is used to represent the result of a file attribute data request,
/// which can either be successful data or an error.
#[derive(Debug)]
pub enum FileAttributeDataOrErrorV2 {
    /// Successful file attribute data.
    FileAttributeData(FileAttributeDataV2),
    /// An error occurred during the request.
    Error(EdenFsError),
    /// The request included an unknown field.
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

impl_eden_data_into_edenfs_result!(Sha1OrError, Vec<u8>, Sha1);
impl_eden_data_into_edenfs_result!(SizeOrError, i64, Size);
impl_eden_data_into_edenfs_result!(
    SourceControlTypeOrError,
    SourceControlType,
    SourceControlType
);
impl_eden_data_into_edenfs_result!(ObjectIdOrError, Vec<u8>, ObjectId);
impl_eden_data_into_edenfs_result!(Blake3OrError, Vec<u8>, Blake3);
impl_eden_data_into_edenfs_result!(DigestSizeOrError, i64, DigestSize);
impl_eden_data_into_edenfs_result!(DigestHashOrError, Vec<u8>, DigestHash);
impl_eden_data_into_edenfs_result!(
    FileAttributeDataOrErrorV2,
    FileAttributeDataV2,
    FileAttributeData
);

/// Contains the results of a get attributes from files request.
///
/// This struct contains a vector of results, one for each file in the request.
/// Each result can be either successful file attribute data or an error.
pub struct GetAttributesFromFilesResultV2 {
    /// The results of the request, one for each file.
    pub res: Vec<FileAttributeDataOrErrorV2>,
}

impl From<thrift_types::edenfs::GetAttributesFromFilesResultV2> for GetAttributesFromFilesResultV2 {
    fn from(from: thrift_types::edenfs::GetAttributesFromFilesResultV2) -> Self {
        Self {
            res: from.res.into_iter().map(Into::into).collect(),
        }
    }
}

/// Specifies the scope of an attributes request.
///
/// This enum is used to specify whether an attributes request should include
/// files, trees (directories), or both.
///
/// # Examples
///
/// ```
/// use edenfs_client::attributes::AttributesRequestScope;
///
/// // Request attributes for files only
/// let scope = AttributesRequestScope::FilesOnly;
///
/// // Request attributes for trees (directories) only
/// let scope = AttributesRequestScope::TreesOnly;
///
/// // Request attributes for both files and trees
/// let scope = AttributesRequestScope::TreesAndFiles;
///
/// // Default scope is TreesAndFiles
/// let default_scope = AttributesRequestScope::default();
/// assert!(matches!(
///     default_scope,
///     AttributesRequestScope::TreesAndFiles
/// ));
/// ```
#[repr(i32)]
#[derive(Debug, Clone)]
pub enum AttributesRequestScope {
    /// Request attributes for files only.
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

impl FromStr for AttributesRequestScope {
    type Err = EdenFsError;
    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "filesonly" => Ok(Self::FilesOnly),
            "treesonly" => Ok(Self::TreesOnly),
            "treesandfiles" => Ok(Self::TreesAndFiles),
            _ => Err(EdenFsError::Other(anyhow!(
                "invalid file attribute request scope: {:?}",
                s
            ))),
        }
    }
}

impl Default for AttributesRequestScope {
    fn default() -> Self {
        Self::TreesAndFiles
    }
}

impl EdenFsClient {
    async fn get_attributes_from_files_v2<P, S, A>(
        &self,
        mount_point: P,
        paths: &[S],
        requested_attributes: A,
        sync: Option<SyncBehavior>,
        scope: Option<AttributesRequestScope>,
    ) -> Result<GetAttributesFromFilesResultV2>
    where
        P: AsRef<Path>,
        S: AsRef<str>,
        A: TryIntoFileAttributeBitmask,
    {
        let params = thrift_types::edenfs::GetAttributesFromFilesParams {
            mountPoint: bytes_from_path(mount_point.as_ref().to_path_buf())?,
            requestedAttributes: requested_attributes.try_into_bitmask()?,
            paths: paths
                .iter()
                .map(|s| s.as_ref().as_bytes().to_vec())
                .collect(),
            sync: sync.map(Into::into).unwrap_or_default(),
            scope: scope.map(Into::into),
            ..Default::default()
        };
        self.with_thrift(|thrift| {
            (
                thrift.getAttributesFromFilesV2(&params),
                EdenThriftMethod::GetAttributesFromFilesV2,
            )
        })
        .await
        .map_err(|e| {
            EdenFsError::Other(anyhow!(
                "failed to get getAttributesFromFilesV2 result: {:?}",
                e
            ))
        })
        .map(Into::into)
    }
}

/// A request factory for getting file attributes.
///
/// This struct is used to create a request for getting file attributes from EdenFS.
/// It implements the `RequestFactory` trait, which allows it to be used with the
/// request execution framework.
pub struct GetAttributesV2Request {
    mount_point: PathBuf,
    paths: Vec<String>,
    requested_attributes: i64,
    request_scope: Option<AttributesRequestScope>,
}

impl GetAttributesV2Request {
    /// Creates a new `GetAttributesV2Request`.
    ///
    /// This method creates a new request for getting file attributes from EdenFS.
    ///
    /// # Parameters
    ///
    /// * `mount_path` - The path to the EdenFS mount point.
    /// * `paths` - A slice of paths to get attributes for, relative to the mount point.
    /// * `requested_attributes` - A slice of attribute names to request.
    ///
    /// # Returns
    ///
    /// A new `GetAttributesV2Request` instance.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::path::PathBuf;
    ///
    /// use edenfs_client::attributes::AttributesRequestScope;
    /// use edenfs_client::attributes::GetAttributesV2Request;
    /// use edenfs_client::types::FileAttributes;
    ///
    /// // Create a request for getting SHA1 and size attributes for two files
    /// let mount_path = PathBuf::from("/path/to/mount");
    /// let paths = ["file1.txt", "file2.txt"];
    /// let attrs = [FileAttributes::Sha1, FileAttributes::FileSize];
    /// let scope = AttributesRequestScope::TreesAndFiles;
    /// let request = GetAttributesV2Request::new(mount_path, &paths, attrs.as_slice(), Some(scope));
    /// ```
    pub fn new<P, A>(
        mount_path: PathBuf,
        paths: &[P],
        requested_attributes: A,
        request_scope: Option<AttributesRequestScope>,
    ) -> Self
    where
        P: AsRef<str>,
        A: TryIntoFileAttributeBitmask,
    {
        Self {
            mount_point: mount_path,
            paths: paths.iter().map(|p| p.as_ref().into()).collect(),
            requested_attributes: requested_attributes.try_into_bitmask().unwrap_or_else(|e| {
                tracing::error!("failed to convert attributes to bitmap: {:?}", e);
                tracing::info!(
                    "defaulting to requesting all attributes in getAttributesFromFilesV2 requests"
                );
                FileAttributes::all_attributes_as_bitmask()
            }),
            request_scope,
        }
    }
}

impl RequestFactory for GetAttributesV2Request {
    fn make_request(&self) -> impl FnOnce(RequestParam) -> RequestResult {
        let mount_point = self.mount_point.clone();
        let paths = self.paths.clone();
        let requested_attributes = self.requested_attributes;
        let request_scope = self.request_scope.clone();
        move |client: Box<Arc<EdenFsClient>>| {
            Box::new(async move {
                // Required to ensure the lifetime of paths extends for the duration of the lambda
                let paths = paths;
                client
                    .get_attributes_from_files_v2(
                        mount_point,
                        &paths,
                        requested_attributes,
                        Some(SyncBehavior::no_sync()),
                        request_scope,
                    )
                    .await
                    .map(|_| ())
            })
        }
    }

    fn request_name(&self) -> &'static str {
        "getAttributesFromFilesV2"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sha1_or_error_into_edenfs_result() {
        let sha1 = Sha1OrError::Sha1(vec![1, 2, 3]);
        let result = sha1.into_edenfs_result();
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), vec![1, 2, 3]);

        let error = Sha1OrError::Error(EdenFsError::Other(anyhow!("error")));
        let result = error.into_edenfs_result();
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().to_string(), "error");

        let unknown_field = Sha1OrError::UnknownField(123);
        let result = unknown_field.into_edenfs_result();
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().to_string(), "Unknown field: 123");
    }
}
