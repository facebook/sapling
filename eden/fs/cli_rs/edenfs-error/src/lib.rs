/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Define common EdenFS errors

use std::path::PathBuf;
use std::result::Result as StdResult;

use thiserror::Error;
use thrift_streaming_clients::errors::StreamJournalChangedError;
use thrift_streaming_clients::errors::StreamStartStatusError;
use thrift_thriftclients::thrift::errors::AddBindMountError;
use thrift_thriftclients::thrift::errors::ChangesSinceV2Error;
use thrift_thriftclients::thrift::errors::EnsureMaterializedError;
use thrift_thriftclients::thrift::errors::FlushStatsNowError;
use thrift_thriftclients::thrift::errors::GetAccessCountsError;
use thrift_thriftclients::thrift::errors::GetAttributesFromFilesV2Error;
use thrift_thriftclients::thrift::errors::GetConfigError;
use thrift_thriftclients::thrift::errors::GetCurrentJournalPositionError;
use thrift_thriftclients::thrift::errors::GetCurrentSnapshotInfoError;
use thrift_thriftclients::thrift::errors::GetDaemonInfoError;
use thrift_thriftclients::thrift::errors::GetFileContentError;
use thrift_thriftclients::thrift::errors::GetSHA1Error;
use thrift_thriftclients::thrift::errors::GetScmStatusV2Error;
use thrift_thriftclients::thrift::errors::GlobFilesError;
use thrift_thriftclients::thrift::errors::ListMountsError;
use thrift_thriftclients::thrift::errors::PredictiveGlobFilesError;
use thrift_thriftclients::thrift::errors::PrefetchFilesError;
use thrift_thriftclients::thrift::errors::PrefetchFilesV2Error;
use thrift_thriftclients::thrift::errors::ReaddirError;
use thrift_thriftclients::thrift::errors::RemoveBindMountError;
use thrift_thriftclients::thrift::errors::RemoveRecursivelyError;
use thrift_thriftclients::thrift::errors::SetPathObjectIdError;
#[cfg(target_os = "macos")]
use thrift_thriftclients::thrift::errors::StartFileAccessMonitorError;
use thrift_thriftclients::thrift::errors::StartRecordingBackingStoreFetchError;
#[cfg(target_os = "macos")]
use thrift_thriftclients::thrift::errors::StopFileAccessMonitorError;
use thrift_thriftclients::thrift::errors::StopRecordingBackingStoreFetchError;
use thrift_thriftclients::thrift::errors::SynchronizeWorkingCopyError;
use thrift_thriftclients::thrift::errors::UnmountError;
use thrift_thriftclients::thrift::errors::UnmountV2Error;
use thrift_types::edenfs::EdenError;
use tokio::time::error::Elapsed;

pub type ExitCode = i32;
pub type Result<T, E = EdenFsError> = std::result::Result<T, E>;

#[derive(Debug, PartialEq, Eq)]
pub enum EdenThriftErrorType {
    PosixError,
    Win32Error,
    HResultError,
    ArgumentError,
    GenericError,
    MountGenerationChangedError,
    JournalTruncatedError,
    CheckoutInProgressError,
    OutOfDateParentError,
    AttributeUnavailable,
    UnknownError,
}

impl From<thrift_types::edenfs::EdenErrorType> for EdenThriftErrorType {
    fn from(from: thrift_types::edenfs::EdenErrorType) -> Self {
        match from {
            thrift_types::edenfs::EdenErrorType::POSIX_ERROR => Self::PosixError,
            thrift_types::edenfs::EdenErrorType::WIN32_ERROR => Self::Win32Error,
            thrift_types::edenfs::EdenErrorType::HRESULT_ERROR => Self::HResultError,
            thrift_types::edenfs::EdenErrorType::ARGUMENT_ERROR => Self::ArgumentError,
            thrift_types::edenfs::EdenErrorType::GENERIC_ERROR => Self::GenericError,
            thrift_types::edenfs::EdenErrorType::MOUNT_GENERATION_CHANGED => {
                Self::MountGenerationChangedError
            }
            thrift_types::edenfs::EdenErrorType::JOURNAL_TRUNCATED => Self::JournalTruncatedError,
            thrift_types::edenfs::EdenErrorType::CHECKOUT_IN_PROGRESS => {
                Self::CheckoutInProgressError
            }
            thrift_types::edenfs::EdenErrorType::OUT_OF_DATE_PARENT => Self::OutOfDateParentError,
            thrift_types::edenfs::EdenErrorType::ATTRIBUTE_UNAVAILABLE => {
                Self::AttributeUnavailable
            }
            _ => Self::UnknownError,
        }
    }
}

