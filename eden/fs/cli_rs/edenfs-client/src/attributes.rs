/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fmt::Display;
use std::str::FromStr;

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Result;
use edenfs_error::EdenFsError;
use thrift_types::edenfs::FileAttributes;
use thrift_types::fbthrift::ThriftEnum;

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

pub fn all_attributes() -> &'static [&'static str] {
    FileAttributes::variants()
}

pub fn file_attributes_from_strings<T>(attrs: &[T]) -> Result<i64>
where
    T: AsRef<str> + Display,
{
    attrs
        .iter()
        .map(|attr| {
            FileAttributes::from_str(attr.as_ref())
                .with_context(|| anyhow!("invalid file attribute: {}", attr))
        })
        .try_fold(0, |acc, x| x.map(|y| acc | y.inner_value() as i64))
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
