/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![feature(backtrace)]

use blobstore::LoadableError;
use source_control as scs_thrift;
use std::backtrace::Backtrace;
use std::backtrace::BacktraceStatus;
use std::convert::Infallible;
use std::error::Error as StdError;
use std::fmt;
use std::sync::Arc;

use anyhow::Error;
use thiserror::Error;

pub mod macro_reexport {
    pub use anyhow::anyhow;
}

macro_rules! cloneable_error {
    ($name: ident) => {
        #[derive(Clone, Debug)]
        pub struct $name(pub ::std::sync::Arc<Error>);

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(f)
            }
        }

        impl From<Error> for $name {
            fn from(error: Error) -> Self {
                Self(::std::sync::Arc::new(error))
            }
        }

        impl StdError for $name {
            fn source(&self) -> Option<&(dyn StdError + 'static)> {
                Some(&**self.0)
            }

            fn backtrace(&self) -> Option<&Backtrace> {
                Some(self.0.backtrace())
            }
        }
    };
}

cloneable_error!(InternalError);
cloneable_error!(RequestError);

#[derive(Clone, Debug, Error)]
pub enum MegarepoError {
    #[error("{0}")]
    RequestError(#[source] RequestError),
    #[error("{0}")]
    InternalError(#[source] InternalError),
}

impl MegarepoError {
    pub fn internal(e: impl Into<Error>) -> Self {
        Self::InternalError(InternalError::from(e.into()))
    }

    pub fn request(e: impl Into<Error>) -> Self {
        Self::RequestError(RequestError::from(e.into()))
    }
}

/// By default, let's treat errors as internal
impl From<Error> for MegarepoError {
    fn from(e: Error) -> Self {
        match e.downcast::<MegarepoError>() {
            Ok(megarepo_error) => match megarepo_error {
                Self::RequestError(e) => Self::RequestError(e),
                Self::InternalError(e) => Self::InternalError(e),
            },
            Err(orig) => Self::internal(orig),
        }
    }
}

impl From<Infallible> for MegarepoError {
    fn from(_i: Infallible) -> Self {
        unreachable!()
    }
}

impl From<LoadableError> for MegarepoError {
    fn from(e: LoadableError) -> Self {
        MegarepoError::InternalError(InternalError(Arc::new(e.into())))
    }
}

#[macro_export]
macro_rules! bail_request {
    ($msg:literal $(,)?) => {
        return Err($crate::MegarepoError::RequestError($crate::RequestError(::std::sync::Arc::new($crate::macro_reexport::anyhow!($msg)))))
    };
    ($fmt:expr, $($arg:tt)*) => {
        return Err($crate::MegarepoError::RequestError($crate::RequestError(::std::sync::Arc::new($crate::macro_reexport::anyhow!($fmt, $($arg)*)))))
    };
}

#[macro_export]
macro_rules! bail_internal {
    ($msg:literal $(,)?) => {
        return Err($crate::MegarepoError::InternalError($crate::InternalError(::std::sync::Arc::new($crate::macro_reexport::anyhow!($msg)))))
    };
    ($fmt:expr, $($arg:tt)*) => {
        return Err($crate::MegarepoError::InternalError($crate::InternalError(::std::sync::Arc::new($crate::macro_reexport::anyhow!($fmt, $($arg)*)))))
    };
}

impl From<MegarepoError> for scs_thrift::MegarepoAsynchronousRequestError {
    fn from(e: MegarepoError) -> Self {
        match e {
            MegarepoError::RequestError(e) => Self::request_error(scs_thrift::RequestErrorStruct {
                kind: scs_thrift::RequestErrorKind::INVALID_REQUEST,
                reason: format!("{}", e),
                ..Default::default()
            }),
            MegarepoError::InternalError(error) => {
                let reason = error.to_string();
                let backtrace = error
                    .backtrace()
                    .and_then(|backtrace| match backtrace.status() {
                        BacktraceStatus::Captured => Some(backtrace.to_string()),
                        _ => None,
                    });
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
