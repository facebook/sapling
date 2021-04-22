/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![feature(backtrace)]
#![deny(warnings)]

pub use anyhow::anyhow;
use blobstore::LoadableError;
use std::backtrace::Backtrace;
use std::convert::Infallible;
use std::error::Error as StdError;
use std::fmt;
pub use std::sync::Arc;

use anyhow::Error;
use thiserror::Error;

macro_rules! cloneable_error {
    ($name: ident) => {
        #[derive(Clone, Debug)]
        pub struct $name(pub Arc<Error>);

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(f)
            }
        }

        impl From<Error> for $name {
            fn from(error: Error) -> Self {
                Self(Arc::new(error))
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
        return Err($crate::MegarepoError::RequestError($crate::RequestError($crate::Arc::new($crate::anyhow!($msg)))))
    };
    ($fmt:expr, $($arg:tt)*) => {
        return Err($crate::MegarepoError::RequestError($crate::RequestError($crate::Arc::new($crate::anyhow!($fmt, $($arg)*)))))
    };
}

#[macro_export]
macro_rules! bail_internal {
    ($msg:literal $(,)?) => {
        return Err($crate::MegarepoError::InternalError($crate::InternalError($crate::Arc::new($crate::anyhow!($msg)))))
    };
    ($fmt:expr, $($arg:tt)*) => {
        return Err($crate::MegarepoError::InternalError($crate::InternalError($crate::Arc::new($crate::anyhow!($fmt, $($arg)*)))))
    };
}
