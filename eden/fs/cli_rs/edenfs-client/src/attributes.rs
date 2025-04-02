/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! File attribute handling for EdenFS.
//!
//! This module provides types and functions for working with file attributes in EdenFS.
//! It allows querying various attributes of files such as SHA1 hash, size, source control type,
//! and more.
//!
//! # Examples
//!
//! ## Getting all available attribute names
//!
//! ```
//! use edenfs_client::attributes;
//!
//! // Get a list of all available attribute names
//! let all_attrs = attributes::all_attributes();
//! println!("Available attributes: {:?}", all_attrs);
//! ```
//!
//! ## Converting attribute names to a bitmask
//!
//! ```
//! use edenfs_client::attributes;
//!
//! // Convert a list of attribute names to a bitmask
//! let attrs = ["SHA1_HASH", "SIZE", "SOURCE_CONTROL_TYPE"];
//! match attributes::file_attributes_from_strings(&attrs) {
//!     Ok(bitmask) => println!("Attribute bitmask: {}", bitmask),
//!     Err(e) => eprintln!("Error: {}", e),
//! }
//! ```
//!
//! ## Getting a bitmask for all attributes
//!
//! ```
//! use edenfs_client::attributes;
//!
//! // Get a bitmask representing all available attributes
//! let all_attrs_bitmask = attributes::all_attributes_as_bitmask();
//! println!("All attributes bitmask: {}", all_attrs_bitmask);
//! ```

use std::fmt::Display;
use std::path::Path;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;

use anyhow::anyhow;
use edenfs_error::EdenFsError;
use edenfs_error::Result;
use edenfs_utils::bytes_from_path;
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

/// Represents either a SHA1 hash or an error.
///
/// This enum is used to represent the result of a SHA1 hash request, which can
/// either be a successful hash value or an error.
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

impl Default for AttributesRequestScope {
    fn default() -> Self {
        Self::TreesAndFiles
    }
}

#[repr(i32)]
#[derive(Debug, Clone, PartialEq)]
enum FileAttributes {
    None = 0,
    Sha1 = 1,
    FileSize = 2,
    SourceControlType = 4,
    ObjectId = 8,
    Blake3 = 16,
    DigestSize = 32,
    DigestHash = 64,
}

impl From<FileAttributes> for thrift_types::edenfs::FileAttributes {
    fn from(from: FileAttributes) -> Self {
        match from {
            FileAttributes::None => Self::NONE,
            FileAttributes::Sha1 => Self::SHA1_HASH,
            FileAttributes::FileSize => Self::FILE_SIZE,
            FileAttributes::SourceControlType => Self::SOURCE_CONTROL_TYPE,
            FileAttributes::ObjectId => Self::OBJECT_ID,
            FileAttributes::Blake3 => Self::BLAKE3_HASH,
            FileAttributes::DigestSize => Self::DIGEST_SIZE,
            FileAttributes::DigestHash => Self::DIGEST_HASH,
        }
    }
}

impl From<thrift_types::edenfs::FileAttributes> for FileAttributes {
    fn from(from: thrift_types::edenfs::FileAttributes) -> Self {
        match from {
            thrift_types::edenfs::FileAttributes::NONE => Self::None,
            thrift_types::edenfs::FileAttributes::SHA1_HASH => Self::Sha1,
            thrift_types::edenfs::FileAttributes::FILE_SIZE => Self::FileSize,
            thrift_types::edenfs::FileAttributes::SOURCE_CONTROL_TYPE => Self::SourceControlType,
            thrift_types::edenfs::FileAttributes::OBJECT_ID => Self::ObjectId,
            thrift_types::edenfs::FileAttributes::BLAKE3_HASH => Self::Blake3,
            thrift_types::edenfs::FileAttributes::DIGEST_SIZE => Self::DigestSize,
            thrift_types::edenfs::FileAttributes::DIGEST_HASH => Self::DigestHash,
            _ => Self::None,
        }
    }
}

impl FromStr for FileAttributes {
    type Err = EdenFsError;
    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "None" => Ok(Self::None),
            "Sha1" => Ok(Self::Sha1),
            "FileSize" => Ok(Self::FileSize),
            "SourceControlType" => Ok(Self::SourceControlType),
            "ObjectId" => Ok(Self::ObjectId),
            "Blake3" => Ok(Self::Blake3),
            "DigestSize" => Ok(Self::DigestSize),
            "DigestHash" => Ok(Self::DigestHash),
            _ => Err(EdenFsError::Other(anyhow!(
                "invalid file attribute: {:?}",
                s
            ))),
        }
    }
}

/// Converts a slice of `FileAttributes` to a bitmask.
///
/// This function takes a slice of `FileAttributes` and returns a bitmask
/// representing those attributes.
///
/// # Parameters
///
/// * `attrs` - A slice of `FileAttributes` to convert to a bitmask.
///
/// # Returns
///
/// A bitmask representing the given attributes.
fn attributes_as_bitmask(attrs: &[FileAttributes]) -> i64 {
    attrs.iter().fold(0, |acc, x| acc | x.clone() as i64)
}

