/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is dual-licensed under either the MIT license found in the
 * LICENSE-MIT file in the root directory of this source tree or the Apache
 * License, Version 2.0 found in the LICENSE-APACHE file in the root directory
 * of this source tree. You may select, at your option, one of the
 * above-listed licenses.
 */

use std::error::Error;
use std::sync::Arc;

use thiserror::Error;

/// SharedError is a simple, cloneable Error wrapper.
/// It holds the inner error in an Arc to support Clone.
///
/// Propagation of errors via `?` converts automatically
/// to SharedError.
///
/// ```
/// use shared_error::std::*;
/// use thiserror::Error;
///
/// #[derive(Debug, Error)]
/// enum SomeErrorType {
///     #[error("Some error variant: {0}")]
///     SomeErrorVariant(String),
/// }
///
/// fn some_fallible_func() -> Result<(), SharedError<SomeErrorType>> {
///     let result: Result<(), SomeErrorType> =
///         Err(SomeErrorType::SomeErrorVariant("some context".to_owned()));
///     Ok(result?)
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
/// }
/// ```
#[derive(Error, Debug)]
#[error(transparent)]
pub struct SharedError<T: Error + 'static> {
    #[from]
    error: Arc<T>,
}

impl<T: Error + 'static> Clone for SharedError<T> {
    fn clone(&self) -> Self {
        Self {
            error: self.error.clone(),
        }
    }
}

impl<T: Error + 'static> From<T> for SharedError<T> {
    fn from(error: T) -> SharedError<T> {
        SharedError {
            error: Arc::new(error),
        }
    }
}

impl<T: Error + 'static> SharedError<T> {
    /// Return reference to the inner Error.
    pub fn inner(&self) -> &T {
        &self.error
    }
}

#[cfg(test)]
mod tests {
    use thiserror::Error;

    use super::*;

    #[derive(Debug, Error)]
    enum TestError {
        #[error("Some error: {0}")]
        SomeError(String),
    }

    #[test]
    fn test_convert_to_shared_error() {
        let error = TestError::SomeError("some context".to_owned());
        let shared_error: SharedError<_> = error.into();
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
        let shared_error: SharedError<_> = error.into();
        let cloned_error = shared_error.clone();
        assert_eq!(
            cloned_error.inner().to_string(),
            "Some error: some context".to_owned()
        );
        assert_eq!(
            cloned_error.to_string(),
            "Some error: some context".to_owned()
        );
        assert!(shared_error.source().is_none());
    }

    #[test]
    fn test_convert_to_result_with_shared_error() {
        fn some_fallible_func() -> Result<(), SharedError<TestError>> {
            let result: Result<(), TestError> =
                Err(TestError::SomeError("some context".to_owned()));
            Ok(result?)
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