#[derive(Debug)]
pub struct ThriftRequestError {
    #[allow(dead_code)]
    pub message: String,
    #[allow(dead_code)]
    pub error_code: Option<i32>,
    #[allow(dead_code)]
    pub error_type: EdenThriftErrorType,
}

impl From<EdenError> for ThriftRequestError {
    fn from(from: EdenError) -> Self {
        Self {
            message: from.message,
            error_code: from.errorCode,
            error_type: from.errorType.into(),
        }
    }
}

pub trait EdenDataIntoEdenFsResult {
    type Data;

    fn into_edenfs_result(self) -> Result<Self::Data, EdenFsError>;
}

#[macro_export]
macro_rules! impl_eden_data_into_edenfs_result {
    ($typ: ident, $data: ty, $ok_variant: ident) => {
        impl EdenDataIntoEdenFsResult for $typ {
            type Data = $data;

            fn into_edenfs_result(self) -> Result<Self::Data, EdenFsError> {
                match self {
                    Self::$ok_variant(data) => Ok(data),
                    Self::Error(e) => Err(e),
                    Self::UnknownField(field) => Err(EdenFsError::Other {
                        0: anyhow::anyhow!("Unknown field: {}", field),
                    }),
                }
            }
        }
    };
}

#[derive(Error, Debug)]
pub enum EdenFsError {
    #[error("Timed out when connecting to EdenFS daemon: {0:?}")]
    ThriftConnectionTimeout(PathBuf),