/// Returns a bitmask representing all available file attributes.
///
/// This function returns a bitmask that includes all available file attributes.
///
/// # Returns
///
/// A bitmask representing all available file attributes.
///
/// # Examples
///
/// ```
/// use edenfs_client::attributes;
///
/// let all_attrs_bitmask = attributes::all_attributes_as_bitmask();
/// println!("All attributes bitmask: {}", all_attrs_bitmask);
/// ```
pub fn all_attributes_as_bitmask() -> i64 {
    let vals: Vec<FileAttributes> = thrift_types::edenfs::FileAttributes::variant_values()
        .iter()
        .map(|v| v.clone().into())
        .collect();
    attributes_as_bitmask(&vals)
}

/// Returns a slice of all available file attribute names.
///
/// This function returns a slice containing the names of all available file attributes.
///
/// # Returns
///
/// A slice of strings representing all available file attribute names.
///
/// # Examples
///
/// ```
/// use edenfs_client::attributes;
///
/// let all_attrs = attributes::all_attributes();
/// println!("Available attributes: {:?}", all_attrs);
/// ```
pub fn all_attributes() -> &'static [&'static str] {
    thrift_types::edenfs::FileAttributes::variants()
}

/// Converts a slice of attribute names to a bitmask.
///
/// This function takes a slice of attribute names and returns a bitmask
/// representing those attributes.
///
/// # Parameters
///
/// * `attrs` - A slice of attribute names to convert to a bitmask.
///
/// # Returns
///
/// A `Result` containing a bitmask representing the given attributes, or an error
/// if any of the attribute names are invalid.
///
/// # Examples
///
/// ```
/// use edenfs_client::attributes;
///
/// // Convert a list of attribute names to a bitmask
/// let attrs = ["Sha1", "FileSize", "SourceControlType"];
/// match attributes::file_attributes_from_strings(&attrs) {
///     Ok(bitmask) => println!("Attribute bitmask: {}", bitmask),
///     Err(e) => eprintln!("Error: {}", e),
/// }
///
/// // Invalid attribute names will result in an error
/// let invalid_attrs = ["invalid"];
/// assert!(attributes::file_attributes_from_strings(&invalid_attrs).is_err());
/// ```
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
    async fn get_attributes_from_files_v2<P, S>(
        &self,
        mount_point: P,
        paths: &[S],
        requested_attributes: i64,
        sync: Option<SyncBehavior>,
        scope: Option<AttributesRequestScope>,
    ) -> Result<GetAttributesFromFilesResultV2>
    where
        P: AsRef<Path>,
        S: AsRef<str>,
    {
        let params = thrift_types::edenfs::GetAttributesFromFilesParams {
            mountPoint: bytes_from_path(mount_point.as_ref().to_path_buf())?,
            requestedAttributes: requested_attributes,
            paths: paths
                .iter()
                .map(|s| s.as_ref().as_bytes().to_vec())
                .collect(),
            sync: sync.map(Into::into).unwrap_or_default(),
            scope: scope.map(Into::into),
            ..Default::default()
        };
        self.with_thrift(|thrift| thrift.getAttributesFromFilesV2(&params))
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
    /// use edenfs_client::attributes::GetAttributesV2Request;
    ///
    /// // Create a request for getting SHA1 and size attributes for two files
    /// let mount_path = PathBuf::from("/path/to/mount");
    /// let paths = ["file1.txt", "file2.txt"];
    /// let attrs = ["Sha1", "FileSize"];
    /// let request = GetAttributesV2Request::new(mount_path, &paths, &attrs);
    /// ```
    pub fn new<P, S>(mount_path: PathBuf, paths: &[P], requested_attributes: &[S]) -> Self
    where
        P: AsRef<str>,
        S: AsRef<str> + Display,
    {
        Self {
            mount_point: mount_path,
            paths: paths.iter().map(|p| p.as_ref().into()).collect(),
            requested_attributes: file_attributes_from_strings(requested_attributes)
                .unwrap_or_else(|e| {
                    tracing::error!("failed to convert attributes to bitmap: {:?}", e);
                    tracing::info!("defaulting to requesting all attributes in getAttributesFromFilesV2 requests");
                    all_attributes_as_bitmask()
        }),
        }
    }
}

impl RequestFactory for GetAttributesV2Request {
    fn make_request(&self) -> impl FnOnce(RequestParam) -> RequestResult {
        let mount_point = self.mount_point.clone();
        let paths = self.paths.clone();
        let requested_attributes = self.requested_attributes;
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
                        None,
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
mod test {
    use super::*;

    #[test]
    fn test_attributes_from_strings() -> Result<()> {
        assert_eq!(file_attributes_from_strings::<String>(&[])?, 0);
        assert_eq!(
            file_attributes_from_strings(&["Sha1", "SourceControlType"])?,
            FileAttributes::Sha1 as i64 | FileAttributes::SourceControlType as i64
        );
        assert!(file_attributes_from_strings(&["Invalid"]).is_err());
        Ok(())
    }
}
