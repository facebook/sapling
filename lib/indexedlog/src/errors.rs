// Copyright 2019 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! Errors used by the crate

// Pattern taken from https://rust-lang-nursery.github.io/failure/string-custom-error.html
macro_rules! define_error {
    ($name: ident, $doc: expr) => {
        #[doc =$doc]
        #[derive(Debug)]
        pub struct $name {
            inner: ::failure::Context<String>,
        }

        impl ::failure::Fail for $name {
            fn cause(&self) -> Option<&::failure::Fail> {
                self.inner.cause()
            }

            fn backtrace(&self) -> Option<&::failure::Backtrace> {
                self.inner.backtrace()
            }
        }

        impl ::std::fmt::Display for $name {
            fn fmt(&self, f: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
                ::std::fmt::Display::fmt(&self.inner, f)
            }
        }

        impl From<String> for $name {
            fn from(msg: String) -> $name {
                $name {
                    inner: ::failure::Context::new(msg),
                }
            }
        }

        impl From<&'static str> for $name {
            fn from(msg: &'static str) -> $name {
                $name {
                    inner: ::failure::Context::new(msg.to_string()),
                }
            }
        }

        impl From<::failure::Context<String>> for $name {
            fn from(inner: ::failure::Context<String>) -> $name {
                $name { inner }
            }
        }
    };
}

define_error!(
    DataError,
    "An internal assumption about data went wrong. Most likely caused by filesystem corruption."
);
define_error!(ParameterError, "Parameter provided is invalid.");

pub(crate) fn parameter_error(msg: impl AsRef<str>) -> failure::Error {
    ParameterError::from(msg.as_ref().to_string()).into()
}

pub(crate) fn data_error(msg: impl AsRef<str>) -> failure::Error {
    DataError::from(format!("data corruption: {}", msg.as_ref())).into()
}
