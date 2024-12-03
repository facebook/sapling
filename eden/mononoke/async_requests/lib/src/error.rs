/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::backtrace::BacktraceStatus;
use std::convert::Infallible;
use std::error::Error as StdError;
use std::sync::Arc;

use anyhow::Error;
use blobstore::LoadableError;
use megarepo_error::MegarepoError;
use source_control as scs_thrift;
use thiserror::Error;

// The cargo build of anyhow disables its backtrace features when using RUSTC_BOOTSTRAP=1
#[cfg(not(fbcode_build))]
pub static DISABLED: std::backtrace::Backtrace = std::backtrace::Backtrace::disabled();

#[macro_export]
macro_rules! cloneable_error {
    ($name: ident) => {
        #[derive(Clone, Debug)]
        pub struct $name(pub ::std::sync::Arc<anyhow::Error>);

        impl $name {
            #[cfg(fbcode_build)]
            pub fn backtrace(&self) -> &::std::backtrace::Backtrace {
                self.0.backtrace()
            }

            #[cfg(not(fbcode_build))]
            pub fn backtrace(&self) -> &::std::backtrace::Backtrace {
                &$crate::error::DISABLED
            }
        }

        impl ::std::fmt::Display for $name {
            fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                self.0.fmt(f)
            }
        }

        impl From<anyhow::Error> for $name {
            fn from(error: anyhow::Error) -> Self {
                Self(::std::sync::Arc::new(error))
            }
        }

        impl ::std::error::Error for $name {
            fn source(&self) -> Option<&(dyn ::std::error::Error + 'static)> {
                Some(&**self.0)
            }

            #[cfg(fbcode_build)]
            fn provide<'a>(&'a self, request: &mut ::std::error::Request<'a>) {
                request.provide_ref::<::std::backtrace::Backtrace>(self.backtrace());
            }
        }
    };
}

cloneable_error!(InternalError);
cloneable_error!(RequestError);