    #[error("IO error when connecting to EdenFS daemon: {0:?}")]
    ThriftIoError(#[source] std::io::Error),

    #[error("Error when loading configurations: {0}")]
    ConfigurationError(String),

    #[error("EdenFS did not respond within set timeout: {0}")]
    RequestTimeout(Elapsed),

    #[error("The running version of the EdenFS daemon doesn't know that method.")]
    UnknownMethod(String),

    #[error("The Eden Thrift server responded with an EdenError. {0:?}")]
    ThriftRequestError(ThriftRequestError),

    #[error("Encountered an I/O error: {0}")]
    IOError(#[from] std::io::Error),

    #[error("{0}")]
    Other(#[from] anyhow::Error),
}

#[derive(Error, Debug)]
pub enum ConnectAndRequestError<E> {
    #[error(transparent)]
    ConnectionError(ConnectError),
    #[error("Eden Request Failed: {0:?}")]
    RequestError(#[from] E),
}

#[derive(Clone, Debug, Error)]
pub enum ConnectError {
    #[error("Failed to connect to EdenFS daemon: {0}")]
    ConnectionError(String),

    #[error("Failed to wait for daemon to become ready: {0}")]
    DaemonNotReadyError(String),
}

pub trait ResultExt<T> {
    /// Convert any error in a `Result` type into [`EdenFsError`]. Use this when ?-operator can't
    /// automatically infer the type.
    ///
    /// Note: This method will unconditionally convert everything into [`EdenFsError::Other`]
    /// variant even if there is a better match.
    fn from_err(self) -> StdResult<T, EdenFsError>;
}

impl<T, E: std::error::Error + Send + Sync + 'static> ResultExt<T> for StdResult<T, E> {
    fn from_err(self) -> StdResult<T, EdenFsError> {
        self.map_err(|e| EdenFsError::Other(e.into()))
    }
}

#[derive(Copy, Clone, PartialEq, Eq)]
pub enum ErrorHandlingStrategy {
    Reconnect,
    Retry,
    Abort,
}

pub trait HasErrorHandlingStrategy: Send + Sync {
    fn get_error_handling_strategy(&self) -> ErrorHandlingStrategy;
}

impl<E: HasErrorHandlingStrategy> HasErrorHandlingStrategy for ConnectAndRequestError<E> {
    fn get_error_handling_strategy(&self) -> ErrorHandlingStrategy {
        match self {
            Self::ConnectionError(..) => ErrorHandlingStrategy::Reconnect,
            Self::RequestError(e) => e.get_error_handling_strategy(),
        }
    }
}

macro_rules! impl_has_error_handling_strategy {
    ($err: ident) => {
        impl HasErrorHandlingStrategy for $err {
            fn get_error_handling_strategy(&self) -> ErrorHandlingStrategy {
                match self {
                    Self::ThriftError(..) => ErrorHandlingStrategy::Reconnect,
                    Self::ApplicationException(..) => ErrorHandlingStrategy::Retry,
                    Self::ex(..) => ErrorHandlingStrategy::Abort,
                }
            }
        }
    };
}

impl_has_error_handling_strategy!(AddBindMountError);
impl_has_error_handling_strategy!(ChangesSinceV2Error);
impl_has_error_handling_strategy!(EnsureMaterializedError);
impl_has_error_handling_strategy!(FlushStatsNowError);
impl_has_error_handling_strategy!(GetAccessCountsError);
impl_has_error_handling_strategy!(GetAttributesFromFilesV2Error);
impl_has_error_handling_strategy!(GetConfigError);
impl_has_error_handling_strategy!(GetCurrentJournalPositionError);
impl_has_error_handling_strategy!(GetCurrentSnapshotInfoError);
impl_has_error_handling_strategy!(GetDaemonInfoError);
impl_has_error_handling_strategy!(GetFileContentError);
impl_has_error_handling_strategy!(GetScmStatusV2Error);
impl_has_error_handling_strategy!(GetSHA1Error);
impl_has_error_handling_strategy!(GlobFilesError);
impl_has_error_handling_strategy!(ListMountsError);
impl_has_error_handling_strategy!(PredictiveGlobFilesError);
impl_has_error_handling_strategy!(PrefetchFilesError);
impl_has_error_handling_strategy!(PrefetchFilesV2Error);
impl_has_error_handling_strategy!(ReaddirError);
impl_has_error_handling_strategy!(RemoveBindMountError);
impl_has_error_handling_strategy!(RemoveRecursivelyError);
impl_has_error_handling_strategy!(SetPathObjectIdError);
#[cfg(target_os = "macos")]
impl_has_error_handling_strategy!(StartFileAccessMonitorError);
impl_has_error_handling_strategy!(StartRecordingBackingStoreFetchError);
impl_has_error_handling_strategy!(StreamStartStatusError);
#[cfg(target_os = "macos")]
impl_has_error_handling_strategy!(StopFileAccessMonitorError);
impl_has_error_handling_strategy!(StopRecordingBackingStoreFetchError);
impl_has_error_handling_strategy!(SynchronizeWorkingCopyError);
impl_has_error_handling_strategy!(UnmountError);
impl_has_error_handling_strategy!(UnmountV2Error);

macro_rules! impl_has_error_handling_strategy_streaming {
    ($err: ident) => {
        impl HasErrorHandlingStrategy for $err {
            fn get_error_handling_strategy(&self) -> ErrorHandlingStrategy {
                match self {
                    Self::ThriftError(..) => ErrorHandlingStrategy::Reconnect,
                    Self::ApplicationException(..) => ErrorHandlingStrategy::Retry,
                }
            }
        }
    };
}

impl_has_error_handling_strategy_streaming!(StreamJournalChangedError);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_thrift_error_to_eden_error() {
        let error = EdenError {
            message: "test".to_string(),
            errorCode: Some(1),
            errorType: thrift_types::edenfs::EdenErrorType::POSIX_ERROR,
            ..Default::default()
        };
        let result: Result<(), EdenFsError> = Err(EdenFsError::ThriftRequestError(error.into()));
        assert!(result.is_err());
        match result {
            Err(EdenFsError::ThriftRequestError(e)) => {
                assert_eq!(e.message, "test");
                assert_eq!(e.error_code, Some(1));
                assert_eq!(e.error_type, EdenThriftErrorType::PosixError);
            }
            _ => panic!("Expected EdenFsError::ThriftRequestError"),
        }
    }
}
