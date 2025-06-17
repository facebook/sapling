/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under both the MIT license found in the
 * LICENSE-MIT file in the root directory of this source tree and the Apache
 * License, Version 2.0 found in the LICENSE-APACHE file in the root directory
 * of this source tree.
 */

use std::sync::Arc;

use anyhow::Error;
use thiserror::Error;

/// SharedError is a simple, cloneable `anyhow::Error` wrapper.
/// It holds the inner error in an `Arc<anyhow::Error>` to support Clone.
///
/// Propagation of errors via `?` converts automatically
/// to SharedError.
///
/// ```
/// use ::anyhow::Error;
/// use shared_error::anyhow::*;
/// use thiserror::Error;
///
/// #[derive(Debug, Error)]
/// enum SomeErrorType {
///     #[error("Some error variant: {0}")]
///     SomeErrorVariant(String),
/// }
///
/// fn some_fallible_func() -> Result<(), SharedError> {
///     let result: Result<(), SomeErrorType> =
///         Err(SomeErrorType::SomeErrorVariant("some context".to_owned()));
///     Ok(result.shared_error()?)
/// }
///
/// fn some_fallible_func_anyhow() -> Result<(), SharedError> {
///     let result: Result<(), Error> =
///         Err(SomeErrorType::SomeErrorVariant("some context".to_owned()).into());
///     Ok(result.shared_error()?)
/// }
///
/// fn some_caller() {
///     let result = some_fallible_func();
///     match result {
///         Ok(_) => { /* do something */ }
///         Err(shared_error) => {
///             // some_func_1_that_consumes_error(shared_error.clone());
///             // ...
///             // some_func_N_that_consumes_error(shared_error.clone());
///         }
///     }
///     let _result = some_fallible_func_anyhow();
/// }
/// ```
#[derive(Error, Debug, Clone)]
#[error(transparent)]
pub struct SharedError {
    #[from]
    error: Arc<Error>,
}

impl SharedError {
    /// Return reference to the inner Error.
    pub fn inner(&self) -> &Error {
        &self.error
    }

    /// Creates a new arced error
    pub fn new_arcederror(error: Arc<anyhow::Error>) -> Self {
        Self { error }
    }
}

/// Trait to convert std and anyhow Errors into SharedError.
pub trait IntoSharedError<Ret> {
    /// Method to convert std and anyhow Errors into SharedError.
    fn shared_error(self) -> Ret;
}

impl<E: Into<Error>> IntoSharedError<SharedError> for E {
    fn shared_error(self) -> SharedError {
        SharedError {
            error: Arc::new(self.into()),
        }
    }
}

impl<T, E: Into<Error>> IntoSharedError<Result<T, SharedError>> for Result<T, E> {
    fn shared_error(self) -> Result<T, SharedError> {
        self.map_err(|err| err.shared_error())
    }
}

impl slog::KV for SharedError {
    fn serialize(
        &self,
        _record: &slog::Record<'_>,
        serializer: &mut dyn slog::Serializer,
    ) -> slog::Result {
        serializer.emit_str("error", &format!("{self}"))?;
        serializer.emit_str("error_debug", &format!("{self:#?}"))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::error::Error as _;

    use super::*;

    #[derive(Debug, Error)]
    enum TestError {
        #[error("Some error: {0}")]
        SomeError(String),
    }

    #[test]
    fn test_convert_to_shared_error() {
        let error = TestError::SomeError("some context".to_owned());
        let shared_error: SharedError = error.shared_error();
        assert_eq!(
            shared_error.inner().to_string(),
            "Some error: some context".to_owned()
        );
        assert_eq!(
            shared_error.to_string(),
            "Some error: some context".to_owned()
        );
        assert!(shared_error.source().is_none());
    }

    #[test]
    #[allow(clippy::redundant_clone)]
    fn test_clone_shared_error() {
        let error = TestError::SomeError("some context".to_owned());
        let shared_error: SharedError = error.shared_error();
        let cloned_error = shared_error.clone();
        assert_eq!(
            cloned_error.inner().to_string(),
            "Some error: some context".to_owned()
        );
        assert_eq!(
            cloned_error.to_string(),
            "Some error: some context".to_owned()
        );
        assert!(cloned_error.source().is_none());
    }

    #[test]
    fn test_convert_to_result_with_shared_error() {
        fn some_fallible_func() -> Result<(), SharedError> {
            let result: Result<(), TestError> =
                Err(TestError::SomeError("some context".to_owned()));
            result.shared_error()
        }

        let shared_error_result = some_fallible_func();
        match shared_error_result {
            Ok(_) => panic!("Can't be an Ok result"),
            Err(shared_error) => {
                assert_eq!(
                    shared_error.to_string(),
                    "Some error: some context".to_owned()
                );
                assert!(shared_error.source().is_none());
            }
        }
    }

    #[test]
    fn test_convert_to_result_with_shared_error_anyhow() {
        fn some_fallible_func() -> Result<(), SharedError> {
            let result: Result<(), Error> =
                Err(TestError::SomeError("some context".to_owned()).into());
            result.shared_error()
        }

        let shared_error_result = some_fallible_func();
        match shared_error_result {
            Ok(_) => panic!("Can't be an Ok result"),
            Err(shared_error) => {
                assert_eq!(
                    shared_error.to_string(),
                    "Some error: some context".to_owned()
                );
                assert!(shared_error.source().is_none());
            }
        }
    }
}