#[derive(Clone, Debug, Error)]
pub enum AsyncRequestsError {
    #[error("{0}")]
    RequestError(#[source] RequestError),
    #[error("{0}")]
    InternalError(#[source] InternalError),
}

impl AsyncRequestsError {
    pub fn internal(e: impl Into<Error>) -> Self {
        Self::InternalError(InternalError::from(e.into()))
    }

    pub fn request(e: impl Into<Error>) -> Self {
        Self::RequestError(RequestError::from(e.into()))
    }
}

/// By default, let's treat errors as internal
impl From<Error> for AsyncRequestsError {
    fn from(e: Error) -> Self {
        match e.downcast::<AsyncRequestsError>() {
            Ok(megarepo_error) => match megarepo_error {
                Self::RequestError(e) => Self::RequestError(e),
                Self::InternalError(e) => Self::InternalError(e),
            },
            Err(orig) => Self::internal(orig),
        }
    }
}

impl From<Infallible> for AsyncRequestsError {
    fn from(_i: Infallible) -> Self {
        unreachable!()
    }
}

impl From<LoadableError> for AsyncRequestsError {
    fn from(e: LoadableError) -> Self {
        AsyncRequestsError::InternalError(InternalError(Arc::new(e.into())))
    }
}

#[macro_export]
macro_rules! bail_request {
    ($msg:literal $(,)?) => {
        return Err($crate::AsyncRequestsError::RequestError($crate::RequestError(::std::sync::Arc::new($crate::macro_reexport::anyhow!($msg)))))
    };
    ($fmt:expr, $($arg:tt)*) => {
        return Err($crate::AsyncRequestsError::RequestError($crate::RequestError(::std::sync::Arc::new($crate::macro_reexport::anyhow!($fmt, $($arg)*)))))
    };
}

#[macro_export]
macro_rules! bail_internal {
    ($msg:literal $(,)?) => {
        return Err($crate::AsyncRequestsError::InternalError($crate::InternalError(::std::sync::Arc::new($crate::macro_reexport::anyhow!($msg)))))
    };
    ($fmt:expr, $($arg:tt)*) => {
        return Err($crate::AsyncRequestsError::InternalError($crate::InternalError(::std::sync::Arc::new($crate::macro_reexport::anyhow!($fmt, $($arg)*)))))
    };
}

impl From<AsyncRequestsError> for scs_thrift::AsyncRequestError {
    fn from(e: AsyncRequestsError) -> Self {
        match e {
            AsyncRequestsError::RequestError(e) => {
                Self::request_error(scs_thrift::RequestErrorStruct {
                    kind: scs_thrift::RequestErrorKind::INVALID_REQUEST,
                    reason: format!("{}", e),
                    ..Default::default()
                })
            }
            AsyncRequestsError::InternalError(error) => {
                let reason = error.to_string();
                let backtrace = match error.backtrace().status() {
                    BacktraceStatus::Captured => Some(error.backtrace().to_string()),
                    _ => None,
                };
                let mut source_chain = Vec::new();
                let mut error: &dyn StdError = &error;
                while let Some(source) = error.source() {
                    source_chain.push(source.to_string());
                    error = source;
                }

                Self::internal_error(scs_thrift::InternalErrorStruct {
                    reason,
                    backtrace,
                    source_chain,
                    ..Default::default()
                })
            }
        }
    }
}

impl From<scs_thrift::AsyncRequestError> for AsyncRequestsError {
    fn from(e: scs_thrift::AsyncRequestError) -> Self {
        match e {
            scs_thrift::AsyncRequestError::request_error(e) => Self::RequestError(RequestError(
                Arc::new(anyhow::anyhow!("RequestError: {}", e.reason)),
            )),
            scs_thrift::AsyncRequestError::internal_error(e) => {
                let mut error = anyhow::anyhow!("InternalError: {}", e.reason);
                if let Some(backtrace) = e.backtrace {
                    error = error.context(backtrace);
                }
                if !e.source_chain.is_empty() {
                    error = error.context(format!("Source chain: {:?}", e.source_chain));
                }
                Self::InternalError(InternalError(Arc::new(error)))
            }
            scs_thrift::AsyncRequestError::UnknownField(_) => {
                unreachable!("Unknown field in AsyncRequestError")
            }
        }
    }
}

impl From<AsyncRequestsError> for scs_thrift::MegarepoAsynchronousRequestError {
    fn from(e: AsyncRequestsError) -> Self {
        match e {
            AsyncRequestsError::RequestError(e) => {
                Self::request_error(scs_thrift::RequestErrorStruct {
                    kind: scs_thrift::RequestErrorKind::INVALID_REQUEST,
                    reason: format!("{}", e),
                    ..Default::default()
                })
            }
            AsyncRequestsError::InternalError(error) => {
                let reason = error.to_string();
                let backtrace = match error.backtrace().status() {
                    BacktraceStatus::Captured => Some(error.backtrace().to_string()),
                    _ => None,
                };
                let mut source_chain = Vec::new();
                let mut error: &dyn StdError = &error;
                while let Some(source) = error.source() {
                    source_chain.push(source.to_string());
                    error = source;
                }

                Self::internal_error(scs_thrift::InternalErrorStruct {
                    reason,
                    backtrace,
                    source_chain,
                    ..Default::default()
                })
            }
        }
    }
}

impl From<AsyncRequestsError> for MegarepoError {
    fn from(e: AsyncRequestsError) -> Self {
        match e {
            AsyncRequestsError::RequestError(e) => Self::request(e),
            AsyncRequestsError::InternalError(e) => Self::internal(e),
        }
    }
}

impl From<MegarepoError> for AsyncRequestsError {
    fn from(e: MegarepoError) -> Self {
        match e {
            MegarepoError::RequestError(e) => Self::request(e),
            MegarepoError::InternalError(e) => Self::internal(e),
        }
    }
}
